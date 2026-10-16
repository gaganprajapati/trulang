use crate::protocol::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;
use trulang_compiler::bytecode::{Chunk, Value};
use trulang_compiler::compile;
use trulang_syntax::parse;
use trulang_vm::{StepStatus, VirtualMachine};

pub struct DebugSession {
    seq: u64,
    pub breakpoints: HashMap<String, HashSet<usize>>,
    pub program_path: Option<String>,
    pub chunk: Option<Chunk>,
    pub vm: Option<VirtualMachine>,
    pub is_running: bool,
    pub is_paused: bool,
    pub is_terminated: bool,
    pub stop_on_entry: bool,
    pub last_stopped_line: Option<usize>,
}

impl Default for DebugSession {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugSession {
    pub fn new() -> Self {
        Self {
            seq: 1,
            breakpoints: HashMap::new(),
            program_path: None,
            chunk: None,
            vm: None,
            is_running: false,
            is_paused: false,
            is_terminated: false,
            stop_on_entry: false,
            last_stopped_line: None,
        }
    }

    fn next_seq(&mut self) -> u64 {
        let s = self.seq;
        self.seq += 1;
        s
    }

    pub fn handle_request(&mut self, req: DapRequest) -> (DapResponse, Vec<DapEvent>) {
        let mut events = Vec::new();
        let req_seq = req.seq;
        let command = req.command.clone();

        match command.as_str() {
            "initialize" => {
                let caps = Capabilities {
                    supports_configuration_done_request: true,
                    supports_function_breakpoints: false,
                    supports_conditional_breakpoints: false,
                    supports_evaluate_for_hovers: true,
                    supports_step_back: false,
                    supports_set_variable: false,
                };
                let resp_seq = self.next_seq();
                let resp = DapResponse::success(
                    resp_seq,
                    req_seq,
                    &command,
                    Some(serde_json::to_value(caps).unwrap()),
                );

                let event_seq = self.next_seq();
                events.push(DapEvent::new(event_seq, "initialized", None));
                (resp, events)
            }

            "setBreakpoints" => {
                let args: Result<SetBreakpointsArguments, _> =
                    serde_json::from_value(req.arguments.unwrap_or_default());
                match args {
                    Ok(args) => {
                        let path = args.source.path.clone().unwrap_or_default();
                        let canonical_path = fs::canonicalize(&path)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| path.clone());

                        let mut lines = HashSet::new();
                        if let Some(bps) = args.breakpoints {
                            for bp in bps {
                                lines.insert(bp.line);
                            }
                        } else if let Some(ls) = args.lines {
                            for l in ls {
                                lines.insert(l);
                            }
                        }

                        let verified_bps: Vec<Breakpoint> = lines
                            .iter()
                            .map(|&l| Breakpoint {
                                id: Some(l),
                                verified: true,
                                line: Some(l),
                                source: Some(args.source.clone()),
                                message: None,
                            })
                            .collect();

                        self.breakpoints.insert(canonical_path, lines);

                        let resp_seq = self.next_seq();
                        let resp = DapResponse::success(
                            resp_seq,
                            req_seq,
                            &command,
                            Some(serde_json::json!({ "breakpoints": verified_bps })),
                        );
                        (resp, events)
                    }
                    Err(e) => {
                        let resp_seq = self.next_seq();
                        (
                            DapResponse::error(resp_seq, req_seq, &command, e.to_string()),
                            events,
                        )
                    }
                }
            }

            "configurationDone" => {
                let resp_seq = self.next_seq();
                (
                    DapResponse::success(resp_seq, req_seq, &command, None),
                    events,
                )
            }

            "launch" => {
                let args: Result<LaunchArguments, _> =
                    serde_json::from_value(req.arguments.unwrap_or_default());
                match args {
                    Ok(args) => {
                        let prog_path = args.program.clone();
                        self.stop_on_entry = args.stop_on_entry.unwrap_or(false);

                        let source = match fs::read_to_string(&prog_path) {
                            Ok(s) => s,
                            Err(e) => {
                                let resp_seq = self.next_seq();
                                return (
                                    DapResponse::error(
                                        resp_seq,
                                        req_seq,
                                        &command,
                                        format!(
                                            "Failed to read source file '{}': {}",
                                            prog_path, e
                                        ),
                                    ),
                                    events,
                                );
                            }
                        };

                        let ast = match parse(&source) {
                            Ok(a) => a,
                            Err(e) => {
                                let resp_seq = self.next_seq();
                                return (
                                    DapResponse::error(
                                        resp_seq,
                                        req_seq,
                                        &command,
                                        format!("Syntax error in '{}': {}", prog_path, e),
                                    ),
                                    events,
                                );
                            }
                        };

                        let chunk = match compile(&ast) {
                            Ok(c) => c,
                            Err(e) => {
                                let resp_seq = self.next_seq();
                                return (
                                    DapResponse::error(
                                        resp_seq,
                                        req_seq,
                                        &command,
                                        format!("Compilation error in '{}': {}", prog_path, e),
                                    ),
                                    events,
                                );
                            }
                        };

                        let req_budget = args.request_budget.unwrap_or(16 * 1024 * 1024);
                        let max_req_budget = args.max_request_budget;
                        let pers_budget = args.persistent_budget.unwrap_or(512 * 1024 * 1024);

                        let mut vm = if let Some(max_b) = max_req_budget {
                            VirtualMachine::with_elastic_budgets(
                                req_budget,
                                Some(max_b),
                                pers_budget,
                            )
                        } else {
                            VirtualMachine::with_budgets(req_budget, pers_budget)
                        };

                        vm.init_execution(&chunk);

                        let canonical = fs::canonicalize(&prog_path)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| prog_path.clone());
                        self.program_path = Some(canonical);
                        self.chunk = Some(chunk);
                        self.vm = Some(vm);
                        self.is_running = true;
                        self.is_terminated = false;

                        let resp_seq = self.next_seq();
                        let resp = DapResponse::success(resp_seq, req_seq, &command, None);

                        if self.stop_on_entry {
                            self.is_paused = true;
                            let line = self.get_current_line().unwrap_or(1);
                            self.last_stopped_line = Some(line);
                            let evt_seq = self.next_seq();
                            events.push(DapEvent::new(
                                evt_seq,
                                "stopped",
                                Some(serde_json::json!({
                                    "reason": "entry",
                                    "threadId": 1
                                })),
                            ));
                        } else {
                            self.run_until_breakpoint_or_exit(&mut events);
                        }

                        (resp, events)
                    }
                    Err(e) => {
                        let resp_seq = self.next_seq();
                        (
                            DapResponse::error(resp_seq, req_seq, &command, e.to_string()),
                            events,
                        )
                    }
                }
            }

