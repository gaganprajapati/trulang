use crate::error::FfiError;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

type Job = Box<dyn FnOnce() + Send + 'static>;

struct Worker {
    _id: usize,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<Receiver<Job>>>, is_shutdown: Arc<AtomicBool>) -> Self {
        let thread = thread::Builder::new()
            .name(format!("trulang-ffi-worker-{}", id))
            .spawn(move || loop {
                let job = {
                    let rx = match receiver.lock() {
                        Ok(guard) => guard,
                        Err(_) => break, // poisoned
                    };
                    rx.recv()
                };

                match job {
                    Ok(job) => {
                        job();
                    }
                    Err(_) => {
                        // Channel disconnected, clean worker shutdown
                        break;
                    }
                }

                if is_shutdown.load(Ordering::Relaxed) {
                    break;
                }
            })
            .expect("Failed to spawn FFI worker thread");

        Self {
            _id: id,
            thread: Some(thread),
        }
    }
}

pub struct WorkerPool {
    workers: Vec<Worker>,
    sender: Option<Sender<Job>>,
    active_tasks: Arc<AtomicUsize>,
    completed_tasks: Arc<AtomicUsize>,
    is_shutdown: Arc<AtomicBool>,
}

impl WorkerPool {
    pub const DEFAULT_POOL_SIZE: usize = 4;
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new(size: usize) -> Self {
        let (sender, receiver) = channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        let is_shutdown = Arc::new(AtomicBool::new(false));

        let mut workers = Vec::with_capacity(size);
        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver), Arc::clone(&is_shutdown)));
        }

        Self {
            workers,
            sender: Some(sender),
            active_tasks: Arc::new(AtomicUsize::new(0)),
            completed_tasks: Arc::new(AtomicUsize::new(0)),
            is_shutdown,
        }
    }

    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    pub fn active_tasks(&self) -> usize {
        self.active_tasks.load(Ordering::SeqCst)
    }

    pub fn completed_tasks(&self) -> usize {
        self.completed_tasks.load(Ordering::SeqCst)
    }

    /// Submits a blocking foreign task to the background pool and waits for completion.
    /// This prevents blocking foreign operations (like DB queries or network requests)
    /// from starving or holding up the single-threaded TruLang actor mailbox.
    pub fn spawn_blocking<F, R>(&self, task: F) -> Result<R, FfiError>
    where
        F: FnOnce() -> Result<R, FfiError> + Send + 'static,
        R: Send + 'static,
    {
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| FfiError::WorkerPoolError("Worker pool has been shut down".into()))?;

        let (resp_tx, resp_rx) = channel();
        let active = Arc::clone(&self.active_tasks);
        let completed = Arc::clone(&self.completed_tasks);

        active.fetch_add(1, Ordering::SeqCst);

        let job: Job = Box::new(move || {
            let res = task();
            active.fetch_sub(1, Ordering::SeqCst);
            completed.fetch_add(1, Ordering::SeqCst);
            let _ = resp_tx.send(res);
        });

        sender
            .send(job)
            .map_err(|e| FfiError::WorkerPoolError(format!("Failed to dispatch task to worker pool: {}", e)))?;

        match resp_rx.recv_timeout(Self::DEFAULT_TIMEOUT) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                Err(FfiError::WorkerPoolError("Blocking FFI task timed out".into()))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(FfiError::WorkerPoolError("Worker thread died during foreign task execution".into()))
            }
        }
    }
}

impl Default for WorkerPool {
    fn default() -> Self {
        Self::new(Self::DEFAULT_POOL_SIZE)
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.is_shutdown.store(true, Ordering::SeqCst);
        // Dropping the sender causes all worker receivers to disconnect cleanly
        self.sender.take();

        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
