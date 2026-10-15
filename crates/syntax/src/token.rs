use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Service,
    Fn,
    Let,
    Mut,
    Return,
    If,
    Else,
    While,
    For,
    In,
    Try,
    Catch,
    Defer,
    Persist,
    MemoryBudget,
    Request,
    Persistent,
    Max,
    True,
    False,
    Null,

    // Identifiers and Literals
    Ident(String),
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),

    // Symbols & Operators
    Plus,          // +
    Minus,         // -
    Star,          // *
    Slash,         // /
    Percent,       // %
    Bang,          // !
    Eq,            // =
    EqEq,          // ==
    BangEq,        // !=
    Lt,            // <
    LtEq,          // <=
    Gt,            // >
    GtEq,          // >=
    AmpAmp,        // &&
    PipePipe,      // ||
    Arrow,         // ->
    QuestionDot,   // ?.
    Question,      // ?
    ColonColon,    // ::
    Colon,         // :
    Semicolon,     // ;
    Comma,         // ,
    Dot,           // .
    OpenParen,     // (
    CloseParen,    // )
    OpenBrace,     // {
    CloseBrace,    // }
    OpenBracket,   // [
    CloseBracket,  // ]

    // Memory unit suffixes for budget literals (e.g. 16 MB)
    ByteUnit(String), // B, KB, MB, GB

    Eof,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Service => write!(f, "service"),
            TokenKind::Fn => write!(f, "fn"),
            TokenKind::Let => write!(f, "let"),
            TokenKind::Mut => write!(f, "mut"),
            TokenKind::Return => write!(f, "return"),
            TokenKind::If => write!(f, "if"),
            TokenKind::Else => write!(f, "else"),
            TokenKind::While => write!(f, "while"),
            TokenKind::For => write!(f, "for"),
            TokenKind::In => write!(f, "in"),
            TokenKind::Try => write!(f, "try"),
            TokenKind::Catch => write!(f, "catch"),
            TokenKind::Defer => write!(f, "defer"),
            TokenKind::Persist => write!(f, "persist"),
            TokenKind::MemoryBudget => write!(f, "memory_budget"),
            TokenKind::Request => write!(f, "request"),
            TokenKind::Persistent => write!(f, "persistent"),
            TokenKind::Max => write!(f, "max"),
            TokenKind::True => write!(f, "true"),
            TokenKind::False => write!(f, "false"),
            TokenKind::Null => write!(f, "null"),
            TokenKind::Ident(s) => write!(f, "identifier '{}'", s),
            TokenKind::IntLit(n) => write!(f, "{}", n),
            TokenKind::FloatLit(fl) => write!(f, "{}", fl),
            TokenKind::StringLit(s) => write!(f, "\"{}\"", s),
            TokenKind::Plus => write!(f, "+"),
            TokenKind::Minus => write!(f, "-"),
            TokenKind::Star => write!(f, "*"),
            TokenKind::Slash => write!(f, "/"),
            TokenKind::Percent => write!(f, "%"),
            TokenKind::Bang => write!(f, "!"),
            TokenKind::Eq => write!(f, "="),
            TokenKind::EqEq => write!(f, "=="),
            TokenKind::BangEq => write!(f, "!="),
            TokenKind::Lt => write!(f, "<"),
            TokenKind::LtEq => write!(f, "<="),
            TokenKind::Gt => write!(f, ">"),
            TokenKind::GtEq => write!(f, ">="),
            TokenKind::AmpAmp => write!(f, "&&"),
            TokenKind::PipePipe => write!(f, "||"),
            TokenKind::Arrow => write!(f, "->"),
            TokenKind::QuestionDot => write!(f, "?."),
            TokenKind::Question => write!(f, "?"),
            TokenKind::ColonColon => write!(f, "::"),
            TokenKind::Colon => write!(f, ":"),
            TokenKind::Semicolon => write!(f, ";"),
            TokenKind::Comma => write!(f, ","),
            TokenKind::Dot => write!(f, "."),
            TokenKind::OpenParen => write!(f, "("),
            TokenKind::CloseParen => write!(f, ")"),
            TokenKind::OpenBrace => write!(f, "{{"),
            TokenKind::CloseBrace => write!(f, "}}"),
            TokenKind::OpenBracket => write!(f, "["),
            TokenKind::CloseBracket => write!(f, "]"),
            TokenKind::ByteUnit(u) => write!(f, "{}", u),
            TokenKind::Eof => write!(f, "end of file"),
        }
    }
}
