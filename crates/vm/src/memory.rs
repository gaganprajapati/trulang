use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, PartialEq)]
pub enum MemoryRegion {
    RegionA, // Execution Memory (per-request bump arena)
    RegionB, // Persistent Memory (ARC long-lived)
}

#[derive(Debug)]
pub struct RegionAAllocator {
    pub budget_bytes: usize,
    pub used_bytes: usize,
    pub allocation_count: usize,
}

impl RegionAAllocator {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            used_bytes: 0,
            allocation_count: 0,
        }
    }

    pub fn allocate(&mut self, size: usize) -> Result<(), MemoryError> {
        if self.budget_bytes > 0 && self.used_bytes + size > self.budget_bytes {
            return Err(MemoryError::RequestMemoryExceeded {
                budget: self.budget_bytes,
                attempted: self.used_bytes + size,
            });
        }
        self.used_bytes += size;
        self.allocation_count += 1;
        Ok(())
    }

    pub fn reset(&mut self) {
        self.used_bytes = 0;
        self.allocation_count = 0;
    }
}

#[derive(Debug, Default)]
pub struct RegionBAllocator {
    pub budget_bytes: usize,
    pub live_bytes: AtomicUsize,
    pub total_allocated_bytes: AtomicUsize,
    pub live_objects: AtomicUsize,
}

impl RegionBAllocator {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            live_bytes: AtomicUsize::new(0),
            total_allocated_bytes: AtomicUsize::new(0),
            live_objects: AtomicUsize::new(0),
        }
    }

    pub fn allocate(&self, size: usize) -> Result<(), MemoryError> {
        let current = self.live_bytes.load(Ordering::Relaxed);
        if self.budget_bytes > 0 && current + size > self.budget_bytes {
            return Err(MemoryError::PersistentMemoryExceeded {
                budget: self.budget_bytes,
                attempted: current + size,
            });
        }
        self.live_bytes.fetch_add(size, Ordering::Relaxed);
        self.total_allocated_bytes.fetch_add(size, Ordering::Relaxed);
        self.live_objects.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn deallocate(&self, size: usize) {
        self.live_bytes.fetch_sub(size, Ordering::Relaxed);
        self.live_objects.fetch_sub(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemoryError {
    RequestMemoryExceeded { budget: usize, attempted: usize },
    PersistentMemoryExceeded { budget: usize, attempted: usize },
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::RequestMemoryExceeded { budget, attempted } => {
                write!(
                    f,
                    "Region A (Execution) memory budget exceeded: attempted {} bytes, limit is {} bytes",
                    attempted, budget
                )
            }
            MemoryError::PersistentMemoryExceeded { budget, attempted } => {
                write!(
                    f,
                    "Region B (Persistent) memory budget exceeded: attempted {} bytes, limit is {} bytes",
                    attempted, budget
                )
            }
        }
    }
}

impl std::error::Error for MemoryError {}
