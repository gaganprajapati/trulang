pub mod protocol;
pub mod server;
pub mod session;

pub use protocol::{
    Breakpoint, Capabilities, DapEvent, DapRequest, DapResponse, Scope, Source, SourceBreakpoint,
    StackFrame, Thread, Variable,
};
pub use server::DapServer;
pub use session::DebugSession;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_dap_initialize_handshake() {
        let mut session = DebugSession::new();
        let req = DapRequest {
            seq: 1,
            msg_type: "request".to_string(),
            command: "initialize".to_string(),
            arguments: Some(serde_json::json!({
                "clientID": "vscode",
                "adapterID": "trulang"
            })),
        };

        let (resp, events) = session.handle_request(req);
        assert!(resp.success);
        assert_eq!(resp.command, "initialize");
        assert_eq!(resp.request_seq, 1);

        let body = resp.body.expect("Initialize response has body");
        let caps: Capabilities = serde_json::from_value(body).expect("Valid capabilities");
        assert!(caps.supports_configuration_done_request);
        assert!(caps.supports_evaluate_for_hovers);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "initialized");
    }

    #[test]
    fn test_dap_set_breakpoints_and_launch() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!("dap_test_{}.tr", std::process::id()));
        let source_code = r#"
fn calc(a: i64, b: i64) -> i64 {
    let sum = a + b;
    return sum;
}

fn main() -> i64 {
    let mut x = 10;
    let mut y = 20;
    let res = calc(x, y);
    return res;
}
"#;
        std::fs::write(&file_path, source_code).expect("Write test file failed");

        let mut session = DebugSession::new();
        let path_str = file_path.to_str().unwrap().to_string();

        // 1. Set breakpoints at line 10 (let res = calc(x, y);)
        let set_bp_req = DapRequest {
            seq: 1,
            msg_type: "request".to_string(),
            command: "setBreakpoints".to_string(),
            arguments: Some(serde_json::json!({
                "source": { "path": path_str },
                "lines": [10]
            })),
        };
        let (bp_resp, _) = session.handle_request(set_bp_req);
        assert!(bp_resp.success);

        // 2. Launch program
        let launch_req = DapRequest {
            seq: 2,
            msg_type: "request".to_string(),
            command: "launch".to_string(),
            arguments: Some(serde_json::json!({
                "program": path_str,
                "stopOnEntry": false
            })),
        };
        let (launch_resp, launch_events) = session.handle_request(launch_req);
        assert!(launch_resp.success);

        // Verify stopped at breakpoint
        assert_eq!(launch_events.len(), 1);
        assert_eq!(launch_events[0].event, "stopped");
        let stop_body = launch_events[0].body.as_ref().unwrap();
        assert_eq!(stop_body["reason"], "breakpoint");

        // 3. Request stack trace
        let st_req = DapRequest {
            seq: 3,
            msg_type: "request".to_string(),
            command: "stackTrace".to_string(),
            arguments: Some(serde_json::json!({ "threadId": 1 })),
        };
        let (st_resp, _) = session.handle_request(st_req);
        assert!(st_resp.success);
        let st_body = st_resp.body.unwrap();
        let frames = st_body["stackFrames"].as_array().unwrap();
        assert!(!frames.is_empty());
        let frame_name = frames[0]["name"].as_str().unwrap();
        assert_eq!(frame_name, "main");

        // 4. Request scopes
        let scopes_req = DapRequest {
            seq: 4,
            msg_type: "request".to_string(),
            command: "scopes".to_string(),
            arguments: Some(serde_json::json!({ "frameId": 0 })),
        };
        let (scopes_resp, _) = session.handle_request(scopes_req);
        assert!(scopes_resp.success);
        let scopes_body = scopes_resp.body.unwrap();
        let scopes_arr = scopes_body["scopes"].as_array().unwrap();
        assert_eq!(scopes_arr.len(), 3);
        assert!(scopes_arr[0]["name"].as_str().unwrap().contains("Region A"));
        assert!(scopes_arr[1]["name"].as_str().unwrap().contains("Region B"));

        // 5. Request variables for Region A
        let var_ref_a = scopes_arr[0]["variablesReference"].as_i64().unwrap();
        let vars_req_a = DapRequest {
            seq: 5,
            msg_type: "request".to_string(),
            command: "variables".to_string(),
            arguments: Some(serde_json::json!({ "variablesReference": var_ref_a })),
        };
        let (vars_resp_a, _) = session.handle_request(vars_req_a);
        assert!(vars_resp_a.success);
        let vars_a = vars_resp_a.body.unwrap()["variables"]
            .as_array()
            .unwrap()
            .clone();
        assert!(vars_a.iter().any(|v| v["name"] == "used_bytes"));
        assert!(vars_a.iter().any(|v| v["name"] == "budget_bytes"));

        // 6. Request variables for Region B
        let var_ref_b = scopes_arr[1]["variablesReference"].as_i64().unwrap();
        let vars_req_b = DapRequest {
            seq: 6,
            msg_type: "request".to_string(),
            command: "variables".to_string(),
            arguments: Some(serde_json::json!({ "variablesReference": var_ref_b })),
        };
        let (vars_resp_b, _) = session.handle_request(vars_req_b);
        assert!(vars_resp_b.success);
        let vars_b = vars_resp_b.body.unwrap()["variables"]
            .as_array()
            .unwrap()
            .clone();
        assert!(vars_b.iter().any(|v| v["name"] == "live_bytes"));
        assert!(vars_b.iter().any(|v| v["name"] == "budget_bytes"));

        // 7. Continue to completion
        let cont_req = DapRequest {
            seq: 7,
            msg_type: "request".to_string(),
            command: "continue".to_string(),
            arguments: Some(serde_json::json!({ "threadId": 1 })),
        };
        let (cont_resp, cont_events) = session.handle_request(cont_req);
        assert!(cont_resp.success);
        assert!(cont_events.iter().any(|e| e.event == "terminated"));

        let _ = std::fs::remove_file(file_path);
    }

    #[test]
    fn test_dap_step_over_and_step_in() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!("dap_step_test_{}.tr", std::process::id()));
        let source_code = r#"
