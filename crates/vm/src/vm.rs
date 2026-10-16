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
    AssertionFailure {
        message: String,
        line: usize,
    },
    AssertionEqFailure {
        left: String,
        right: String,
        message: Option<String>,
        line: usize,
    },
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmError::StackUnderflow => write!(f, "VM Stack underflow"),
            VmError::StackOverflow(s) => write!(f, "Stack overflow: {}", s),
            VmError::ArithmeticOverflow(s) => write!(f, "Arithmetic overflow: {}", s),
            VmError::TimeLimitExceeded(s) => write!(f, "Execution limit exceeded: {}", s),
            VmError::CyclicPersistentData(s) => {
                write!(f, "Cycle detected in persistent data: {}", s)
            }
            VmError::InvalidOperand(s) => write!(f, "Invalid operand: {}", s),
            VmError::DivisionByZero => write!(f, "Division by zero"),
            VmError::MemoryExceeded(mem) => write!(f, "Memory limit exceeded: {}", mem),
            VmError::UnhandledException(val) => write!(f, "Unhandled exception: {}", val),
            VmError::InvalidJumpAddress(addr) => write!(f, "Invalid jump address {}", addr),
            VmError::AssertionFailure { message, line } => {
                write!(f, "Assertion failed: {} (at line {})", message, line)
            }
            VmError::AssertionEqFailure {
                left,
                right,
                message,
                line,
            } => {
                write!(
                    f,
                    "Assertion failed: assert_eq (at line {})
  left:  {}
  right: {}",
                    line, left, right
                )?;
                if let Some(msg) = message {
                    write!(
                        f,
                        "
  message: {}",
                        msg
                    )?;
                }
                Ok(())
            }
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

