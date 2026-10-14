use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

pub const SHM_MAGIC: u32 = 0x564C414E; // "VLAN"
pub const SHM_VERSION: u32 = 1;
pub const RING_CAPACITY: usize = 512;

/// Get the secure default directory for VLang shared memory segments.
/// Follows standard security hierarchy: $XDG_RUNTIME_DIR/vlang -> ~/.vlang/shm -> /tmp/vlang_runtime_{uid}
pub fn default_shm_dir() -> PathBuf {
    #[cfg(unix)]
    {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            let path = PathBuf::from(runtime_dir).join("vlang");
            if ensure_secure_dir(&path).is_ok() {
                return path;
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home).join(".vlang").join("shm");
            if ensure_secure_dir(&path).is_ok() {
                return path;
            }
        }
        let uid = unsafe { libc::getuid() };
        let path = std::env::temp_dir().join(format!("vlang_runtime_{}", uid));
        let _ = ensure_secure_dir(&path);
        path
    }
    #[cfg(not(unix))]
    {
        let path = std::env::temp_dir().join("vlang_shm");
        let _ = std::fs::create_dir_all(&path);
        path
    }
}

pub fn default_shm_path(pid: u32) -> PathBuf {
    default_shm_dir().join(format!("vlang_shm_{}.dat", pid))
}

