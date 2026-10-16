use crate::error::SyntaxError;
use crate::span::Span;
use crate::token::TokenKind;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub struct Lexer<'a> {
    source: &'a str,
    chars: Vec<(usize, char)>,
    cursor: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        let chars: Vec<(usize, char)> = source.char_indices().collect();
        Self {
            source,
            chars,
            cursor: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.cursor).map(|(_, c)| *c)
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.cursor + 1).map(|(_, c)| *c)
    }

    fn current_byte_pos(&self) -> usize {
        self.chars.get(self.cursor).map(|(pos, _)| *pos).unwrap_or(self.source.len())
    }

    fn advance(&mut self) -> Option<char> {
        if let Some((_, c)) = self.chars.get(self.cursor) {
            let c = *c;
            self.cursor += 1;
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            Some(c)
        } else {
            None
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, SyntaxError> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace_and_comments()?;

            let start_pos = self.current_byte_pos();
            let start_line = self.line;
            let start_col = self.col;

            let Some(c) = self.peek() else {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    span: Span::new(start_pos, start_pos, start_line, start_col),
                });
                break;
            };

            let kind = match c {
                '(' => { self.advance(); TokenKind::OpenParen }
                ')' => { self.advance(); TokenKind::CloseParen }
                '{' => { self.advance(); TokenKind::OpenBrace }
                '}' => { self.advance(); TokenKind::CloseBrace }
                '[' => { self.advance(); TokenKind::OpenBracket }
                ']' => { self.advance(); TokenKind::CloseBracket }
                ';' => { self.advance(); TokenKind::Semicolon }
                ',' => { self.advance(); TokenKind::Comma }
                '+' => { self.advance(); TokenKind::Plus }
                '*' => { self.advance(); TokenKind::Star }
                '%' => { self.advance(); TokenKind::Percent }
                '-' => {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        TokenKind::Arrow
                    } else {
                        TokenKind::Minus
                    }
                }
                '=' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::EqEq
                    } else {
                        TokenKind::Eq
                    }
                }
                '!' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::BangEq
                    } else {
                        TokenKind::Bang
                    }
                }
                '<' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::LtEq
                    } else {
                        TokenKind::Lt
                    }
                }
                '>' => {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        TokenKind::GtEq
                    } else {
                        TokenKind::Gt
                    }
                }
                '&' => {
                    self.advance();
                    if self.peek() == Some('&') {
                        self.advance();
                        TokenKind::AmpAmp
                    } else {
                        return Err(SyntaxError::new("Unexpected single '&', did you mean '&&'?", Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                    }
                }
                '|' => {
                    self.advance();
                    if self.peek() == Some('|') {
                        self.advance();
                        TokenKind::PipePipe
                    } else {
                        return Err(SyntaxError::new("Unexpected single '|', did you mean '||'?", Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                    }
                }
                ':' => {
                    self.advance();
                    if self.peek() == Some(':') {
                        self.advance();
                        TokenKind::ColonColon
                    } else {
                        TokenKind::Colon
                    }
                }
                '.' => {
                    self.advance();
                    TokenKind::Dot
                }
                '?' => {
                    self.advance();
                    if self.peek() == Some('.') {
                        self.advance();
                        TokenKind::QuestionDot
                    } else {
                        TokenKind::Question
                    }
                }
                '"' => self.lex_string(start_pos, start_line, start_col)?,
                '/' => {
                    self.advance();
                    TokenKind::Slash
                }
                '0'..='9' => self.lex_number(start_pos, start_line, start_col)?,
                'a'..='z' | 'A'..='Z' | '_' => self.lex_ident(start_pos)?,
                unknown => {
                    return Err(SyntaxError::new(
                        format!("Unexpected character '{}'", unknown),
                        Span::new(start_pos, start_pos + unknown.len_utf8(), start_line, start_col),
                    ));
                }
            };

            let end_pos = self.current_byte_pos();
            tokens.push(Token {
                kind,
                span: Span::new(start_pos, end_pos, start_line, start_col),
            });
        }

        Ok(tokens)
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), SyntaxError> {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.advance();
                }
                Some('/') => {
                    if self.peek_next() == Some('/') {
                        // Single-line comment
                        self.advance();
                        self.advance();
                        while let Some(c) = self.peek() {
                            self.advance();
                            if c == '\n' {
                                break;
                            }
                        }
                    } else if self.peek_next() == Some('*') {
                        // Block comment
                        let start_pos = self.current_byte_pos();
                        let start_line = self.line;
                        let start_col = self.col;
                        self.advance();
                        self.advance();
                        let mut closed = false;
                        while let Some(c) = self.advance() {
                            if c == '*' && self.peek() == Some('/') {
                                self.advance();
                                closed = true;
                                break;
                            }
                        }
                        if !closed {
                            return Err(SyntaxError::new(
                                "Unterminated block comment",
                                Span::new(start_pos, self.current_byte_pos(), start_line, start_col),
                            ));
                        }
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn lex_ident(&mut self, start_pos: usize) -> Result<TokenKind, SyntaxError> {
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                self.advance();
            } else {
                break;
            }
        }
        let end_pos = self.current_byte_pos();
        let word = &self.source[start_pos..end_pos];

        let kind = match word {
            "service" => TokenKind::Service,
            "fn" => TokenKind::Fn,
            "let" => TokenKind::Let,
            "mut" => TokenKind::Mut,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "try" => TokenKind::Try,
            "catch" => TokenKind::Catch,
            "defer" => TokenKind::Defer,
            "persist" => TokenKind::Persist,
            "memory_budget" => TokenKind::MemoryBudget,
            "request" => TokenKind::Request,
            "persistent" => TokenKind::Persistent,
            "max" => TokenKind::Max,
            "import" => TokenKind::Import,
            "as" => TokenKind::As,
            "test" => TokenKind::Test,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "null" => TokenKind::Null,
            // Memory units
            "B" | "KB" | "MB" | "GB" => TokenKind::ByteUnit(word.to_string()),
            _ => TokenKind::Ident(word.to_string()),
        };

        Ok(kind)
    }

    fn lex_number(&mut self, start_pos: usize, start_line: usize, start_col: usize) -> Result<TokenKind, SyntaxError> {
        let mut is_float = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.advance();
            } else if c == '.' && self.peek_next().is_some_and(|next| next.is_ascii_digit()) {
                is_float = true;
                self.advance();
            } else {
                break;
            }
        }

        let end_pos = self.current_byte_pos();
        let raw = &self.source[start_pos..end_pos];

        if is_float {
            match raw.parse::<f64>() {
                Ok(fl) => Ok(TokenKind::FloatLit(fl)),
                Err(_) => Err(SyntaxError::new(
                    format!("Invalid float literal '{}'", raw),
                    Span::new(start_pos, end_pos, start_line, start_col),
                )),
            }
        } else {
            match raw.parse::<i64>() {
                Ok(n) => Ok(TokenKind::IntLit(n)),
                Err(_) => Err(SyntaxError::new(
                    format!("Invalid integer literal '{}'", raw),
                    Span::new(start_pos, end_pos, start_line, start_col),
                )),
            }
        }
    }

    fn lex_string(&mut self, start_pos: usize, start_line: usize, start_col: usize) -> Result<TokenKind, SyntaxError> {
        self.advance(); // consume opening quote '"'
        let mut content = String::new();

        while let Some(c) = self.advance() {
            match c {
                '"' => return Ok(TokenKind::StringLit(content)),
                '\\' => {
                    let esc = self.advance().ok_or_else(|| {
                        SyntaxError::new("Unterminated escape sequence in string", Span::new(start_pos, self.current_byte_pos(), start_line, start_col))
                    })?;
                    match esc {
                        'n' => content.push('\n'),
                        't' => content.push('\t'),
                        'r' => content.push('\r'),
                        '\\' => content.push('\\'),
                        '"' => content.push('"'),
                        '0' => content.push('\0'),
                        other => {
                            return Err(SyntaxError::new(
                                format!("Invalid escape character '\\{}'", other),
                                Span::new(start_pos, self.current_byte_pos(), start_line, start_col),
                            ));
                        }
                    }
                }
                '\n' => {
                    return Err(SyntaxError::new(
                        "Unclosed string literal across newline",
                        Span::new(start_pos, self.current_byte_pos(), start_line, start_col),
                    ));
                }
                ch => content.push(ch),
            }
        }

        Err(SyntaxError::new(
            "Unclosed string literal at end of file",
            Span::new(start_pos, self.current_byte_pos(), start_line, start_col),
        ))
    }
}