            "threads" => {
                let resp_seq = self.next_seq();
                let resp = DapResponse::success(
                    resp_seq,
                    req_seq,
                    &command,
                    Some(serde_json::json!({
                        "threads": [
                            Thread { id: 1, name: "Main Actor Thread".to_string() }
                        ]
                    })),
                );
                (resp, events)
            }

            "stackTrace" => {
                let chunk = match &self.chunk {
                    Some(c) => c,
                    None => {
                        let resp_seq = self.next_seq();
                        return (
                            DapResponse::success(
                                resp_seq,
                                req_seq,
                                &command,
                                Some(serde_json::json!({ "stackFrames": [], "totalFrames": 0 })),
                            ),
                            events,
                        );
                    }
                };

                let vm = match &self.vm {
                    Some(v) => v,
                    None => {
                        let resp_seq = self.next_seq();
                        return (
                            DapResponse::success(
                                resp_seq,
                                req_seq,
                                &command,
                                Some(serde_json::json!({ "stackFrames": [], "totalFrames": 0 })),
                            ),
                            events,
                        );
                    }
                };

                let mut stack_frames = Vec::new();
                let file_name = self
                    .program_path
                    .as_ref()
                    .and_then(|p| Path::new(p).file_name())
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| "main.tr".to_string());

                for (idx, frame) in vm.frames.iter().enumerate().rev() {
                    let mut best: Option<(&str, usize)> = None;
                    for (name, &entry) in &chunk.functions {
                        if entry <= frame.ip {
                            if let Some((_, best_entry)) = best {
                                if entry > best_entry {
                                    best = Some((name.as_str(), entry));
                                }
                            } else {
                                best = Some((name.as_str(), entry));
                            }
                        }
                    }
                    let func_name = best.map(|(n, _)| n).unwrap_or("<main>");

                    let line = chunk.lines.get(frame.ip).copied().unwrap_or(1);
                    stack_frames.push(StackFrame {
                        id: idx as i64,
                        name: func_name.to_string(),
                        source: Some(Source {
                            name: Some(file_name.clone()),
                            path: self.program_path.clone(),
                        }),
                        line,
                        column: 1,
                    });
                }

