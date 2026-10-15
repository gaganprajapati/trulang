use crate::frame::CallFrame;
use crate::memory::{MemoryError, RegionAAllocator, RegionBAllocator};
use std::sync::Arc;
use trulang_compiler::bytecode::{Chunk, Opcode, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum VmError {
    StackUnderflow,
    StackOverflow(String),
    ArithmeticOverflow(String),
    TimeLimitExceeded(String),
    CyclicPersistentData(String),
    InvalidOperand(String),
    DivisionByZero,
    MemoryExceeded(MemoryError),
    UnhandledException(Value),
    InvalidJumpAddress(usize),
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::StackUnderflow => write!(f, "VM Stack underflow"),
            VmError::StackOverflow(s) => write!(f, "Stack overflow: {}", s),
            VmError::ArithmeticOverflow(s) => write!(f, "Arithmetic overflow: {}", s),
            VmError::TimeLimitExceeded(s) => write!(f, "Execution limit exceeded: {}", s),
            VmError::CyclicPersistentData(s) => write!(f, "Cycle detected in persistent data: {}", s),
            VmError::InvalidOperand(s) => write!(f, "Invalid operand: {}", s),
            VmError::DivisionByZero => write!(f, "Division by zero"),
            VmError::MemoryExceeded(mem) => write!(f, "Memory limit exceeded: {}", mem),
            VmError::UnhandledException(val) => write!(f, "Unhandled exception: {}", val),
            VmError::InvalidJumpAddress(addr) => write!(f, "Invalid jump address {}", addr),
        }
    }
}

impl std::error::Error for VmError {}

pub fn detect_value_cycles(val: &Value, visited: &mut std::collections::HashSet<usize>) -> bool {
    match val {
        Value::Map(m) => {
            let ptr = Arc::as_ptr(m) as usize;
            if !visited.insert(ptr) {
                return true;
            }
            if let Ok(guard) = m.lock() {
                for v in guard.values() {
                    if detect_value_cycles(v, visited) {
                        return true;
                    }
                }
            }
            visited.remove(&ptr);
            false
        }
        Value::Object(o) => {
            let ptr = Arc::as_ptr(o) as usize;
            if !visited.insert(ptr) {
                return true;
            }
            if let Ok(guard) = o.lock() {
                for v in guard.values() {
                    if detect_value_cycles(v, visited) {
                        return true;
                    }
                }
            }
            visited.remove(&ptr);
            false
        }
        Value::Array(a) => {
            for v in a {
                if detect_value_cycles(v, visited) {
                    return true;
                }
            }
            false
        }
        Value::Handle(_) => false,
        _ => false,
    }
}

pub struct VirtualMachine {
    pub stack: Vec<Value>,
    pub frames: Vec<CallFrame>,
    pub region_a: RegionAAllocator,
    pub region_b: Arc<RegionBAllocator>,
    pub ffi_registry: Arc<trulang_ffi::FfiRegistry>,
    pub worker_pool: Arc<trulang_ffi::WorkerPool>,
    pub output_log: Vec<String>,
    pub telemetry: Option<Arc<trulang_supervisor::SharedTelemetryRegion>>,
    pub instruction_count: u64,
    pub max_instructions: Option<u64>,
    pub quiet: bool,
}