pub fn ensure_secure_dir(path: &Path) -> io::Result<()> {
    if !path.exists() {
        #[cfg(unix)]
        {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            builder.mode(0o700);
            builder.create(path)?;
        }
        #[cfg(not(unix))]
        {
            std::fs::create_dir_all(path)?;
        }
    } else {
        #[cfg(unix)]
        {
            let perms = std::fs::Permissions::from_mode(0o700);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
    Ok(())
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryEventType {
    AllocRegionA = 0,
    FreeRegionA = 1,
    PersistToRegionB = 2,
    BudgetExceeded = 3,
    Heartbeat = 4,
}

impl From<u8> for TelemetryEventType {
    fn from(val: u8) -> Self {
        match val {
            0 => TelemetryEventType::AllocRegionA,
            1 => TelemetryEventType::FreeRegionA,
            2 => TelemetryEventType::PersistToRegionB,
            3 => TelemetryEventType::BudgetExceeded,
            _ => TelemetryEventType::Heartbeat,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TelemetryRecord {
    pub seq: u64, // Seqlock counter: odd = writing, even = stable
    pub timestamp_ns: u64,
    pub event_type: u8,
    pub _reserved: [u8; 3],
    pub bytes: u32,
    pub line: u32,
    pub col: u32,
    pub label: [u8; 32],
}

impl Default for TelemetryRecord {
    fn default() -> Self {
        Self {
            seq: 0,
            timestamp_ns: 0,
            event_type: 0,
            _reserved: [0; 3],
            bytes: 0,
            line: 0,
            col: 0,
            label: [0; 32],
        }
    }
}

impl TelemetryRecord {
    pub fn get_label(&self) -> String {
        let len = self.label.iter().position(|&b| b == 0).unwrap_or(32);
        String::from_utf8_lossy(&self.label[..len]).to_string()
    }

    pub fn set_label(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let copy_len = bytes.len().min(31);
        self.label[..copy_len].copy_from_slice(&bytes[..copy_len]);
        self.label[copy_len] = 0;
    }
}

#[repr(C)]
pub struct ShmHeader {
    pub magic: u32,
    pub version: u32,
    pub worker_pid: u32,
    pub ring_capacity: u32,

    pub region_a_used_bytes: AtomicU64,
    pub region_a_budget_bytes: AtomicU64,
    pub region_a_alloc_count: AtomicU64,

    pub region_b_live_bytes: AtomicU64,
    pub region_b_budget_bytes: AtomicU64,
    pub region_b_alloc_count: AtomicU64,
    pub region_b_live_objects: AtomicU64,

    pub last_heartbeat_ns: AtomicU64,
    pub ring_head: AtomicU32,
    pub ring_tail: AtomicU32,
}

impl ShmHeader {
    pub fn init_new(worker_pid: u32) -> Self {
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        Self {
            magic: SHM_MAGIC,
            version: SHM_VERSION,
            worker_pid,
            ring_capacity: RING_CAPACITY as u32,
            region_a_used_bytes: AtomicU64::new(0),
            region_a_budget_bytes: AtomicU64::new(16 * 1024 * 1024),
            region_a_alloc_count: AtomicU64::new(0),
            region_b_live_bytes: AtomicU64::new(0),
            region_b_budget_bytes: AtomicU64::new(512 * 1024 * 1024),
            region_b_alloc_count: AtomicU64::new(0),
            region_b_live_objects: AtomicU64::new(0),
            last_heartbeat_ns: AtomicU64::new(now_ns),
            ring_head: AtomicU32::new(0),
            ring_tail: AtomicU32::new(0),
        }
    }
}

pub struct SharedTelemetryRegion {
    pub path: PathBuf,
    pub file: File,
    raw_ptr: *mut u8,
    size: usize,
}

// Safety: raw_ptr points to an OS-managed shared file mapping containing atomic types
unsafe impl Send for SharedTelemetryRegion {}
unsafe impl Sync for SharedTelemetryRegion {}

impl SharedTelemetryRegion {
    pub fn total_size() -> usize {
        std::mem::size_of::<ShmHeader>() + RING_CAPACITY * std::mem::size_of::<TelemetryRecord>()
    }

    pub fn create_or_open(path: impl AsRef<Path>, worker_pid: u32) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            let _ = ensure_secure_dir(parent);
        }
        let size = Self::total_size();

        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }

        let mut file = options.open(&path)?;
        #[cfg(unix)]
        {
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }

        let current_len = file.metadata()?.len() as usize;
        if current_len < size {
            file.seek(SeekFrom::Start(size as u64 - 1))?;
            file.write_all(&[0])?;
            file.flush()?;

            // Initialize header
            let header = ShmHeader::init_new(worker_pid);
            file.seek(SeekFrom::Start(0))?;
            let header_bytes = unsafe {
                std::slice::from_raw_parts(
                    &header as *const ShmHeader as *const u8,
                    std::mem::size_of::<ShmHeader>(),
                )
            };
            file.write_all(header_bytes)?;
            file.flush()?;
        }

        // Map memory via mmap
        let raw_ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                std::os::fd::AsRawFd::as_raw_fd(&file),
                0,
            )
        };

        if raw_ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            path,
            file,
            raw_ptr: raw_ptr as *mut u8,
            size,
        })
    }

    pub fn open_existing(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let size = Self::total_size();

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)?;

        let raw_ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                std::os::fd::AsRawFd::as_raw_fd(&file),
                0,
            )
        };

        if raw_ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        let region = Self {
            path,
            file,
            raw_ptr: raw_ptr as *mut u8,
            size,
        };

        let header = region.header();
        if header.magic != SHM_MAGIC || header.version != SHM_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid shared memory magic or version",
            ));
        }

        Ok(region)
    }

    pub fn header(&self) -> &ShmHeader {
        unsafe { &*(self.raw_ptr as *const ShmHeader) }
    }

    pub fn header_mut(&self) -> &mut ShmHeader {
        unsafe { &mut *(self.raw_ptr as *mut ShmHeader) }
    }

    pub fn records_ptr(&self) -> *mut TelemetryRecord {
        unsafe { self.raw_ptr.add(std::mem::size_of::<ShmHeader>()) as *mut TelemetryRecord }
    }

    pub fn push_event(
        &self,
        event_type: TelemetryEventType,
        bytes: u32,
        line: u32,
        col: u32,
        label: &str,
    ) {
        let header = self.header();
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        header.last_heartbeat_ns.store(now_ns, Ordering::Relaxed);

        let head = header.ring_head.fetch_add(1, Ordering::SeqCst) as usize % RING_CAPACITY;
        let mut record = TelemetryRecord {
            seq: 0,
            timestamp_ns: now_ns,
            event_type: event_type as u8,
            _reserved: [0; 3],
            bytes,
            line,
            col,
            label: [0; 32],
        };
        record.set_label(label);

        unsafe {
            let slot = self.records_ptr().add(head);
            // Seqlock write sequence:
            // 1. Fetch current sequence, make it odd to signal in-progress write
            let cur_seq = std::ptr::read_volatile(std::ptr::addr_of!((*slot).seq));
            let writing_seq = cur_seq.wrapping_add(1) | 1;
            std::ptr::write_volatile(std::ptr::addr_of_mut!((*slot).seq), writing_seq);

            // 2. Barrier before writing record payload
            std::sync::atomic::fence(Ordering::Release);

            std::ptr::write_volatile(std::ptr::addr_of_mut!((*slot).timestamp_ns), record.timestamp_ns);
            std::ptr::write_volatile(std::ptr::addr_of_mut!((*slot).event_type), record.event_type);
            std::ptr::write_volatile(std::ptr::addr_of_mut!((*slot)._reserved), record._reserved);
            std::ptr::write_volatile(std::ptr::addr_of_mut!((*slot).bytes), record.bytes);
            std::ptr::write_volatile(std::ptr::addr_of_mut!((*slot).line), record.line);
            std::ptr::write_volatile(std::ptr::addr_of_mut!((*slot).col), record.col);
            std::ptr::write_volatile(std::ptr::addr_of_mut!((*slot).label), record.label);

            // 3. Barrier before marking sequence even (write committed)
            std::sync::atomic::fence(Ordering::Release);
            let final_seq = writing_seq.wrapping_add(1);
            std::ptr::write_volatile(std::ptr::addr_of_mut!((*slot).seq), final_seq);
        }
    }

    pub fn read_recent_records(&self, max_count: usize) -> Vec<TelemetryRecord> {
        let header = self.header();
        let head = header.ring_head.load(Ordering::SeqCst) as usize;
        let count = head.min(RING_CAPACITY).min(max_count);

        let mut results = Vec::with_capacity(count);
        for i in 0..count {
            let idx = (head + RING_CAPACITY - count + i) % RING_CAPACITY;
            unsafe {
                let slot = self.records_ptr().add(idx);
                let mut read_success = false;
                let mut rec = TelemetryRecord::default();

                // Seqlock read sequence with optimistic retry
                for _ in 0..16 {
                    let seq1 = std::ptr::read_volatile(std::ptr::addr_of!((*slot).seq));
                    if seq1 % 2 != 0 {
                        // Writer is currently updating this slot, spin
                        std::hint::spin_loop();
                        continue;
                    }

                    std::sync::atomic::fence(Ordering::Acquire);
                    rec = std::ptr::read_volatile(slot);
                    std::sync::atomic::fence(Ordering::Acquire);

                    let seq2 = std::ptr::read_volatile(std::ptr::addr_of!((*slot).seq));
                    if seq1 == seq2 && seq1 % 2 == 0 {
                        read_success = true;
                        break;
                    }
                    std::hint::spin_loop();
                }

                if read_success && rec.timestamp_ns > 0 {
                    results.push(rec);
                }
            }
        }
        results
    }
}

impl Drop for SharedTelemetryRegion {
    fn drop(&mut self) {
        if !self.raw_ptr.is_null() && self.raw_ptr != libc::MAP_FAILED as *mut u8 {
            unsafe {
                libc::munmap(self.raw_ptr as *mut libc::c_void, self.size);
            }
        }
    }
}