                let total = stack_frames.len();
                let resp_seq = self.next_seq();
                let resp = DapResponse::success(
                    resp_seq,
                    req_seq,
                    &command,
                    Some(serde_json::json!({
                        "stackFrames": stack_frames,
                        "totalFrames": total
                    })),
                );
                (resp, events)
            }

            "scopes" => {
                let resp_seq = self.next_seq();
                let args: Result<ScopesArguments, _> =
                    serde_json::from_value(req.arguments.unwrap_or_default());
                let frame_id = args.map(|a| a.frame_id).unwrap_or(0);

                let scopes = vec![
                    Scope {
                        name: "Region A (Execution Bump Arena)".to_string(),
                        variables_reference: 1000 + frame_id,
                        expensive: false,
                    },
                    Scope {
                        name: "Region B (Persistent DAG State)".to_string(),
                        variables_reference: 2000 + frame_id,
                        expensive: false,
                    },
                    Scope {
                        name: "Locals & Stack".to_string(),
                        variables_reference: 3000 + frame_id,
                        expensive: false,
                    },
                ];

                let resp = DapResponse::success(
                    resp_seq,
                    req_seq,
                    &command,
                    Some(serde_json::json!({ "scopes": scopes })),
                );
                (resp, events)
            }

            "variables" => {
                let args: Result<VariablesArguments, _> =
                    serde_json::from_value(req.arguments.unwrap_or_default());
                let var_ref = args.map(|a| a.variables_reference).unwrap_or(0);
                let resp_seq = self.next_seq();

                let mut vars = Vec::new();
                if let Some(vm) = &self.vm {
                    if (1000..2000).contains(&var_ref) {
                        // Region A Scope
                        vars.push(Variable {
                            name: "used_bytes".to_string(),
                            value: format!("{} bytes", vm.region_a.used_bytes),
                            var_type: Some("usize".to_string()),
                            variables_reference: 0,
                        });
                        vars.push(Variable {
                            name: "budget_bytes".to_string(),
                            value: format!("{} bytes", vm.region_a.budget_bytes),
                            var_type: Some("usize".to_string()),
                            variables_reference: 0,
                        });
                        vars.push(Variable {
                            name: "allocation_count".to_string(),
                            value: format!("{}", vm.region_a.allocation_count),
                            var_type: Some("usize".to_string()),
                            variables_reference: 0,
                        });
                        vars.push(Variable {
                            name: "burst_count".to_string(),
                            value: format!("{}", vm.region_a.burst_count),
                            var_type: Some("usize".to_string()),
                            variables_reference: 0,
                        });
                        vars.push(Variable {
                            name: "is_bursting".to_string(),
                            value: format!("{}", vm.region_a.is_bursting()),
                            var_type: Some("bool".to_string()),
                            variables_reference: 0,
                        });
                    } else if (2000..3000).contains(&var_ref) {
                        // Region B Scope
                        vars.push(Variable {
                            name: "live_bytes".to_string(),
                            value: format!(
                                "{} bytes",
                                vm.region_b.live_bytes.load(Ordering::Relaxed)
                            ),
                            var_type: Some("usize".to_string()),
                            variables_reference: 0,
                        });
                        vars.push(Variable {
                            name: "budget_bytes".to_string(),
                            value: format!("{} bytes", vm.region_b.budget_bytes),
                            var_type: Some("usize".to_string()),
                            variables_reference: 0,
                        });
                        vars.push(Variable {
                            name: "live_objects".to_string(),
                            value: format!("{}", vm.region_b.live_objects.load(Ordering::Relaxed)),
                            var_type: Some("usize".to_string()),
                            variables_reference: 0,
                        });
                        vars.push(Variable {
                            name: "total_allocated_bytes".to_string(),
                            value: format!(
                                "{} bytes",
                                vm.region_b.total_allocated_bytes.load(Ordering::Relaxed)
                            ),
                            var_type: Some("usize".to_string()),
                            variables_reference: 0,
                        });
                    } else if (3000..4000).contains(&var_ref) {
                        // Locals Scope
                        let frame_idx = (var_ref - 3000) as usize;
                        if let Some(frame) = vm.frames.get(frame_idx) {
                            let slots = &vm.stack[frame.slots_offset..];
                            for (i, val) in slots.iter().enumerate() {
                                vars.push(Variable {
                                    name: format!("slot_{}", i),
                                    value: format!("{}", val),
                                    var_type: Some(
                                        match val {
                                            Value::Int(_) => "i64",
                                            Value::Float(_) => "f64",
                                            Value::Bool(_) => "bool",
                                            Value::String(_) => "string",
                                            Value::Array(_) => "array",
                                            Value::Map(_) => "map",
                                            Value::Object(_) => "object",
                                            Value::Handle(_) => "handle",
                                            Value::Null => "null",
                                        }
                                        .to_string(),
                                    ),
                                    variables_reference: 0,
                                });
                            }
                        }
                    }
                }

                let resp = DapResponse::success(
                    resp_seq,
                    req_seq,
                    &command,
                    Some(serde_json::json!({ "variables": vars })),
                );
                (resp, events)
            }