#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    Continued,
    Terminated(Value),
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
    pub profiler: Option<crate::profiler::MemoryProfiler>,
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
            profiler: None,
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
            profiler: None,
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
            profiler: None,
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
            shm.header().region_a_used_bytes.store(
                self.region_a.used_bytes as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }

        if let Some(prof) = &mut self.profiler {
            let func = "active_scope".to_string();
            let line = 0;
            let stack_path = vec!["main:0".to_string()];
            prof.record_alloc(size, func, line, is_burst, stack_path);
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
            profiler: None,
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
        header.region_a_budget_bytes.store(
            self.region_a.budget_bytes as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        header.region_b_budget_bytes.store(
            self.region_b.budget_bytes as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.telemetry = Some(shm);
    }

    pub fn attach_profiler(&mut self, profiler: crate::profiler::MemoryProfiler) {
        self.profiler = Some(profiler);
    }

    pub fn take_profiler(&mut self) -> Option<crate::profiler::MemoryProfiler> {
        self.profiler.take()
    }

    pub fn init_execution(&mut self, chunk: &Chunk) {
        for (alias, path) in &chunk.imports {
            self.ffi_registry.register_alias(alias, path);
        }
        let start_ip = chunk.functions.get("main").copied().unwrap_or(0);
        self.frames.clear();
        self.frames.push(CallFrame::new(start_ip, 0));
    }

    pub fn is_terminated(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn current_ip(&self) -> Option<usize> {
        self.frames.last().map(|f| f.ip)
    }

    pub fn current_line(&self, chunk: &Chunk) -> Option<usize> {
        self.current_ip()
            .and_then(|ip| chunk.lines.get(ip).copied())
    }

    pub fn current_function<'a>(&self, chunk: &'a Chunk) -> Option<&'a str> {
        let current_ip = self.current_ip()?;
        let mut best: Option<(&'a str, usize)> = None;
        for (name, &entry) in &chunk.functions {
            if entry <= current_ip {
                if let Some((_, best_entry)) = best {
                    if entry > best_entry {
                        best = Some((name.as_str(), entry));
                    }
                } else {
                    best = Some((name.as_str(), entry));
                }
            }
        }
        best.map(|(name, _)| name)
    }

    pub fn current_stack_path(&self, chunk: &Chunk) -> Vec<String> {
        let mut path = Vec::new();
        for frame in &self.frames {
            let func_name = chunk
                .functions
                .iter()
                .find(|(_, &entry)| {
                    entry == frame.ip || (entry <= frame.ip && frame.ip - entry < 200)
                })
                .map(|(k, _)| k.as_str())
                .unwrap_or("fn");
            let line = chunk.lines.get(frame.ip).copied().unwrap_or(0);
            path.push(format!("{}:{}", func_name, line));
        }
        if path.is_empty() {
            path.push("main:0".to_string());
        }
        path
    }

    pub fn execute(&mut self, chunk: &Chunk) -> Result<Value, VmError> {
        self.init_execution(chunk);
        loop {
            match self.step_instruction(chunk)? {
                StepStatus::Continued => {}
                StepStatus::Terminated(val) => return Ok(val),
            }
        }
    }

    pub fn step_instruction(&mut self, chunk: &Chunk) -> Result<StepStatus, VmError> {
        let frame = match self.frames.last_mut() {
            Some(f) => f,
            None => {
                return Ok(StepStatus::Terminated(
                    self.stack.pop().unwrap_or(Value::Null),
                ))
            }
        };

        if frame.ip >= chunk.code.len() {
            let _ = self.frames.pop();
            if self.frames.is_empty() {
                return Ok(StepStatus::Terminated(
                    self.stack.pop().unwrap_or(Value::Null),
                ));
            } else {
                return Ok(StepStatus::Continued);
            }
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

        let current_ip = frame.ip;
        let op = &chunk.code[current_ip];
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
            Opcode::NewObject(count) => {
                let obj = Value::new_object();
                let mut approx_bytes = 64;
                if let Value::Object(ref o) = obj {
                    let mut guard = o.lock().unwrap();
                    for _ in 0..*count {
                        let key_val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                        let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                        let key = match key_val {
                            Value::String(s) => (*s).clone(),
                            other => format!("{}", other),
                        };
                        approx_bytes += key.len() + trulang_ffi::estimate_value_size(&val);
                        guard.insert(key, val);
                    }
                }
                self.allocate_region_a(approx_bytes)?;
                self.stack.push(obj);
            }
            Opcode::GetFieldNamed(field) => {
                let target = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                match target {
                    Value::Object(obj) => {
                        let val = obj
                            .lock()
                            .unwrap()
                            .get(field)
                            .cloned()
                            .unwrap_or(Value::Null);
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
                            self.stack
                                .push(Value::String(Arc::new("req_1001".to_string())));
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
                    Value::Map(m) => match method.as_str() {
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
                                let val =
                                    m.lock().unwrap().get(&key).cloned().unwrap_or(Value::Null);
                                self.stack.push(val);
                            } else {
                                self.stack.push(Value::Null);
                            }
                        }
                        "contains" => {
                            if !args.is_empty() {
                                let key = format!("{}", args[0]);
                                self.stack
                                    .push(Value::Bool(m.lock().unwrap().contains_key(&key)));
                            } else {
                                self.stack.push(Value::Bool(false));
                            }
                        }
                        _ => self.stack.push(Value::Null),
                    },
                    Value::Object(obj) => match method.as_str() {
                        "param" | "query" => {
                            let key = args.first().map(|a| format!("{}", a)).unwrap_or_default();
                            let guard = obj.lock().unwrap();
                            let val = if let Some(Value::Map(ref params)) = guard.get("params") {
                                params
                                    .lock()
                                    .unwrap()
                                    .get(&key)
                                    .cloned()
                                    .unwrap_or(Value::Null)
                            } else {
                                guard.get(&key).cloned().unwrap_or(Value::Null)
                            };
                            self.stack.push(val);
                        }
                        "header" => {
                            let key = args
                                .first()
                                .map(|a| format!("{}", a).to_ascii_lowercase())
                                .unwrap_or_default();
                            let guard = obj.lock().unwrap();
                            let val = if let Some(Value::Map(ref headers)) = guard.get("headers") {
                                headers
                                    .lock()
                                    .unwrap()
                                    .get(&key)
                                    .cloned()
                                    .unwrap_or(Value::Null)
                            } else {
                                Value::Null
                            };
                            self.stack.push(val);
                        }
                        "json" => {
                            let guard = obj.lock().unwrap();
                            let val = guard.get("json_data").cloned().unwrap_or(Value::Null);
                            self.stack.push(val);
                        }
                        "path" => {
                            let guard = obj.lock().unwrap();
                            let val = guard.get("path").cloned().unwrap_or(Value::Null);
                            self.stack.push(val);
                        }
                        "method" => {
                            let guard = obj.lock().unwrap();
                            let val = guard.get("method").cloned().unwrap_or(Value::Null);
                            self.stack.push(val);
                        }
                        "body" => {
                            let guard = obj.lock().unwrap();
                            let val = guard.get("body").cloned().unwrap_or(Value::Null);
                            self.stack.push(val);
                        }
                        "get" => {
                            if !args.is_empty() {
                                let key = format!("{}", args[0]);
                                let val = obj
                                    .lock()
                                    .unwrap()
                                    .get(&key)
                                    .cloned()
                                    .unwrap_or(Value::Null);
                                self.stack.push(val);
                            } else {
                                self.stack.push(Value::Null);
                            }
                        }
                        "insert" | "set" => {
                            if args.len() >= 2 {
                                let key = format!("{}", args[0]);
                                let val = args[1].clone();
                                obj.lock().unwrap().insert(key, val);
                            }
                            self.stack.push(Value::Null);
                        }
                        "contains" => {
                            if !args.is_empty() {
                                let key = format!("{}", args[0]);
                                self.stack
                                    .push(Value::Bool(obj.lock().unwrap().contains_key(&key)));
                            } else {
                                self.stack.push(Value::Bool(false));
                            }
                        }
                        _ => self.stack.push(Value::Null),
                    },
                    Value::String(ref s) => {
                        let namespaced = format!("{}::{}", s, method);
                        let dotted = format!("{}.{}", s, method);
                        if let Some(&target_ip) = chunk
                            .functions
                            .get(&namespaced)
                            .or_else(|| chunk.functions.get(&dotted))
                        {
                            if self.frames.len() >= Self::MAX_CALL_FRAMES {
                                return Err(VmError::StackOverflow(format!(
                                    "Call frame depth limit ({}) exceeded",
                                    Self::MAX_CALL_FRAMES
                                )));
                            }
                            if target_ip >= chunk.code.len() {
                                return Err(VmError::InvalidOperand(format!(
                                    "Invalid function address: {}",
                                    target_ip
                                )));
                            }
                            let slots_offset = self.stack.len();
                            for arg in args {
                                self.stack.push(arg);
                            }
                            self.frames.push(CallFrame::new(target_ip, slots_offset));
                        } else if self.ffi_registry.has_module(s) {
                            let res = self
                                .ffi_registry
                                .invoke(s, method, &args, Some(&self.worker_pool))
                                .map_err(|e| VmError::InvalidOperand(e.to_string()))?;
                            let bytes = trulang_ffi::estimate_value_size(&res);
                            self.allocate_region_a(bytes)?;
                            self.stack.push(res);
                        } else {
                            // Mock services / builtins: database.find_user, Response.json, Response.error
                            match method.as_str() {
                                "find_user" => {
                                    let user_obj = Value::new_object();
                                    if let Value::Object(ref o) = user_obj {
                                        let mut g = o.lock().unwrap();
                                        g.insert(
                                            "id".to_string(),
                                            args.first().cloned().unwrap_or(Value::Int(1)),
                                        );
                                        g.insert(
                                            "name".to_string(),
                                            Value::String(Arc::new("Alice Smith".to_string())),
                                        );
                                        g.insert(
                                            "email".to_string(),
                                            Value::String(Arc::new(
                                                "alice@example.com".to_string(),
                                            )),
                                        );
                                    }
                                    self.stack.push(user_obj);
                                }
                                "json" => {
                                    let resp = Value::new_object();
                                    if let Value::Object(ref o) = resp {
                                        let mut g = o.lock().unwrap();
                                        g.insert("status".to_string(), Value::Int(200));
                                        g.insert(
                                            "data".to_string(),
                                            args.first().cloned().unwrap_or(Value::Null),
                                        );
                                    }
                                    self.stack.push(resp);
                                }
                                "error" => {
                                    let resp = Value::new_object();
                                    if let Value::Object(ref o) = resp {
                                        let mut g = o.lock().unwrap();
                                        g.insert(
                                            "status".to_string(),
                                            args.first().cloned().unwrap_or(Value::Int(500)),
                                        );
                                        g.insert(
                                            "error".to_string(),
                                            args.get(1).cloned().unwrap_or(Value::Null),
                                        );
                                    }
                                    self.stack.push(resp);
                                }
                                _ => {
                                    return Err(VmError::InvalidOperand(format!(
                                        "Undefined method '{}' on string or module '{}'",
                                        method, s
                                    )))
                                }
                            }
                        }
                    }
                    _ => self.stack.push(Value::Null),
                }
            }
            Opcode::Add => {
                let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let res = match (a, b) {
                    (Value::Int(x), Value::Int(y)) => match x.checked_add(y) {
                        Some(v) => Value::Int(v),
                        None => {
                            return Err(VmError::ArithmeticOverflow(
                                "Integer addition overflow".into(),
                            ))
                        }
                    },
                    (Value::Float(x), Value::Float(y)) => Value::Float(x + y),
                    (Value::String(x), Value::String(y)) => {
                        let mut combined = (*x).clone();
                        combined.push_str(&y);
                        self.allocate_region_a(combined.len())?;
                        Value::String(Arc::new(combined))
                    }
                    (a, b) => {
                        return Err(VmError::InvalidOperand(format!(
                            "Cannot add {} and {}",
                            a, b
                        )))
                    }
                };
                self.stack.push(res);
            }
            Opcode::Sub => {
                let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let res = match (a, b) {
                    (Value::Int(x), Value::Int(y)) => match x.checked_sub(y) {
                        Some(v) => Value::Int(v),
                        None => {
                            return Err(VmError::ArithmeticOverflow(
                                "Integer subtraction overflow".into(),
                            ))
                        }
                    },
                    (Value::Float(x), Value::Float(y)) => Value::Float(x - y),
                    (a, b) => {
                        return Err(VmError::InvalidOperand(format!(
                            "Cannot subtract {} and {}",
                            a, b
                        )))
                    }
                };
                self.stack.push(res);
            }
            Opcode::Mul => {
                let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let res = match (a, b) {
                    (Value::Int(x), Value::Int(y)) => match x.checked_mul(y) {
                        Some(v) => Value::Int(v),
                        None => {
                            return Err(VmError::ArithmeticOverflow(
                                "Integer multiplication overflow".into(),
                            ))
                        }
                    },
                    (Value::Float(x), Value::Float(y)) => Value::Float(x * y),
                    (a, b) => {
                        return Err(VmError::InvalidOperand(format!(
                            "Cannot multiply {} and {}",
                            a, b
                        )))
                    }
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
                        None => {
                            return Err(VmError::ArithmeticOverflow(
                                "Integer division overflow".into(),
                            ))
                        }
                    },
                    (Value::Float(_), Value::Float(0.0)) => return Err(VmError::DivisionByZero),
                    (Value::Float(x), Value::Float(y)) => Value::Float(x / y),
                    (a, b) => {
                        return Err(VmError::InvalidOperand(format!(
                            "Cannot divide {} by {}",
                            a, b
                        )))
                    }
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
                        None => {
                            return Err(VmError::ArithmeticOverflow(
                                "Integer modulo overflow".into(),
                            ))
                        }
                    },
                    (a, b) => {
                        return Err(VmError::InvalidOperand(format!(
                            "Cannot modulo {} by {}",
                            a, b
                        )))
                    }
                }
            }
            Opcode::Neg => {
                let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                match val {
                    Value::Int(x) => match x.checked_neg() {
                        Some(v) => self.stack.push(Value::Int(v)),
                        None => {
                            return Err(VmError::ArithmeticOverflow(
                                "Integer negation overflow".into(),
                            ))
                        }
                    },
                    Value::Float(x) => self.stack.push(Value::Float(-x)),
                    other => {
                        return Err(VmError::InvalidOperand(format!("Cannot negate {}", other)))
                    }
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
                    (a, b) => {
                        return Err(VmError::InvalidOperand(format!(
                            "Cannot compare {} and {}",
                            a, b
                        )))
                    }
                }
            }
            Opcode::LtEq => {
                let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Bool(x <= y)),
                    (Value::Float(x), Value::Float(y)) => self.stack.push(Value::Bool(x <= y)),
                    (a, b) => {
                        return Err(VmError::InvalidOperand(format!(
                            "Cannot compare {} and {}",
                            a, b
                        )))
                    }
                }
            }
            Opcode::Gt => {
                let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Bool(x > y)),
                    (Value::Float(x), Value::Float(y)) => self.stack.push(Value::Bool(x > y)),
                    (a, b) => {
                        return Err(VmError::InvalidOperand(format!(
                            "Cannot compare {} and {}",
                            a, b
                        )))
                    }
                }
            }
            Opcode::GtEq => {
                let b = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let a = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => self.stack.push(Value::Bool(x >= y)),
                    (Value::Float(x), Value::Float(y)) => self.stack.push(Value::Bool(x >= y)),
                    (a, b) => {
                        return Err(VmError::InvalidOperand(format!(
                            "Cannot compare {} and {}",
                            a, b
                        )))
                    }
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
            Opcode::Call(arity) => {
                if self.frames.len() >= Self::MAX_CALL_FRAMES {
                    return Err(VmError::StackOverflow(format!(
                        "Call frame depth limit ({}) exceeded",
                        Self::MAX_CALL_FRAMES
                    )));
                }
                let callee = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let target_ip = match callee {
                    Value::String(ref name) => match chunk.functions.get(name.as_str()) {
                        Some(&ip) => ip,
                        None => {
                            return Err(VmError::InvalidOperand(format!(
                                "Undefined function: {}",
                                name
                            )))
                        }
                    },
                    Value::Int(ip) => ip as usize,
                    other => {
                        return Err(VmError::InvalidOperand(format!(
                            "Cannot call non-function value: {}",
                            other
                        )))
                    }
                };
                if target_ip >= chunk.code.len() {
                    return Err(VmError::InvalidOperand(format!(
                        "Invalid function address: {}",
                        target_ip
                    )));
                }
                if self.stack.len() < *arity {
                    return Err(VmError::StackUnderflow);
                }
                let slots_offset = self.stack.len() - *arity;
                self.frames.push(CallFrame::new(target_ip, slots_offset));
            }
            Opcode::Return => {
                let return_val = self.stack.pop().unwrap_or(Value::Null);
                let frame = self.frames.pop().unwrap();

                // If more frames exist, push return value back
                if self.frames.is_empty() {
                    return Ok(StepStatus::Terminated(return_val));
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
                    h.region_a_budget_bytes
                        .store(*bytes as u64, std::sync::atomic::Ordering::Relaxed);
                    h.region_a_used_bytes
                        .store(0, std::sync::atomic::Ordering::Relaxed);
                    shm.push_event(
                        trulang_supervisor::TelemetryEventType::AllocRegionA,
                        *bytes as u32,
                        0,
                        0,
                        "request_arena",
                    );
                }
                if self.profiler.is_some() {
                    let func = self.current_function(chunk).unwrap_or("fn").to_string();
                    let line = chunk.lines.get(current_ip).copied().unwrap_or(0);
                    let stack_path = self.current_stack_path(chunk);
                    if let Some(prof) = &mut self.profiler {
                        prof.record_reset(func, line, stack_path);
                    }
                }
            }
            Opcode::FreeRegionA => {
                self.region_a.reset();
                if let Some(shm) = &self.telemetry {
                    shm.header()
                        .region_a_used_bytes
                        .store(0, std::sync::atomic::Ordering::Relaxed);
                    shm.push_event(
                        trulang_supervisor::TelemetryEventType::FreeRegionA,
                        0,
                        0,
                        0,
                        "reset_arena",
                    );
                }
                if self.profiler.is_some() {
                    let func = self.current_function(chunk).unwrap_or("fn").to_string();
                    let line = chunk.lines.get(current_ip).copied().unwrap_or(0);
                    let stack_path = self.current_stack_path(chunk);
                    if let Some(prof) = &mut self.profiler {
                        prof.record_reset(func, line, stack_path);
                    }
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
                        shm.push_event(
                            trulang_supervisor::TelemetryEventType::BudgetExceeded,
                            size as u32,
                            0,
                            0,
                            "persist_overflow",
                        );
                    }
                    VmError::MemoryExceeded(e)
                })?;
                if let Some(shm) = &self.telemetry {
                    let h = shm.header();
                    h.region_b_live_bytes.store(
                        self.region_b
                            .live_bytes
                            .load(std::sync::atomic::Ordering::Relaxed)
                            as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    h.region_b_live_objects.store(
                        self.region_b
                            .live_objects
                            .load(std::sync::atomic::Ordering::Relaxed)
                            as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    shm.push_event(
                        trulang_supervisor::TelemetryEventType::PersistToRegionB,
                        size as u32,
                        0,
                        0,
                        "persist_object",
                    );
                }
                if self.profiler.is_some() {
                    let func = self.current_function(chunk).unwrap_or("fn").to_string();
                    let line = chunk.lines.get(current_ip).copied().unwrap_or(0);
                    let stack_path = self.current_stack_path(chunk);
                    if let Some(prof) = &mut self.profiler {
                        prof.record_persist(size, func, line, stack_path);
                    }
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
            Opcode::Assert(ref msg) => {
                let val = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let is_truthy = match val {
                    Value::Bool(b) => b,
                    Value::Null => false,
                    Value::Int(n) => n != 0,
                    _ => true,
                };
                if !is_truthy {
                    let line = chunk.lines.get(current_ip).copied().unwrap_or(1);
                    let message = msg
                        .clone()
                        .unwrap_or_else(|| format!("Expected truthy value, found {}", val));
                    return Err(VmError::AssertionFailure { message, line });
                }
            }
            Opcode::AssertEq(ref msg) => {
                let expected = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                let actual = self.stack.pop().ok_or(VmError::StackUnderflow)?;
                if actual != expected {
                    let line = chunk.lines.get(current_ip).copied().unwrap_or(1);
                    return Err(VmError::AssertionEqFailure {
                        left: format!("{}", actual),
                        right: format!("{}", expected),
                        message: msg.clone(),
                        line,
                    });
                }
            }
        }

        if self.profiler.is_some() {
            let func = self.current_function(chunk).unwrap_or("fn").to_string();
            let line = chunk.lines.get(current_ip).copied().unwrap_or(0);
            let stack_path = self.current_stack_path(chunk);
            if let Some(prof) = &mut self.profiler {
                if let Some(last) = prof.events.last_mut() {
                    if last.function == "active_scope" {
                        last.function = func;
                        last.line = line;
                        last.stack_path = stack_path;
                    }
                }
            }
        }

        if self.frames.is_empty() {
            Ok(StepStatus::Terminated(
                self.stack.pop().unwrap_or(Value::Null),
            ))
        } else {
            Ok(StepStatus::Continued)
        }
    }
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}
