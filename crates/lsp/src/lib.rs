pub mod analysis;
pub mod formatter;
pub mod protocol;
pub mod server;
pub mod transport;

pub use analysis::Analyzer;
pub use formatter::Formatter;
pub use server::LanguageServer;
pub use transport::Transport;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_lsp_initialize_handshake() {
        let mut server = LanguageServer::new();
        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });

        let (resp_str, notifs) = server.handle_message(&init_req.to_string());
        assert!(notifs.is_empty());
        assert!(resp_str.is_some());

        let resp: serde_json::Value = serde_json::from_str(&resp_str.unwrap()).unwrap();
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["capabilities"]["hoverProvider"], true);
        assert_eq!(resp["result"]["capabilities"]["definitionProvider"], true);
        assert_eq!(resp["result"]["capabilities"]["documentFormattingProvider"], true);
    }

    #[test]
    fn test_lsp_did_open_valid_code_emits_empty_diagnostics() {
        let mut server = LanguageServer::new();
        let doc = r#"
service TestSvc {
    memory_budget request = 16 MB max 64 MB;
    memory_budget persistent = 512 MB;

    fn handle(x: i64) -> i64 {
        return x * 2;
    }
}
"#;
        let did_open = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///test.vl",
                    "languageId": "vlang",
                    "version": 1,
                    "text": doc
                }
            }
        });

        let (resp, notifs) = server.handle_message(&did_open.to_string());
        assert!(resp.is_none());
        assert_eq!(notifs.len(), 1);

        let notif_val: serde_json::Value = serde_json::from_str(&notifs[0]).unwrap();
        assert_eq!(notif_val["method"], "textDocument/publishDiagnostics");
        let diags = notif_val["params"]["diagnostics"].as_array().unwrap();
        assert!(diags.is_empty(), "Valid code must have 0 diagnostics");
    }

    #[test]
    fn test_lsp_did_open_syntax_error_publishes_diagnostic() {
        let mut server = LanguageServer::new();
        let doc = "service Broken {\n    let x =\n}";
        let did_open = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///broken.vl",
                    "languageId": "vlang",
                    "version": 1,
                    "text": doc
                }
            }
        });

        let (_, notifs) = server.handle_message(&did_open.to_string());
        assert_eq!(notifs.len(), 1);

        let notif_val: serde_json::Value = serde_json::from_str(&notifs[0]).unwrap();
        let diags = notif_val["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0]["severity"], 1);
        assert!(diags[0]["message"].as_str().unwrap().contains("Unexpected token in expression"));
    }

    #[test]
    fn test_lsp_did_open_type_error_publishes_diagnostic_with_hint() {
        let mut server = LanguageServer::new();
        let doc = r#"
fn test() {
    let x = 10;
    x = 20;
}
"#;
        let did_open = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///immut.vl",
                    "languageId": "vlang",
                    "version": 1,
                    "text": doc
                }
            }
        });

        let (_, notifs) = server.handle_message(&did_open.to_string());
        assert_eq!(notifs.len(), 1);

        let notif_val: serde_json::Value = serde_json::from_str(&notifs[0]).unwrap();
        let diags = notif_val["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(diags.len(), 1);
        let msg = diags[0]["message"].as_str().unwrap();
        assert!(msg.contains("Cannot reassign immutable variable 'x'"));
        assert!(msg.contains("Hint: Declare as 'let mut'"));
    }

    #[test]
    fn test_lsp_hover_on_service_actor() {
        let mut server = LanguageServer::new();
        let doc = r#"
service PaymentService {
    memory_budget request = 16 MB max 64 MB;
    memory_budget persistent = 256 MB;

    let mut cache = Map::new();

    fn process(req: i64) -> i64 {
        return req;
    }
}
"#;
        // Open document first
        server.handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///payment.vl",
                    "languageId": "vlang",
                    "version": 1,
                    "text": doc
                }
            }
        }).to_string());

        // Hover over PaymentService (line 1, char 10)
        let hover_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": "file:///payment.vl" },
                "position": { "line": 1, "character": 10 }
            }
        });

        let (resp_str, _) = server.handle_message(&hover_req.to_string());
        let resp: serde_json::Value = serde_json::from_str(&resp_str.unwrap()).unwrap();
        let markdown = resp["result"]["contents"]["value"].as_str().unwrap();
        assert!(markdown.contains("PaymentService"));
        assert!(markdown.contains("16 MB (burst up to 64 MB)"));
        assert!(markdown.contains("256 MB"));
    }

    #[test]
    fn test_lsp_definition_jump() {
        let mut server = LanguageServer::new();
        let doc = r#"
service AuthService {
    let mut session_store = Map::new();

    fn verify(token: string) -> bool {
        return true;
    }
}
"#;
        server.handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///auth.vl",
                    "languageId": "vlang",
                    "version": 1,
                    "text": doc
                }
            }
        }).to_string());

        // Definition jump on verify
        let def_req = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": "file:///auth.vl" },
                "position": { "line": 4, "character": 9 }
            }
        });

        let (resp_str, _) = server.handle_message(&def_req.to_string());
        let resp: serde_json::Value = serde_json::from_str(&resp_str.unwrap()).unwrap();
        assert_eq!(resp["result"]["uri"], "file:///auth.vl");
        assert_eq!(resp["result"]["range"]["start"]["line"], 4);
    }

    #[test]
    fn test_lsp_document_formatting() {
        let mut server = LanguageServer::new();
        let unformatted = "service UnformattedService{\nlet x = 10;\nfn handle(){\nreturn x;\n}\n}\n";
        server.handle_message(&json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///unformatted.vl",
                    "languageId": "vlang",
                    "version": 1,
                    "text": unformatted
                }
            }
        }).to_string());

        let fmt_req = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "textDocument/formatting",
            "params": {
                "textDocument": { "uri": "file:///unformatted.vl" }
            }
        });

        let (resp_str, _) = server.handle_message(&fmt_req.to_string());
        let resp: serde_json::Value = serde_json::from_str(&resp_str.unwrap()).unwrap();
        let edits = resp["result"].as_array().unwrap();
        assert_eq!(edits.len(), 1);
        let formatted_text = edits[0]["new_text"].as_str().unwrap();
        assert!(formatted_text.contains("    let x = 10;\n"));
        assert!(formatted_text.contains("        return x;\n"));
    }

    #[test]
    fn test_transport_message_read_and_write() {
        let mut buffer = Vec::new();
        let payload = r#"{"jsonrpc":"2.0","method":"test"}"#;
        Transport::write_message(&mut buffer, payload).unwrap();

        let mut cursor = std::io::Cursor::new(buffer);
        let read_back = Transport::read_message(&mut cursor).unwrap();
        assert_eq!(read_back, Some(payload.to_string()));
    }
}