            "continue" => {
                let resp_seq = self.next_seq();
                let resp = DapResponse::success(
                    resp_seq,
                    req_seq,
                    &command,
                    Some(serde_json::json!({ "allThreadsContinued": true })),
                );
                self.run_until_breakpoint_or_exit(&mut events);
                (resp, events)
            }

            "next" => {
                // Step Over
                let current_depth = self.vm.as_ref().map(|v| v.frames.len()).unwrap_or(0);
                let current_line = self.get_current_line().unwrap_or(0);
                self.step_until(&mut events, |vm, l| {
                    vm.frames.len() < current_depth
                        || (vm.frames.len() == current_depth && l != current_line)
                });
                let resp_seq = self.next_seq();
                (
                    DapResponse::success(resp_seq, req_seq, &command, None),
                    events,
                )
            }

            "stepIn" => {
                // Step In
                let current_line = self.get_current_line().unwrap_or(0);
                self.step_until(&mut events, |_vm, l| l != current_line);
                let resp_seq = self.next_seq();
                (
                    DapResponse::success(resp_seq, req_seq, &command, None),
                    events,
                )
            }

            "stepOut" => {
                // Step Out
                let target_depth = self
                    .vm
                    .as_ref()
                    .map(|v| v.frames.len().saturating_sub(1))
                    .unwrap_or(0);
                self.step_until(&mut events, |vm, _l| vm.frames.len() <= target_depth);
                let resp_seq = self.next_seq();
                (
                    DapResponse::success(resp_seq, req_seq, &command, None),
                    events,
                )
            }

            "evaluate" => {
                let args: Result<EvaluateArguments, _> =
                    serde_json::from_value(req.arguments.unwrap_or_default());
                let expr = args.map(|a| a.expression).unwrap_or_default();
                let resp_seq = self.next_seq();

                let result_str = if let Some(vm) = &self.vm {
                    match expr.as_str() {
                        "region_a" => format!(
                            "Region A: {} / {} bytes",
                            vm.region_a.used_bytes, vm.region_a.budget_bytes
                        ),
                        "region_b" => format!(
                            "Region B: {} live bytes",
                            vm.region_b.live_bytes.load(Ordering::Relaxed)
                        ),
                        _ => {
                            if let Some(top) = vm.stack.last() {
                                format!("{}", top)
                            } else {
                                "null".to_string()
                            }
                        }
                    }
                } else {
                    "null".to_string()
                };

                let resp = DapResponse::success(
                    resp_seq,
                    req_seq,
                    &command,
                    Some(serde_json::json!({
                        "result": result_str,
                        "variablesReference": 0
                    })),
                );
                (resp, events)
            }

            "disconnect" => {
                self.is_running = false;
                self.is_paused = false;
                self.is_terminated = true;
                let resp_seq = self.next_seq();
                (
                    DapResponse::success(resp_seq, req_seq, &command, None),
                    events,
                )
            }