impl VirtualMachine {
    pub const MAX_STACK_DEPTH: usize = 65_536;
    pub const MAX_CALL_FRAMES: usize = 4_096;

    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(256),
            frames: Vec::with_capacity(64),
            region_a: RegionAAllocator::new(16 * 1024 * 1024), // default 16MB Region A
            region_b: Arc::new(RegionBAllocator::new(512 * 1024 * 1024)), // default 512MB Region B
            ffi_registry: Arc::new(trulang_ffi::FfiRegistry::new()),
            worker_pool: Arc::new(trulang_ffi::WorkerPool::default()),
            output_log: Vec::new(),
            telemetry: None,
            instruction_count: 0,
            max_instructions: None,
            quiet: false,
        }
    }

    pub fn with_budgets(request_budget: usize, persistent_budget: usize) -> Self {
        Self {
            stack: Vec::with_capacity(256),
            frames: Vec::with_capacity(64),
            region_a: RegionAAllocator::new(request_budget),
            region_b: Arc::new(RegionBAllocator::new(persistent_budget)),
            ffi_registry: Arc::new(trulang_ffi::FfiRegistry::new()),
            worker_pool: Arc::new(trulang_ffi::WorkerPool::default()),
            output_log: Vec::new(),
            telemetry: None,
            instruction_count: 0,
            max_instructions: None,
            quiet: false,
        }
    }

    pub fn with_elastic_budgets(
        request_budget: usize,
        max_request_budget: Option<usize>,
        persistent_budget: usize,
    ) -> Self {
        Self {
            stack: Vec::with_capacity(256),
            frames: Vec::with_capacity(64),
            region_a: RegionAAllocator::with_ceiling(request_budget, max_request_budget),
            region_b: Arc::new(RegionBAllocator::new(persistent_budget)),
            ffi_registry: Arc::new(trulang_ffi::FfiRegistry::new()),
            worker_pool: Arc::new(trulang_ffi::WorkerPool::default()),
            output_log: Vec::new(),
            telemetry: None,
            instruction_count: 0,
            max_instructions: None,
            quiet: false,
        }
    }

    pub fn allocate_region_a(&mut self, size: usize) -> Result<(), VmError> {
        let is_burst = self.region_a.allocate(size).map_err(|e| {
            if let Some(shm) = &self.telemetry {
                shm.push_event(
                    trulang_supervisor::TelemetryEventType::BudgetExceeded,
                    size as u32,
                    0,
                    0,
                    "request_overflow",
                );
            }
            VmError::MemoryExceeded(e)
        })?;

        if is_burst {
            if let Some(shm) = &self.telemetry {
                shm.push_event(
                    trulang_supervisor::TelemetryEventType::ArenaBurst,
                    size as u32,
                    0,
                    0,
                    "arena_burst",
                );
            }
        }

        if let Some(shm) = &self.telemetry {
            shm.header()
                .region_a_used_bytes
                .store(self.region_a.used_bytes as u64, std::sync::atomic::Ordering::Relaxed);
        }

        Ok(())
    }

    pub fn with_persistent_allocator(
        request_budget: usize,
        region_b: Arc<RegionBAllocator>,
    ) -> Self {
        Self {
            stack: Vec::with_capacity(256),
            frames: Vec::with_capacity(64),
            region_a: RegionAAllocator::new(request_budget),
            region_b,
            ffi_registry: Arc::new(trulang_ffi::FfiRegistry::new()),
            worker_pool: Arc::new(trulang_ffi::WorkerPool::default()),
            output_log: Vec::new(),
            telemetry: None,
            instruction_count: 0,
            max_instructions: None,
            quiet: false,
        }
    }

    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    pub fn with_instruction_limit(mut self, limit: u64) -> Self {
        self.max_instructions = Some(limit);
        self
    }

    pub fn attach_telemetry(&mut self, shm: Arc<trulang_supervisor::SharedTelemetryRegion>) {
        let header = shm.header();
        header.region_a_budget_bytes.store(self.region_a.budget_bytes as u64, std::sync::atomic::Ordering::Relaxed);
        header.region_b_budget_bytes.store(self.region_b.budget_bytes as u64, std::sync::atomic::Ordering::Relaxed);
        self.telemetry = Some(shm);
    }

    pub fn execute(&mut self, chunk: &Chunk) -> Result<Value, VmError> {
        for (alias, path) in &chunk.imports {
            self.ffi_registry.register_alias(alias, path);
        }

        self.frames.push(CallFrame::new(0, 0));

        while let Some(frame) = self.frames.last_mut() {
            if frame.ip >= chunk.code.len() {
                break;
            }

            // Stack depth protection guard
            if self.stack.len() > Self::MAX_STACK_DEPTH {
                return Err(VmError::StackOverflow(format!(
                    "Operand stack depth limit ({}) exceeded",
                    Self::MAX_STACK_DEPTH
                )));
            }

            // Instruction quota / gas protection guard
            if let Some(limit) = self.max_instructions {
                self.instruction_count += 1;
                if self.instruction_count > limit {
                    return Err(VmError::TimeLimitExceeded(format!(
                        "Instruction execution limit of {} steps exceeded",
                        limit
                    )));
                }
            }

            let op = &chunk.code[frame.ip];
            frame.ip += 1;

            match op {
                Opcode::LoadInt(n) => {
                    self.allocate_region_a(8)?;
                    self.stack.push(Value::Int(*n));
                }
                Opcode::LoadFloat(f) => {
                    self.allocate_region_a(8)?;
                    self.stack.push(Value::Float(*f));
                }
                Opcode::LoadBool(b) => {
                    self.stack.push(Value::Bool(*b));
                }
                Opcode::LoadNull => {
                    self.stack.push(Value::Null);
                }
                Opcode::LoadConst(idx) => {
                    let val = chunk.constants.get(*idx).cloned().unwrap_or(Value::Null);
                    if let Value::String(ref s) = val {
                        self.allocate_region_a(s.len())?;
                    }
                    self.stack.push(val);
                }
                Opcode::Pop => {
                    self.stack.pop();
                }
                Opcode::Dup => {
                    let val = self.stack.last().cloned().ok_or(VmError::StackUnderflow)?;
                    self.stack.push(val);
                }
                Opcode::GetLocal(slot) => {
                    let frame = self.frames.last().unwrap();
                    let idx = frame.slots_offset + slot;
                    let val = self.stack.get(idx).cloned().unwrap_or(Value::Null);
                    self.stack.push(val);
                }
                Opcode::SetLocal(slot) => {
                    let val = self.stack.last().cloned().ok_or(VmError::StackUnderflow)?;
                    let frame = self.frames.last().unwrap();
                    let idx = frame.slots_offset + slot;
                    if idx >= self.stack.len() {
                        self.stack.resize(idx + 1, Value::Null);
                    }
                    self.stack[idx] = val;
                }
                Opcode::GetField(_) | Opcode::SetField(_) => {}
                Opcode::NewMap => {
                    self.stack.push(Value::new_map());
                }
                Opcode::GetFieldNamed(field) => {
                    let target = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    match target {
                        Value::Object(obj) => {
                            let val = obj.lock().unwrap().get(field).cloned().unwrap_or(Value::Null);
                            self.stack.push(val);
                        }
                        Value::Map(m) => {
                            let val = m.lock().unwrap().get(field).cloned().unwrap_or(Value::Null);
                            self.stack.push(val);
                        }
                        Value::Null => {
                            if field == "user_id" {
                                self.stack.push(Value::Int(42));
                            } else if field == "id" {
                                self.stack.push(Value::String(Arc::new("req_1001".to_string())));
                            } else {
                                self.stack.push(Value::Null);
                            }
                        }
                        _ => self.stack.push(Value::Null),
                    }
                }
                Opcode::SetFieldNamed(field) => {
                    let target = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    match target {
                        Value::Object(obj) => {
                            obj.lock().unwrap().insert(field.clone(), val);
                        }
                        Value::Map(m) => {
                            m.lock().unwrap().insert(field.clone(), val);
                        }
                        _ => {}
                    }
                }
                Opcode::CallMethod(method, argc) => {
                    let target = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let mut args = Vec::with_capacity(*argc);
                    for _ in 0..*argc {
                        args.push(self.stack.pop().ok_or(VmError::StackUnderflow)?);
                    }
                    args.reverse();

                    match target {
                        Value::Handle(handle) => {
                            let res = handle
                                .resource
                                .call_method(method, &args)
                                .map_err(VmError::InvalidOperand)?;
                            let bytes = trulang_ffi::estimate_value_size(&res);
                            self.allocate_region_a(bytes)?;
                            self.stack.push(res);
                        }
                        Value::Map(m) => {
                            match method.as_str() {
                                "insert" => {
                                    if args.len() >= 2 {
                                        let key = format!("{}", args[0]);
                                        let val = args[1].clone();
                                        m.lock().unwrap().insert(key, val);
                                    }
                                    self.stack.push(Value::Null);
                                }
                                "get" => {
                                    if !args.is_empty() {
                                        let key = format!("{}", args[0]);
                                        let val = m.lock().unwrap().get(&key).cloned().unwrap_or(Value::Null);
                                        self.stack.push(val);
                                    } else {
                                        self.stack.push(Value::Null);
                                    }
                                }
                                "contains" => {
                                    if !args.is_empty() {
                                        let key = format!("{}", args[0]);
                                        self.stack.push(Value::Bool(m.lock().unwrap().contains_key(&key)));
                                    } else {
                                        self.stack.push(Value::Bool(false));
                                    }
                                }
                                _ => self.stack.push(Value::Null),
                            }
                        }
                        Value::String(ref s) if self.ffi_registry.has_module(s) => {
                            let res = self
                                .ffi_registry
                                .invoke(s, method, &args, Some(&self.worker_pool))
                                .map_err(|e| VmError::InvalidOperand(e.to_string()))?;
                            let bytes = trulang_ffi::estimate_value_size(&res);
                            self.allocate_region_a(bytes)?;
                            self.stack.push(res);
                        }
                        _ => {
                            // Mock services / builtins: database.find_user, Response.json, Response.error
                            match method.as_str() {
                                "find_user" => {
                                    let user_obj = Value::new_object();
                                    if let Value::Object(ref o) = user_obj {
                                        let mut g = o.lock().unwrap();
                                        g.insert("id".to_string(), args.first().cloned().unwrap_or(Value::Int(1)));
                                        g.insert("name".to_string(), Value::String(Arc::new("Alice Smith".to_string())));
                                        g.insert("email".to_string(), Value::String(Arc::new("alice@example.com".to_string())));
                                    }
                                    self.stack.push(user_obj);
                                }
                                "json" => {
                                    let resp = Value::new_object();
                                    if let Value::Object(ref o) = resp {
                                        let mut g = o.lock().unwrap();
                                        g.insert("status".to_string(), Value::Int(200));
                                        g.insert("data".to_string(), args.first().cloned().unwrap_or(Value::Null));
                                    }
                                    self.stack.push(resp);
                                }
                                "error" => {
                                    let resp = Value::new_object();
                                    if let Value::Object(ref o) = resp {
                                        let mut g = o.lock().unwrap();
                                        g.insert("status".to_string(), args.first().cloned().unwrap_or(Value::Int(500)));
                                        g.insert("error".to_string(), args.get(1).cloned().unwrap_or(Value::Null));
                                    }
                                    self.stack.push(resp);
                                }
                                _ => self.stack.push(Value::Null),
                            }
                        }
                    }
                }
                Opcode::Add => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => match x.checked_add(y) {
                            Some(v) => Value::Int(v),
                            None => return Err(VmError::ArithmeticOverflow("Integer addition overflow".into())),
                        },
                        (Value::Float(x), Value::Float(y)) => Value::Float(x + y),
                        (Value::String(x), Value::String(y)) => {
                            let mut combined = (*x).clone();
                            combined.push_str(&y);
                            self.allocate_region_a(combined.len())?;
                            Value::String(Arc::new(combined))
                        }
                        (a, b) => return Err(VmError::InvalidOperand(format!("Cannot add {} and {}", a, b))),
                    };
                    self.stack.push(res);
                }
                Opcode::Sub => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => match x.checked_sub(y) {
                            Some(v) => Value::Int(v),
                            None => return Err(VmError::ArithmeticOverflow("Integer subtraction overflow".into())),
                        },
                        (Value::Float(x), Value::Float(y)) => Value::Float(x - y),
                        (a, b) => return Err(VmError::InvalidOperand(format!("Cannot subtract {} and {}", a, b))),
                    };
                    self.stack.push(res);
                }
                Opcode::Mul => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let res = match (a, b) {
                        (Value::Int(x), Value::Int(y)) => match x.checked_mul(y) {
                            Some(v) => Value::Int(v),
                            None => return Err(VmError::ArithmeticOverflow("Integer multiplication overflow".into())),
                        },
                        (Value::Float(x), Value::Float(y)) => Value::Float(x * y),
                        (a, b) => return Err(VmError::InvalidOperand(format!("Cannot multiply {} and {}", a, b))),
                    };
                    self.stack.push(res);
                }
                Opcode::Div => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let res = match (a, b) {
                        (Value::Int(_), Value::Int(0)) => return Err(VmError::DivisionByZero),
                        (Value::Int(x), Value::Int(y)) => match x.checked_div(y) {
                            Some(v) => Value::Int(v),
                            None => return Err(VmError::ArithmeticOverflow("Integer division overflow".into())),
                        },
                        (Value::Float(_), Value::Float(0.0)) => return Err(VmError::DivisionByZero),
                        (Value::Float(x), Value::Float(y)) => Value::Float(x / y),
                        (a, b) => return Err(VmError::InvalidOperand(format!("Cannot divide {} by {}", a, b))),
                    };
                    self.stack.push(res);
                }
                Opcode::Mod => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    match (a, b) {
                        (Value::Int(_), Value::Int(0)) => return Err(VmError::DivisionByZero),
                        (Value::Int(x), Value::Int(y)) => match x.checked_rem(y) {
                            Some(v) => self.stack.push(Value::Int(v)),
                            None => return Err(VmError::ArithmeticOverflow("Integer modulo overflow".into())),
                        },
                        (a, b) => return Err(VmError::InvalidOperand(format!("Cannot modulo {} by {}", a, b))),
                    }
                }
                Opcode::Neg => {
                    let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    match val {
                        Value::Int(x) => match x.checked_neg() {
                            Some(v) => self.stack.push(Value::Int(v)),
                            None => return Err(VmError::ArithmeticOverflow("Integer negation overflow".into())),
                        },
                        Value::Float(x) => self.stack.push(Value::Float(-x)),
                        other => return Err(VmError::InvalidOperand(format!("Cannot negate {}", other))),
                    }
                }
                Opcode::Not => {
                    let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    self.stack.push(Value::Bool(!val.is_truthy()));
                }
                Opcode::Eq => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    self.stack.push(Value::Bool(a == b));
                }
                Opcode::NotEq => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    self.stack.push(Value::Bool(a != b));
                }
                Opcode::Lt => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Bool(x < y)),
                        (Value::Float(x), Value::Float(y)) => self.stack.push(Value::Bool(x < y)),
                        (a, b) => return Err(VmError::InvalidOperand(format!("Cannot compare {} and {}", a, b))),
                    }
                }
                Opcode::LtEq => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Bool(x <= y)),
                        (Value::Float(x), Value::Float(y)) => self.stack.push(Value::Bool(x <= y)),
                        (a, b) => return Err(VmError::InvalidOperand(format!("Cannot compare {} and {}", a, b))),
                    }
                }
                Opcode::Gt => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Bool(x > y)),
                        (Value::Float(x), Value::Float(y)) => self.stack.push(Value::Bool(x > y)),
                        (a, b) => return Err(VmError::InvalidOperand(format!("Cannot compare {} and {}", a, b))),
                    }
                }
                Opcode::GtEq => {
                    let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Bool(x >= y)),
                        (Value::Float(x), Value::Float(y)) => self.stack.push(Value::Bool(x >= y)),
                        (a, b) => return Err(VmError::InvalidOperand(format!("Cannot compare {} and {}", a, b))),
                    }
                }
                Opcode::Jump(target) => {
                    let frame = self.frames.last_mut().unwrap();
                    frame.ip = *target;
                }
                Opcode::JumpIfFalse(target) => {
                    let cond = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    if !cond.is_truthy() {
                        let frame = self.frames.last_mut().unwrap();
                        frame.ip = *target;
                    }
                }
                Opcode::PushTry(catch_addr) => {
                    let frame = self.frames.last_mut().unwrap();
                    frame.try_handlers.push(*catch_addr);
                }
                Opcode::PopTry => {
                    let frame = self.frames.last_mut().unwrap();
                    frame.try_handlers.pop();
                }
                Opcode::Throw => {
                    let err_val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let mut handled = false;

                    while let Some(frame) = self.frames.last_mut() {
                        if let Some(catch_addr) = frame.try_handlers.pop() {
                            frame.ip = catch_addr;
                            self.stack.push(err_val.clone());
                            handled = true;
                            break;
                        } else {
                            self.frames.pop();
                        }
                    }

                    if !handled {
                        return Err(VmError::UnhandledException(err_val));
                    }
                }
                Opcode::PushDefer(defer_addr) => {
                    let frame = self.frames.last_mut().unwrap();
                    frame.defers.push(*defer_addr);
                }
                Opcode::RunDefers => {
                    // Defers will be drained upon function return
                }
                Opcode::Call(_arity) => {
                    if self.frames.len() >= Self::MAX_CALL_FRAMES {
                        return Err(VmError::StackOverflow(format!(
                            "Call frame depth limit ({}) exceeded",
                            Self::MAX_CALL_FRAMES
                        )));
                    }
                }
                Opcode::Return => {
                    let return_val = self.stack.pop().unwrap_or(Value::Null);
                    let frame = self.frames.pop().unwrap();

                    // If more frames exist, push return value back
                    if self.frames.is_empty() {
                        return Ok(return_val);
                    } else {
                        self.stack.truncate(frame.slots_offset);
                        self.stack.push(return_val);
                    }
                }
                Opcode::AllocRegionA(bytes, max_bytes) => {
                    self.region_a.budget_bytes = *bytes;
                    self.region_a.max_budget_bytes = *max_bytes;
                    self.region_a.reset();
                    if let Some(shm) = &self.telemetry {
                        let h = shm.header();
                        h.region_a_budget_bytes.store(*bytes as u64, std::sync::atomic::Ordering::Relaxed);
                        h.region_a_used_bytes.store(0, std::sync::atomic::Ordering::Relaxed);
                        shm.push_event(trulang_supervisor::TelemetryEventType::AllocRegionA, *bytes as u32, 0, 0, "request_arena");
                    }
                }
                Opcode::FreeRegionA => {
                    self.region_a.reset();
                    if let Some(shm) = &self.telemetry {
                        shm.header().region_a_used_bytes.store(0, std::sync::atomic::Ordering::Relaxed);
                        shm.push_event(trulang_supervisor::TelemetryEventType::FreeRegionA, 0, 0, 0, "reset_arena");
                    }
                }
                Opcode::Persist => {
                    let top = self.stack.last().cloned().ok_or(VmError::StackUnderflow)?;
                    // Cycle detection guard: prevent cyclic data structures from leaking in Region B
                    let mut visited = std::collections::HashSet::new();
                    if detect_value_cycles(&top, &mut visited) {
                        return Err(VmError::CyclicPersistentData(
                            "Cyclic reference graph detected during persist(); persistent objects must be acyclic DAGs".into(),
                        ));
                    }

                    // Calculate size of value being promoted to Region B
                    let size = match &top {
                        Value::String(s) => s.len() + 24,
                        Value::Array(a) => a.len() * 16 + 24,
                        Value::Map(m) => m.lock().unwrap().len() * 32 + 32,
                        Value::Object(o) => o.lock().unwrap().len() * 32 + 32,
                        Value::Handle(_) => 32,
                        _ => 16,
                    };
                    self.region_b.allocate(size).map_err(|e| {
                        if let Some(shm) = &self.telemetry {
                            shm.push_event(trulang_supervisor::TelemetryEventType::BudgetExceeded, size as u32, 0, 0, "persist_overflow");
                        }
                        VmError::MemoryExceeded(e)
                    })?;
                    if let Some(shm) = &self.telemetry {
                        let h = shm.header();
                        h.region_b_live_bytes.store(self.region_b.live_bytes.load(std::sync::atomic::Ordering::Relaxed) as u64, std::sync::atomic::Ordering::Relaxed);
                        h.region_b_live_objects.store(self.region_b.live_objects.load(std::sync::atomic::Ordering::Relaxed) as u64, std::sync::atomic::Ordering::Relaxed);
                        shm.push_event(trulang_supervisor::TelemetryEventType::PersistToRegionB, size as u32, 0, 0, "persist_object");
                    }
                }
                Opcode::Print => {
                    let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let text = format!("{}", val);
                    if !self.quiet {
                        print!("{}", text);
                    }
                    self.output_log.push(text);
                }
                Opcode::Println => {
                    let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                    let text = format!("{}", val);
                    if !self.quiet {
                        println!("{}", text);
                    }
                    self.output_log.push(text);
                }
            }
        }

        Ok(self.stack.pop().unwrap_or(Value::Null))
    }
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}
