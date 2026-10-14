use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, col: usize) -> Self {
        Self { start, end, line, col }
    }

    pub fn merge(self, other: Span) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line.min(other.line),
            col: if self.start <= other.start { self.col } else { other.col },
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}
