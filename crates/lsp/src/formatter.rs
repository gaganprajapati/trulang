use crate::protocol::{Position, Range, TextEdit};

pub struct Formatter;

impl Formatter {
    /// Formats a complete TruLang source string using standard 4-space indentation rules
    pub fn format(content: &str) -> String {
        let mut result = String::with_capacity(content.len());
        let mut indent_level = 0;
        let mut in_multiline_comment = false;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        for (idx, raw_line) in lines.iter().enumerate() {
            let trimmed = raw_line.trim();

            if trimmed.is_empty() {
                // Collapse excessive blank lines
                if !result.ends_with("\n\n") && idx > 0 && idx < total_lines - 1 {
                    result.push('\n');
                }
                continue;
            }

            // Check comment bounds
            if trimmed.starts_with("/*") {
                in_multiline_comment = true;
            }
            if in_multiline_comment {
                result.push_str(raw_line);
                result.push('\n');
                if trimmed.ends_with("*/") {
                    in_multiline_comment = false;
                }
                continue;
            }

            // Decrease indent before printing line if line starts with closing brace
            let starts_with_close = trimmed.starts_with('}') || trimmed.starts_with(')');
            if starts_with_close && indent_level > 0 {
                indent_level -= 1;
            }

            // Add standard 4-space indentation
            let indent_spaces = "    ".repeat(indent_level);
            result.push_str(&indent_spaces);
            result.push_str(trimmed);
            result.push('\n');

            // Count open vs close braces on this line to update subsequent indentation
            let open_braces = trimmed.chars().filter(|&c| c == '{').count();
            let close_braces = trimmed.chars().filter(|&c| c == '}').count();

            if starts_with_close {
                // We already subtracted one indent
                indent_level += open_braces;
                if close_braces > 1 {
                    indent_level = indent_level.saturating_sub(close_braces - 1);
                }
            } else {
                indent_level += open_braces;
                indent_level = indent_level.saturating_sub(close_braces);
            }
        }

        result
    }

    /// Creates an LSP TextEdit that replaces the full document with the formatted content
    pub fn format_document(content: &str) -> Vec<TextEdit> {
        let formatted = Self::format(content);
        if formatted == content {
            return Vec::new();
        }

        let line_count = content.lines().count() as u32;
        let last_line_len = content.lines().last().map_or(0, |l| l.len()) as u32;

        vec![TextEdit {
            range: Range::new(
                Position::new(0, 0),
                Position::new(line_count, last_line_len),
            ),
            new_text: formatted,
        }]
    }
}
