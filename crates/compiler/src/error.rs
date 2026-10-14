use std::fmt;
use vlang_syntax::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct CompileError {
    pub message: String,
    pub span: Span,
    pub hint: Option<String>,
}

impl CompileError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            hint: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn format_with_source(&self, source: &str, file_name: &str) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let line_idx = if self.span.line > 0 { self.span.line - 1 } else { 0 };
        let line_content = lines.get(line_idx).unwrap_or(&"");

        let pointer_padding = " ".repeat(if self.span.col > 1 { self.span.col - 1 } else { 0 });
        let underline_len = if self.span.end > self.span.start {
            (self.span.end - self.span.start).max(1).min(30)
        } else {
            1
        };

        let mut output = format!(
            "--> {}:{}:{}\n  |\n{:<3} | {}\n  | {}{}\n  = compile error: {}",
            file_name,
            self.span.line,
            self.span.col,
            self.span.line,
            line_content,
            pointer_padding,
            "^".repeat(underline_len),
            self.message
        );

        if let Some(hint) = &self.hint {
            output.push_str(&format!("\n  = hint: {}", hint));
        }

        output
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Compile error at {}: {}", self.span, self.message)
    }
}

impl std::error::Error for CompileError {}
