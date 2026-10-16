pub mod monitor;
pub mod shm;

pub use monitor::{HealthStatus, MemorySnapshot, MemorySupervisor};
pub use shm::{SharedTelemetryRegion, TelemetryEventType, TelemetryRecord, SHM_MAGIC, SHM_VERSION};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shm_create_write_read_records() {
        let temp_dir = std::env::temp_dir();
        let shm_path = temp_dir.join(format!("trulang_test_shm_{}.dat", std::process::id()));

        // Cleanup any existing
        let _ = std::fs::remove_file(&shm_path);

        let region = SharedTelemetryRegion::create_or_open(&shm_path, std::process::id())
            .expect("Create shm failed");

        region.push_event(TelemetryEventType::AllocRegionA, 128, 10, 5, "user_token");
        region.push_event(
            TelemetryEventType::PersistToRegionB,
            512,
            15,
            2,
            "cache_entry",
        );

        let supervisor = MemorySupervisor::attach(region);
        let snap = supervisor.snapshot();

        assert_eq!(snap.worker_pid, std::process::id());
        assert_eq!(snap.status, HealthStatus::Healthy);

        let _ = std::fs::remove_file(&shm_path);
    }

    #[test]
    fn test_shm_secure_directory_and_permissions() {
        let pid = std::process::id() + 9999;
        let path = shm::default_shm_path(pid);
        let _ = std::fs::remove_file(&path);

        let region =
            SharedTelemetryRegion::create_or_open(&path, pid).expect("Failed to create secure SHM");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&path).expect("SHM metadata failed");
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(
                mode, 0o600,
                "SHM file permissions must be exactly 0600 (owner read/write only)"
            );
        }

        let records = region.read_recent_records(10);
        assert!(records.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_shm_seqlock_atomic_consistency() {
        let temp_dir = std::env::temp_dir();
        let shm_path = temp_dir.join(format!("trulang_seqlock_test_{}.dat", std::process::id()));
        let _ = std::fs::remove_file(&shm_path);

        let region = SharedTelemetryRegion::create_or_open(&shm_path, std::process::id())
            .expect("Create shm failed");

        for i in 0..50 {
            region.push_event(
                TelemetryEventType::AllocRegionA,
                (i * 16) as u32,
                i as u32,
                1,
                &format!("evt_{}", i),
            );
        }

        let records = region.read_recent_records(50);
        assert_eq!(records.len(), 50);

        // Verify that every read record has an even sequence number (consistent, not torn)
        for r in &records {
            assert_eq!(
                r.seq % 2,
                0,
                "Record sequence must be even, indicating committed write"
            );
        }

        let _ = std::fs::remove_file(&shm_path);
    }

    #[test]
    fn test_arena_burst_telemetry_event() {
        let temp_dir = std::env::temp_dir();
        let shm_path = temp_dir.join(format!("trulang_burst_test_{}.dat", std::process::id()));
        let _ = std::fs::remove_file(&shm_path);

        let region = SharedTelemetryRegion::create_or_open(&shm_path, std::process::id())
            .expect("Create shm failed");

        region.push_event(TelemetryEventType::ArenaBurst, 4096, 42, 10, "arena_burst");

        let records = region.read_recent_records(1);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].event_type, TelemetryEventType::ArenaBurst as u8);
        assert_eq!(
            TelemetryEventType::from(records[0].event_type),
            TelemetryEventType::ArenaBurst
        );
        assert_eq!(records[0].bytes, 4096);
        assert_eq!(records[0].get_label(), "arena_burst");

        let _ = std::fs::remove_file(&shm_path);
    }
}