            _ => {
                let resp_seq = self.next_seq();
                (
                    DapResponse::error(
                        resp_seq,
                        req_seq,
                        &command,
                        format!("Unsupported DAP request: '{}'", command),
                    ),
                    events,
                )
            }
        }
    }

    pub fn get_current_line(&self) -> Option<usize> {
        let vm = self.vm.as_ref()?;
        let chunk = self.chunk.as_ref()?;
        vm.current_line(chunk)
    }

    fn run_until_breakpoint_or_exit(&mut self, events: &mut Vec<DapEvent>) {
        let chunk = match self.chunk.clone() {
            Some(c) => c,
            None => return,
        };

        let target_breakpoints = self
            .program_path
            .as_ref()
            .and_then(|p| self.breakpoints.get(p))
            .cloned()
            .unwrap_or_default();

        while let Some(vm) = self.vm.as_mut() {
            if vm.is_terminated() {
                self.is_running = false;
                self.is_paused = false;
                self.is_terminated = true;
                let evt_seq = self.next_seq();
                events.push(DapEvent::new(evt_seq, "terminated", None));
                return;
            }

            let cur_line = vm.current_line(&chunk).unwrap_or(0);
            if cur_line > 0
                && self.last_stopped_line != Some(cur_line)
                && target_breakpoints.contains(&cur_line)
            {
                self.is_paused = true;
                self.last_stopped_line = Some(cur_line);
                let evt_seq = self.next_seq();
                events.push(DapEvent::new(
                    evt_seq,
                    "stopped",
                    Some(serde_json::json!({
                        "reason": "breakpoint",
                        "threadId": 1
                    })),
                ));
                return;
            }

            match vm.step_instruction(&chunk) {
                Ok(StepStatus::Continued) => {}
                Ok(StepStatus::Terminated(val)) => {
                    self.is_running = false;
                    self.is_paused = false;
                    self.is_terminated = true;
                    let out_seq = self.next_seq();
                    events.push(DapEvent::new(
                        out_seq,
                        "output",
                        Some(serde_json::json!({
                            "category": "stdout",
                            "output": format!("Program terminated with result: {}\n", val)
                        })),
                    ));
                    let term_seq = self.next_seq();
                    events.push(DapEvent::new(term_seq, "terminated", None));
                    return;
                }
                Err(e) => {
                    self.is_running = false;
                    self.is_paused = false;
                    self.is_terminated = true;
                    let err_seq = self.next_seq();
                    events.push(DapEvent::new(
                        err_seq,
                        "output",
                        Some(serde_json::json!({
                            "category": "stderr",
                            "output": format!("Runtime error: {}\n", e)
                        })),
                    ));
                    let term_seq = self.next_seq();
                    events.push(DapEvent::new(term_seq, "terminated", None));
                    return;
                }
            }
        }
    }

    fn step_until<F>(&mut self, events: &mut Vec<DapEvent>, predicate: F)
    where
        F: Fn(&VirtualMachine, usize) -> bool,
    {
        let chunk = match self.chunk.clone() {
            Some(c) => c,
            None => return,
        };

        while let Some(vm) = self.vm.as_mut() {
            if vm.is_terminated() {
                self.is_running = false;
                self.is_paused = false;
                self.is_terminated = true;
                let evt_seq = self.next_seq();
                events.push(DapEvent::new(evt_seq, "terminated", None));
                return;
            }

            match vm.step_instruction(&chunk) {
                Ok(StepStatus::Continued) => {
                    let cur_line = vm.current_line(&chunk).unwrap_or(0);
                    if predicate(vm, cur_line) {
                        self.is_paused = true;
                        self.last_stopped_line = Some(cur_line);
                        let evt_seq = self.next_seq();
                        events.push(DapEvent::new(
                            evt_seq,
                            "stopped",
                            Some(serde_json::json!({
                                "reason": "step",
                                "threadId": 1
                            })),
                        ));
                        return;
                    }
                }
                Ok(StepStatus::Terminated(val)) => {
                    self.is_running = false;
                    self.is_paused = false;
                    self.is_terminated = true;
                    let out_seq = self.next_seq();
                    events.push(DapEvent::new(
                        out_seq,
                        "output",
                        Some(serde_json::json!({
                            "category": "stdout",
                            "output": format!("Program finished: {}\n", val)
                        })),
                    ));
                    let term_seq = self.next_seq();
                    events.push(DapEvent::new(term_seq, "terminated", None));
                    return;
                }
                Err(e) => {
                    self.is_running = false;
                    self.is_paused = false;
                    self.is_terminated = true;
                    let out_seq = self.next_seq();
                    events.push(DapEvent::new(
                        out_seq,
                        "output",
                        Some(serde_json::json!({
                            "category": "stderr",
                            "output": format!("Runtime error: {}\n", e)
                        })),
                    ));
                    let term_seq = self.next_seq();
                    events.push(DapEvent::new(term_seq, "terminated", None));
                    return;
                }
            }
        }
    }
}
