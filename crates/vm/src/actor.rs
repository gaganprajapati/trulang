use crate::memory::RegionBAllocator;
use crate::vm::{VirtualMachine, VmError};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use vlang_compiler::bytecode::{Chunk, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum ActorError {
    MailboxFull { capacity: usize, current: usize },
    VmFailure(VmError),
}

impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorError::MailboxFull { capacity, current } => {
                write!(
                    f,
                    "Service actor mailbox full: {} / {} queued. Request rejected with backpressure (HTTP 429/503)",
                    current, capacity
                )
            }
            ActorError::VmFailure(err) => write!(f, "VM failure: {}", err),
        }
    }
}

impl std::error::Error for ActorError {}

impl From<VmError> for ActorError {
    fn from(err: VmError) -> Self {
        ActorError::VmFailure(err)
    }
}

pub struct ServiceActor {
    pub name: String,
    pub request_budget_bytes: usize,
    pub max_request_budget_bytes: Option<usize>,
    pub persistent_budget_bytes: usize,
    pub mailbox_capacity: usize,
    pub pending_requests: AtomicUsize,
    pub instruction_limit: Option<u64>,
    pub persistent_memory: Arc<RegionBAllocator>,
    pub service_state: HashMap<String, Value>,
    pub chunk: Chunk,
}

impl ServiceActor {
    pub const DEFAULT_MAILBOX_CAPACITY: usize = 10_000;

    pub fn new(
        name: impl Into<String>,
        request_budget: usize,
        persistent_budget: usize,
        chunk: Chunk,
    ) -> Self {
        Self {
            name: name.into(),
            request_budget_bytes: request_budget,
            max_request_budget_bytes: None,
            persistent_budget_bytes: persistent_budget,
            mailbox_capacity: Self::DEFAULT_MAILBOX_CAPACITY,
            pending_requests: AtomicUsize::new(0),
            instruction_limit: None,
            persistent_memory: Arc::new(RegionBAllocator::new(persistent_budget)),
            service_state: HashMap::new(),
            chunk,
        }
    }

    pub fn with_max_request_budget(mut self, max_budget: usize) -> Self {
        self.max_request_budget_bytes = Some(max_budget);
        self
    }

    pub fn with_mailbox_capacity(mut self, capacity: usize) -> Self {
        self.mailbox_capacity = capacity;
        self
    }

    pub fn with_instruction_limit(mut self, limit: u64) -> Self {
        self.instruction_limit = Some(limit);
        self
    }

    pub fn handle_request(&mut self, request: Value) -> Result<Value, ActorError> {
        let current_load = self.pending_requests.load(Ordering::Relaxed);
        if current_load >= self.mailbox_capacity {
            return Err(ActorError::MailboxFull {
                capacity: self.mailbox_capacity,
                current: current_load,
            });
        }

        self.pending_requests.fetch_add(1, Ordering::Relaxed);
        let res = self.execute_request_inner(request);
        self.pending_requests.fetch_sub(1, Ordering::Relaxed);
        res
    }

    fn execute_request_inner(&mut self, request: Value) -> Result<Value, ActorError> {
        let mut vm = VirtualMachine::with_persistent_allocator(
            self.request_budget_bytes,
            Arc::clone(&self.persistent_memory),
        );
        vm.region_a.max_budget_bytes = self.max_request_budget_bytes;

        if let Some(limit) = self.instruction_limit {
            vm = vm.with_instruction_limit(limit);
        }

        // Region A is dedicated to this request
        // Load request object as parameter slot 0
        vm.stack.push(request);

        // Execute service handler chunk
        let response = vm.execute(&self.chunk)?;

        // PRD Requirement: Reclaim temporary Region A execution memory immediately
        vm.region_a.reset();

        Ok(response)
    }

    /// Spawns a dedicated actor worker thread with a bounded synchronized mailbox queue
    pub fn spawn_mailbox(mut self) -> (ActorMailbox, std::thread::JoinHandle<()>) {
        let capacity = self.mailbox_capacity;
        let (tx, rx) = std::sync::mpsc::sync_channel::<(
            Value,
            std::sync::mpsc::Sender<Result<Value, ActorError>>,
        )>(capacity);

        let handle = std::thread::spawn(move || {
            while let Ok((request, reply_tx)) = rx.recv() {
                let response = self.handle_request(request);
                let _ = reply_tx.send(response);
            }
        });

        (ActorMailbox { tx, capacity }, handle)
    }
}

#[derive(Clone)]
pub struct ActorMailbox {
    tx: std::sync::mpsc::SyncSender<(
        Value,
        std::sync::mpsc::Sender<Result<Value, ActorError>>,
    )>,
    pub capacity: usize,
}

impl ActorMailbox {
    pub fn try_send(
        &self,
        request: Value,
    ) -> Result<std::sync::mpsc::Receiver<Result<Value, ActorError>>, ActorError> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        match self.tx.try_send((request, reply_tx)) {
            Ok(()) => Ok(reply_rx),
            Err(std::sync::mpsc::TrySendError::Full(_)) => Err(ActorError::MailboxFull {
                capacity: self.capacity,
                current: self.capacity,
            }),
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => Err(ActorError::VmFailure(
                VmError::UnhandledException(Value::String(Arc::new("Actor stopped".to_string()))),
            )),
        }
    }
}
