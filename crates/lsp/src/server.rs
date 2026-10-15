use crate::analysis::Analyzer;
use crate::formatter::Formatter;
use crate::protocol::*;
use crate::transport::Transport;
use serde_json::json;
use std::collections::HashMap;
use std::io::{BufRead, Write};

pub struct LanguageServer {
    documents: HashMap<String, String>,
    shutdown_requested: bool,
}

impl Default for LanguageServer {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageServer {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            shutdown_requested: false,
        }
    }

    /// Primary event loop for stdio or stream-based LSP execution
    pub fn run<R: BufRead, W: Write>(&mut self, mut reader: R, mut writer: W) -> std::io::Result<()> {
        while let Some(msg_text) = Transport::read_message(&mut reader)? {
            let (response, notifications) = self.handle_message(&msg_text);

            for notif in notifications {
                Transport::write_message(&mut writer, &notif)?;
            }

            if let Some(resp) = response {
                Transport::write_message(&mut writer, &resp)?;
            }

            if self.shutdown_requested {
                break;
            }
        }
        Ok(())
    }

    /// Handles an incoming JSON-RPC string message and produces an optional response and notifications
    pub fn handle_message(&mut self, text: &str) -> (Option<String>, Vec<String>) {
        let request: JsonRpcRequest = match serde_json::from_str(text) {
            Ok(req) => req,
            Err(e) => {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    }
                });
                return (Some(err_resp.to_string()), Vec::new());
            }
        };

        match request.method.as_str() {
            "initialize" => {
                let result = InitializeResult {
                    capabilities: ServerCapabilities {
                        text_document_sync: 1, // Full document sync
                        hover_provider: true,
                        definition_provider: true,
                        document_formatting_provider: true,
                    },
                };
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": request.id,
                    "result": result
                });
                (Some(resp.to_string()), Vec::new())
            }
            "initialized" => (None, Vec::new()),
            "shutdown" => {
                self.shutdown_requested = true;
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": request.id,
                    "result": null
                });
                (Some(resp.to_string()), Vec::new())
            }
            "exit" => (None, Vec::new()),
            "textDocument/didOpen" => {
                let mut notifications = Vec::new();
                if let Some(params_val) = request.params {
                    if let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(params_val) {
                        let uri = params.text_document.uri;
                        let text = params.text_document.text;
                        let diagnostics = Analyzer::validate(&text);
                        self.documents.insert(uri.clone(), text);

                        let publish = json!({
                            "jsonrpc": "2.0",
                            "method": "textDocument/publishDiagnostics",
                            "params": {
                                "uri": uri,
                                "diagnostics": diagnostics
                            }
                        });
                        notifications.push(publish.to_string());
                    }
                }
                (None, notifications)
            }
            "textDocument/didChange" => {
                let mut notifications = Vec::new();
                if let Some(params_val) = request.params {
                    if let Ok(params) = serde_json::from_value::<DidChangeTextDocumentParams>(params_val) {
                        let uri = params.text_document.uri;
                        if let Some(change) = params.content_changes.into_iter().last() {
                            let diagnostics = Analyzer::validate(&change.text);
                            self.documents.insert(uri.clone(), change.text);

                            let publish = json!({
                                "jsonrpc": "2.0",
                                "method": "textDocument/publishDiagnostics",
                                "params": {
                                "uri": uri,
                                "diagnostics": diagnostics
                            }
                            });
                            notifications.push(publish.to_string());
                        }
                    }
                }
                (None, notifications)
            }
            "textDocument/didClose" => {
                if let Some(params_val) = request.params {
                    if let Some(uri) = params_val.get("textDocument").and_then(|t| t.get("uri")).and_then(|u| u.as_str()) {
                        self.documents.remove(uri);
                    }
                }
                (None, Vec::new())
            }
            "textDocument/hover" => {
                let mut hover_result: Option<Hover> = None;
                if let Some(params_val) = request.params {
                    if let Ok(params) = serde_json::from_value::<TextDocumentPositionParams>(params_val) {
                        if let Some(doc) = self.documents.get(&params.text_document.uri) {
                            hover_result = Analyzer::hover(doc, params.position);
                        }
                    }
                }
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": request.id,
                    "result": hover_result
                });
                (Some(resp.to_string()), Vec::new())
            }
            "textDocument/definition" => {
                let mut loc_result: Option<Location> = None;
                if let Some(params_val) = request.params {
                    if let Ok(params) = serde_json::from_value::<TextDocumentPositionParams>(params_val) {
                        if let Some(doc) = self.documents.get(&params.text_document.uri) {
                            loc_result = Analyzer::definition(doc, &params.text_document.uri, params.position);
                        }
                    }
                }
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": request.id,
                    "result": loc_result
                });
                (Some(resp.to_string()), Vec::new())
            }
            "textDocument/formatting" => {
                let mut edits: Vec<TextEdit> = Vec::new();
                if let Some(params_val) = request.params {
                    if let Ok(params) = serde_json::from_value::<DocumentFormattingParams>(params_val) {
                        if let Some(doc) = self.documents.get(&params.text_document.uri) {
                            edits = Formatter::format_document(doc);
                        }
                    }
                }
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": request.id,
                    "result": edits
                });
                (Some(resp.to_string()), Vec::new())
            }
            _ => {
                // If it's a request (has id), return MethodNotFound (-32601)
                if request.id.is_some() {
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": request.id,
                        "error": {
                            "code": -32601,
                            "message": format!("Method not found: {}", request.method)
                        }
                    });
                    (Some(resp.to_string()), Vec::new())
                } else {
                    (None, Vec::new())
                }
            }
        }
    }
}
