use crate::protocol::*;
use crate::session::DebugSession;
use std::io::{BufRead, Write};
use trulang_lsp::transport::Transport;

pub struct DapServer {
    session: DebugSession,
}

impl Default for DapServer {
    fn default() -> Self {
        Self::new()
    }
}

impl DapServer {
    pub fn new() -> Self {
        Self {
            session: DebugSession::new(),
        }
    }

    /// Run the DAP server loop over any buffered reader and writer (e.g. stdio)
    pub fn run<R: BufRead, W: Write>(
        &mut self,
        reader: &mut R,
        writer: &mut W,
    ) -> anyhow::Result<()> {
        loop {
            let msg_text = match Transport::read_message(reader)? {
                Some(text) => text,
                None => break, // EOF reached
            };

            let req: DapRequest = match serde_json::from_str(&msg_text) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Failed to parse DAP request JSON: {}", e);
                    continue;
                }
            };

            let is_disconnect = req.command == "disconnect";
            let (resp, events) = self.session.handle_request(req);

            let resp_json = serde_json::to_string(&resp)?;
            Transport::write_message(writer, &resp_json)?;

            for event in events {
                let event_json = serde_json::to_string(&event)?;
                Transport::write_message(writer, &event_json)?;
            }

            if is_disconnect {
                break;
            }
        }

        Ok(())
    }
}