fn multiply(a: i64, b: i64) -> i64 {
    return a * b;
}

fn main() -> i64 {
    let p = 6;
    let q = 7;
    let prod = multiply(p, q);
    return prod;
}
"#;
        std::fs::write(&file_path, source_code).expect("Write test file failed");

        let mut session = DebugSession::new();
        let path_str = file_path.to_str().unwrap().to_string();

        // 1. Launch with stopOnEntry
        let launch_req = DapRequest {
            seq: 1,
            msg_type: "request".to_string(),
            command: "launch".to_string(),
            arguments: Some(serde_json::json!({
                "program": path_str,
                "stopOnEntry": true
            })),
        };
        let (launch_resp, launch_events) = session.handle_request(launch_req);
        assert!(launch_resp.success);
        assert_eq!(launch_events[0].event, "stopped");
        assert_eq!(launch_events[0].body.as_ref().unwrap()["reason"], "entry");

        // 2. Step Over (next)
        let next_req = DapRequest {
            seq: 2,
            msg_type: "request".to_string(),
            command: "next".to_string(),
            arguments: Some(serde_json::json!({ "threadId": 1 })),
        };
        let (next_resp, next_events) = session.handle_request(next_req);
        assert!(next_resp.success);
        assert!(!next_events.is_empty());
        assert_eq!(next_events[0].event, "stopped");
        assert_eq!(next_events[0].body.as_ref().unwrap()["reason"], "step");

        // 3. Step In (stepIn)
        let stepin_req = DapRequest {
            seq: 3,
            msg_type: "request".to_string(),
            command: "stepIn".to_string(),
            arguments: Some(serde_json::json!({ "threadId": 1 })),
        };
        let (stepin_resp, stepin_events) = session.handle_request(stepin_req);
        assert!(stepin_resp.success);
        assert!(!stepin_events.is_empty());
        assert_eq!(stepin_events[0].event, "stopped");
        assert_eq!(stepin_events[0].body.as_ref().unwrap()["reason"], "step");

        // 4. Continue to finish
        let cont_req = DapRequest {
            seq: 4,
            msg_type: "request".to_string(),
            command: "continue".to_string(),
            arguments: Some(serde_json::json!({ "threadId": 1 })),
        };
        let (cont_resp, cont_events) = session.handle_request(cont_req);
        assert!(cont_resp.success);
        assert!(cont_events.iter().any(|e| e.event == "terminated"));

        let _ = std::fs::remove_file(file_path);
    }

    #[test]
    fn test_dap_server_stdio_mock() {
        let mut input_buf = Vec::new();

        // 1. Initialize message framed
        let init_json = serde_json::to_string(&DapRequest {
            seq: 1,
            msg_type: "request".to_string(),
            command: "initialize".to_string(),
            arguments: None,
        })
        .unwrap();
        trulang_lsp::transport::Transport::write_message(&mut input_buf, &init_json).unwrap();

        // 2. Disconnect message framed
        let disc_json = serde_json::to_string(&DapRequest {
            seq: 2,
            msg_type: "request".to_string(),
            command: "disconnect".to_string(),
            arguments: None,
        })
        .unwrap();
        trulang_lsp::transport::Transport::write_message(&mut input_buf, &disc_json).unwrap();

        let mut reader = Cursor::new(input_buf);
        let mut output_buf = Vec::new();

        let mut server = DapServer::new();
        server
            .run(&mut reader, &mut output_buf)
            .expect("Server run succeeded");

        let output_str = String::from_utf8(output_buf).expect("Valid UTF-8 output");
        assert!(output_str.contains("Content-Length:"));
        assert!(output_str.contains("\"command\":\"initialize\""));
        assert!(output_str.contains("\"event\":\"initialized\""));
        assert!(output_str.contains("\"command\":\"disconnect\""));
    }
}
