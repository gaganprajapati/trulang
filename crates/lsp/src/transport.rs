use std::io::{self, BufRead, Write};

pub struct Transport;

impl Transport {
    /// Reads a single JSON-RPC message from a buffered reader following the LSP spec:
    /// Headers (e.g. Content-Length: 123\r\n\r\n) followed by raw UTF-8 JSON bytes.
    pub fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
        let mut content_length: Option<usize> = None;
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line)?;
            if bytes_read == 0 {
                // EOF reached
                return Ok(None);
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                // Empty line marks end of headers
                break;
            }

            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                let len_str = rest.trim();
                let len = len_str
                    .parse::<usize>()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                content_length = Some(len);
            }
        }

        let length = match content_length {
            Some(len) => len,
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Missing Content-Length header in JSON-RPC message",
                ));
            }
        };

        let mut buffer = vec![0u8; length];
        reader.read_exact(&mut buffer)?;

        let text = String::from_utf8(buffer)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(Some(text))
    }

    /// Writes a JSON-RPC payload to writer with Content-Length header framing
    pub fn write_message<W: Write>(writer: &mut W, json_payload: &str) -> io::Result<()> {
        let length = json_payload.len();
        write!(writer, "Content-Length: {}\r\n\r\n{}", length, json_payload)?;
        writer.flush()?;
        Ok(())
    }
}
