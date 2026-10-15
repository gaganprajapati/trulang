use crate::span::Span;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxError {
    pub message: String,
    pub span: Span,
}

impl SyntaxError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    pub fn format_with_source(&self, source: &str, file_name: &str) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let line_idx = if self.span.line > 0 { self.span.line - 1 } else { 0 };
        let line_content = lines.get(line_idx).unwrap_or(&"");
        
        let pointer_padding = " ".repeat(self.span.col.saturating_sub(1));
        
        format!(
            "--> {}:{}:{}\n  |\n{:<3} | {}\n  | {}^{}\n  = syntax error: {}",
            file_name,
            self.span.line,
            self.span.col,
            self.span.line,
            line_content,
            pointer_padding,
            if self.span.end > self.span.start && self.span.end - self.span.start > 1 {
                "^".repeat((self.span.end - self.span.start - 1).min(30))
            } else {
                "".to_string()
            },
            self.message
        )
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Syntax error at {}: {}", self.span, self.message)
    }
}

impl std::error::Error for SyntaxError {}
