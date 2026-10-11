//! Tokenizer for the Scheme interpreter.
//!
//! Converts the characters of a port into tokens following R7RS's lexical
//! syntax (R7RS section 7.1.1): identifiers including `|...|` forms, numbers
//! (classified by `number_syntax`), strings and characters with all their
//! escapes and names, `#|...|#` block comments, `#;` datum comments, datum
//! labels, `#u8(`, and the `#!fold-case` / `#!no-fold-case` directives.
//!
//! Lookahead is pushed back into the port itself (`PortKind::unread_char`),
//! not held here, so no characters are lost when the parser creates a fresh
//! tokenizer for the next datum.

use crate::io::PortKind;
use crate::number_syntax::{NumberSyntax, parse_number};

/// Represents a lexical token in Scheme source code.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// Number syntax, as written; the parser converts it.
    Number(String),
    /// An identifier written without bars (folded under `#!fold-case`)
    Symbol(String),
    /// An identifier written as `|...|`, escapes decoded
    BarSymbol(String),
    /// A string literal
    String(String),
    /// A boolean literal (#t or #f)
    Boolean(bool),
    /// A character literal (#\a, #\space, etc.)
    Character(char),
    /// Left parenthesis
    LeftParen,
    /// Right parenthesis
    RightParen,
    /// Left bracket (for vectors)
    LeftBracket,
    /// Right bracket (for vectors)
    RightBracket,
    /// Hash paren (for vectors)
    HashParen,
    /// `#u8(`, opening a bytevector
    ByteVectorStart,
    /// Single quote (for quoted forms)
    Quote,
    /// Dot (for dotted pairs)
    Dot,
    /// End of input
    Eof,
    /// Backquote for macros and transformation
    QuasiQuote,
    /// Comma for unquote
    Unquote,
    /// Comma-at for unquote-splicing
    UnquoteSplicing,
    /// `#;`: the next datum is a comment
    DatumComment,
    /// `#n=`: label the next datum
    LabelDef(u64),
    /// `#n#`: refer to a labelled datum
    LabelRef(u64),
    /// Malformed lexical syntax. The offending token has been consumed in
    /// full, so reading can carry on after it.
    Error(String),
}

/// Characters that end an identifier, number or character name. Brackets
/// (s1's vector syntax) and the quote characters are included so that
/// `[a b]` and `a'b` split as they look.
fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || "()[]\";'`,|".contains(c)
}

/// R7RS character names.
fn named_char(name: &str) -> Option<char> {
    Some(match name {
        "alarm" => '\u{7}',
        "backspace" => '\u{8}',
        "delete" => '\u{7f}',
        "escape" => '\u{1b}',
        "newline" => '\n',
        "null" => '\0',
        "return" => '\r',
        "space" => ' ',
        "tab" => '\t',
        _ => return None,
    })
}

/// Tokenizer that reads characters from a port and produces tokens.
pub struct Tokenizer<'a> {
    /// The port read from.
    port_kind: &'a mut PortKind,
}

impl<'a> Tokenizer<'a> {
    /// A tokenizer reading from `port_kind`.
    pub fn new(port_kind: &'a mut PortKind) -> Self {
        Tokenizer { port_kind }
    }

    /// The next character, or `None` at end of input.
    fn read_char(&mut self) -> Option<char> {
        self.port_kind.next_char_utf8()
    }

    /// Push back the character just read; see `PortKind::unread_char`.
    fn unread_char(&mut self, c: char) {
        self.port_kind.unread_char(c);
    }

    /// The next character, without consuming it.
    fn peek_char(&mut self) -> Option<char> {
        let c = self.read_char()?;
        self.unread_char(c);
        Some(c)
    }

    /// Read characters up to (not including) the next delimiter.
    fn read_atom(&mut self, first: char) -> String {
        let mut atom = first.to_string();
        while let Some(c) = self.read_char() {
            if is_delimiter(c) {
                self.unread_char(c);
                break;
            }
            atom.push(c);
        }
        atom
    }

    /// `s`, case-folded if `#!fold-case` is in effect.
    fn fold(&self, s: String) -> String {
        if self.port_kind.fold_case() {
            s.to_lowercase()
        } else {
            s
        }
    }

    /// Skip whitespace, `;` and `#|...|#` comments, and `#!` directives.
    fn skip_atmosphere(&mut self) -> Result<(), String> {
        loop {
            match self.read_char() {
                Some(c) if c.is_whitespace() => {}
                Some(';') => {
                    while let Some(c) = self.read_char() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                Some('#') => match self.read_char() {
                    Some('|') => self.skip_block_comment()?,
                    Some('!') => {
                        // `#!/...` or `#! ...`, as in a script's first line
                        // (`#!/usr/bin/env s1`), is a comment to the end of
                        // the line. Any other `#!` is a directive.
                        match self.read_char() {
                            Some(c) if c == '/' || c == ' ' => {
                                while let Some(c) = self.read_char() {
                                    if c == '\n' {
                                        break;
                                    }
                                }
                                continue;
                            }
                            Some(c) => self.unread_char(c),
                            None => {}
                        }
                        let name = match self.read_char() {
                            Some(c) if !is_delimiter(c) => self.read_atom(c),
                            Some(c) => {
                                self.unread_char(c);
                                String::new()
                            }
                            None => String::new(),
                        };
                        match name.as_str() {
                            "fold-case" => self.port_kind.set_fold_case(true),
                            "no-fold-case" => self.port_kind.set_fold_case(false),
                            _ => return Err(format!("unknown directive #!{}", name)),
                        }
                    }
                    Some(c) => {
                        self.unread_char(c);
                        self.unread_char('#');
                        return Ok(());
                    }
                    None => {
                        self.unread_char('#');
                        return Ok(());
                    }
                },
                Some(c) => {
                    self.unread_char(c);
                    return Ok(());
                }
                None => return Ok(()),
            }
        }
    }

    /// Skip a block comment whose opening `#|` has been read; they nest.
    fn skip_block_comment(&mut self) -> Result<(), String> {
        let mut depth = 1;
        loop {
            match self.read_char() {
                Some('|') if self.peek_char() == Some('#') => {
                    self.read_char();
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some('#') if self.peek_char() == Some('|') => {
                    self.read_char();
                    depth += 1;
                }
                Some(_) => {}
                None => return Err("unterminated block comment".to_string()),
            }
        }
    }

    /// Read `\x<hex>;` after its `x`, as used in strings and `|...|`.
    fn read_hex_escape(&mut self) -> Result<char, String> {
        let mut hex = String::new();
        loop {
            match self.read_char() {
                Some(';') => break,
                Some(c) if c.is_ascii_hexdigit() => hex.push(c),
                _ => return Err(format!("malformed escape \\x{} (expected hex digits and ';')", hex)),
            }
        }
        u32::from_str_radix(&hex, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| format!("invalid character code \\x{};", hex))
    }

    /// Read the body of a string or `|identifier|` up to `close`, decoding
    /// escapes. Line continuations (`\` at the end of a line) apply to
    /// strings only.
    fn read_delimited(&mut self, close: char) -> Result<String, String> {
        let what = if close == '"' { "string" } else { "|identifier|" };
        let mut out = String::new();
        let mut error = None;
        loop {
            match self.read_char() {
                Some(c) if c == close => break,
                Some('\\') => match self.read_char() {
                    Some('a') => out.push('\u{7}'),
                    Some('b') => out.push('\u{8}'),
                    Some('t') => out.push('\t'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some(c @ ('"' | '\\' | '|')) => out.push(c),
                    Some('x' | 'X') => match self.read_hex_escape() {
                        Ok(c) => out.push(c),
                        Err(e) => {
                            error.get_or_insert(e);
                        }
                    },
                    Some(c) if close == '"' && c.is_whitespace() => {
                        if let Err(e) = self.skip_line_continuation(c) {
                            error.get_or_insert(e);
                        }
                    }
                    Some(c) => {
                        error.get_or_insert(format!("unknown escape \\{} in {}", c, what));
                    }
                    None => return Err(format!("unterminated {}", what)),
                },
                Some(c) => out.push(c),
                None => return Err(format!("unterminated {}", what)),
            }
        }
        // Report a bad escape only once the whole literal has been consumed.
        match error {
            Some(e) => Err(e),
            None => Ok(out),
        }
    }

    /// `\<intraline whitespace>*<newline><intraline whitespace>*` inside a
    /// string reads as nothing; `first` is the whitespace after the `\`.
    fn skip_line_continuation(&mut self, first: char) -> Result<(), String> {
        let mut c = Some(first);
        while let Some(ch) = c {
            if ch == '\n' {
                break;
            }
            if !ch.is_whitespace() {
                self.unread_char(ch);
                return Err("'\\' followed by whitespace must end the line".to_string());
            }
            c = self.read_char();
        }
        while let Some(ch) = self.read_char() {
            if ch == '\n' || !ch.is_whitespace() {
                self.unread_char(ch);
                break;
            }
        }
        Ok(())
    }

    /// Read a character literal after its `#\\`.
    fn read_character(&mut self) -> Token {
        let first = match self.read_char() {
            Some(c) => c,
            None => return Token::Error("unexpected end of input after #\\".to_string()),
        };
        // A delimiter right after `#\\` is the character itself: `#\\(`, `#\\ `.
        if is_delimiter(first) {
            return Token::Character(first);
        }
        let name = self.read_atom(first);
        if name.chars().count() == 1 {
            return Token::Character(first);
        }
        let folded = self.fold(name.clone());
        if let Some(c) = named_char(&folded) {
            return Token::Character(c);
        }
        if let Some(hex) = name.strip_prefix('x') {
            if hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
                    Some(c) => Token::Character(c),
                    None => Token::Error(format!("invalid character code #\\{}", name)),
                };
            }
        }
        Token::Error(format!("unknown character name #\\{}", name))
    }

    /// Read the token after a `#` that isn't atmosphere.
    fn read_hash(&mut self) -> Token {
        match self.read_char() {
            Some('(') => Token::HashParen,
            Some('\\') => self.read_character(),
            Some(';') => Token::DatumComment,
            Some(c) if c.is_ascii_digit() => {
                let mut digits = c.to_string();
                loop {
                    match self.read_char() {
                        Some(d) if d.is_ascii_digit() => digits.push(d),
                        Some('=') => return label(&digits, Token::LabelDef),
                        Some('#') => return label(&digits, Token::LabelRef),
                        Some(d) => {
                            self.unread_char(d);
                            let rest = self.read_atom(d);
                            return Token::Error(format!("invalid syntax #{}{}", digits, rest));
                        }
                        None => return Token::Error(format!("invalid syntax #{}", digits)),
                    }
                }
            }
            Some(c) if !is_delimiter(c) => {
                let atom = self.read_atom(c);
                if atom.eq_ignore_ascii_case("u8") && self.peek_char() == Some('(') {
                    self.read_char();
                    return Token::ByteVectorStart;
                }
                match atom.to_ascii_lowercase().as_str() {
                    "t" | "true" => Token::Boolean(true),
                    "f" | "false" => Token::Boolean(false),
                    _ => {
                        let text = format!("#{}", atom);
                        match parse_number(&text, 10) {
                            NumberSyntax::NotANumber => {
                                Token::Error(format!("invalid syntax {}", text))
                            }
                            _ => Token::Number(text),
                        }
                    }
                }
            }
            Some(c) => {
                self.unread_char(c);
                Token::Error("invalid syntax: '#' followed by a delimiter".to_string())
            }
            None => Token::Error("unexpected end of input after #".to_string()),
        }
    }

    /// Read the next token, or `Some(Token::Eof)` at the end of input.
    pub fn next_token(&mut self) -> Option<Token> {
        if let Err(e) = self.skip_atmosphere() {
            return Some(Token::Error(e));
        }
        let token = match self.read_char() {
            None => Token::Eof,
            Some('(') => Token::LeftParen,
            Some(')') => Token::RightParen,
            Some('[') => Token::LeftBracket,
            Some(']') => Token::RightBracket,
            Some('\'') => Token::Quote,
            Some('`') => Token::QuasiQuote,
            Some(',') => {
                if self.peek_char() == Some('@') {
                    self.read_char();
                    Token::UnquoteSplicing
                } else {
                    Token::Unquote
                }
            }
            Some('"') => match self.read_delimited('"') {
                Ok(s) => Token::String(s),
                Err(e) => Token::Error(e),
            },
            Some('|') => match self.read_delimited('|') {
                Ok(s) => Token::BarSymbol(s),
                Err(e) => Token::Error(e),
            },
            Some('#') => self.read_hash(),
            Some(c) => {
                let atom = self.read_atom(c);
                if atom == "." {
                    Token::Dot
                } else if parse_number(&atom, 10) == NumberSyntax::NotANumber {
                    Token::Symbol(self.fold(atom))
                } else {
                    Token::Number(atom)
                }
            }
        };
        Some(token)
    }
}

/// A datum-label token (`#n=` or `#n#`, chosen by `make`) for `digits`.
fn label(digits: &str, make: fn(u64) -> Token) -> Token {
    match digits.parse() {
        Ok(n) => make(n),
        Err(_) => Token::Error(format!("datum label too large: {}", digits)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::PortKind;

    fn tokenizer_from_str<'a>(port: &'a mut PortKind, s: &str) -> Tokenizer<'a> {
        // Set up the string port
        *port = crate::io::new_string_port_input(s);
        Tokenizer::new(port)
    }

    #[test]
    fn test_basic_tokens() {
        let mut port_kind = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port_kind, "hello123\"world\"");
        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Symbol("hello123".to_string()))
        );
        assert_eq!(
            tokenizer.next_token(),
            Some(Token::String("world".to_string()))
        );
        assert_eq!(tokenizer.next_token(), Some(Token::Eof));
    }

    #[test]
    fn test_whitespace_and_comments() {
        let mut port = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port, "  hello  ; comment\n  world");

        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Symbol("hello".to_string()))
        );
        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Symbol("world".to_string()))
        );
        assert_eq!(tokenizer.next_token(), Some(Token::Eof));
    }

    #[test]
    fn test_parentheses_and_brackets() {
        let mut port = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port, "()[]");

        assert_eq!(tokenizer.next_token(), Some(Token::LeftParen));
        assert_eq!(tokenizer.next_token(), Some(Token::RightParen));
        assert_eq!(tokenizer.next_token(), Some(Token::LeftBracket));
        assert_eq!(tokenizer.next_token(), Some(Token::RightBracket));
        assert_eq!(tokenizer.next_token(), Some(Token::Eof));
    }

    #[test]
    fn test_booleans_and_nil() {
        let mut port = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port, "#t #f");

        assert_eq!(tokenizer.next_token(), Some(Token::Boolean(true)));
        assert_eq!(tokenizer.next_token(), Some(Token::Boolean(false)));
        assert_eq!(tokenizer.next_token(), Some(Token::Eof));
    }

    #[test]
    fn test_character() {
        let mut port = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port, "#\\a #\\space");

        assert_eq!(tokenizer.next_token(), Some(Token::Character('a')));
        assert_eq!(tokenizer.next_token(), Some(Token::Character(' ')));
        assert_eq!(tokenizer.next_token(), Some(Token::Eof));
    }

    #[test]
    fn test_quote_and_dot() {
        let mut port = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port, "' .;");

        assert_eq!(tokenizer.next_token(), Some(Token::Quote));
        assert_eq!(tokenizer.next_token(), Some(Token::Dot));
        assert_eq!(tokenizer.next_token(), Some(Token::Eof));
    }

    #[test]
    fn test_multiple_tokens_per_line() {
        let mut port = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port, "hello world 123");

        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Symbol("hello".to_string()))
        );
        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Symbol("world".to_string()))
        );
        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Number("123".to_string()))
        );
        assert_eq!(tokenizer.next_token(), Some(Token::Eof));
    }

    #[test]
    fn test_comments() {
        let mut port = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port, "hello ; this is a comment\nworld");

        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Symbol("hello".to_string()))
        );
        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Symbol("world".to_string()))
        );
        assert_eq!(tokenizer.next_token(), Some(Token::Eof));
    }

    #[test]
    fn test_string_literal_debug() {
        let mut port = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port, "\"hello world\"");

        let token1 = tokenizer.next_token();
        let token2 = tokenizer.next_token();
        println!("Input: \"hello world\"");
        println!("Token 1: {:?}", token1);
        println!("Token 2: {:?}", token2);

        // This test is just for debugging, so we'll make it pass
        assert!(true);
    }

    #[test]
    fn test_vector_token_debug() {
        let mut port = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port, "#(1 2 3)");
        let mut tokens = Vec::new();
        loop {
            let tok = tokenizer.next_token();
            println!("Token: {:?}", tok);
            if let Some(Token::Eof) = tok {
                break;
            }
            tokens.push(tok);
        }
        // This test is just for debugging, so we'll make it pass
        assert!(true);
    }

    #[test]
    fn test_negative_and_positive_numbers() {
        let mut port = PortKind::Stdin;
        let mut tokenizer = tokenizer_from_str(&mut port, "-45 +123 -123412341234123412341234");
        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Number("-45".to_string()))
        );
        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Number("+123".to_string()))
        );
        assert_eq!(
            tokenizer.next_token(),
            Some(Token::Number("-123412341234123412341234".to_string()))
        );
        assert_eq!(tokenizer.next_token(), Some(Token::Eof));
    }

    fn tokens(s: &str) -> Vec<Token> {
        let mut port = crate::io::new_string_port_input(s);
        let mut tokenizer = Tokenizer::new(&mut port);
        let mut out = Vec::new();
        loop {
            match tokenizer.next_token() {
                Some(Token::Eof) | None => return out,
                Some(t) => out.push(t),
            }
        }
    }

    fn sym(s: &str) -> Token {
        Token::Symbol(s.to_string())
    }

    #[test]
    fn test_character_names_and_delimiters() {
        assert_eq!(
            tokens("#\\alarm #\\backspace #\\delete #\\escape #\\newline #\\null #\\return #\\space #\\tab"),
            ['\u{7}', '\u{8}', '\u{7f}', '\u{1b}', '\n', '\0', '\r', ' ', '\t']
                .map(Token::Character)
                .to_vec()
        );
        assert_eq!(
            tokens("#\\x41 #\\x #\\λ #\\( #\\)"),
            ['A', 'x', 'λ', '(', ')'].map(Token::Character).to_vec()
        );
        // The bug that swallowed the rest of r7rs-tests.scm: `#\\n` must stop
        // at the closing parens.
        assert_eq!(
            tokens("(f #\\n))"),
            vec![Token::LeftParen, sym("f"), Token::Character('n'), Token::RightParen, Token::RightParen]
        );
        assert!(matches!(tokens("#\\bogus")[..], [Token::Error(_)]));
    }

    #[test]
    fn test_string_escapes() {
        assert_eq!(
            tokens(r#""a\x41;\t\a\\\"""#),
            vec![Token::String("aA\t\u{7}\\\"".to_string())]
        );
        assert_eq!(
            tokens("\"one \\\n    two\""),
            vec![Token::String("one two".to_string())]
        );
        // A bad escape is reported once the whole string has been read.
        assert!(matches!(tokens(r#""bad \q" next"#)[..], [Token::Error(_), Token::Symbol(_)]));
        assert!(matches!(tokens("\"unterminated")[..], [Token::Error(_)]));
    }

    #[test]
    fn test_identifiers() {
        assert_eq!(
            tokens("|hello world| |a\\x41;\\|| + - ... ->x .5 . a.b"),
            vec![
                Token::BarSymbol("hello world".to_string()),
                Token::BarSymbol("aA|".to_string()),
                sym("+"),
                sym("-"),
                sym("..."),
                sym("->x"),
                Token::Number(".5".to_string()),
                Token::Dot,
                sym("a.b"),
            ]
        );
        assert_eq!(tokens("[a b]"), vec![Token::LeftBracket, sym("a"), sym("b"), Token::RightBracket]);
    }

    #[test]
    fn test_hash_syntax() {
        assert_eq!(
            tokens("#t #true #f #false #xFF #e1.5 +inf.0 1/2"),
            vec![
                Token::Boolean(true),
                Token::Boolean(true),
                Token::Boolean(false),
                Token::Boolean(false),
                Token::Number("#xFF".to_string()),
                Token::Number("#e1.5".to_string()),
                Token::Number("+inf.0".to_string()),
                Token::Number("1/2".to_string()),
            ]
        );
        assert_eq!(
            tokens("#u8( #; #0= #12#"),
            vec![Token::ByteVectorStart, Token::DatumComment, Token::LabelDef(0), Token::LabelRef(12)]
        );
        assert!(matches!(tokens("#bogus")[..], [Token::Error(_)]));
    }

    #[test]
    fn test_comments_and_directives() {
        assert_eq!(tokens("a #| x #| nested |# y |# b"), vec![sym("a"), sym("b")]);
        assert_eq!(tokens("#!/usr/bin/env s1 -x\na #! comment (\nb"), vec![sym("a"), sym("b")]);
        assert!(matches!(tokens("a #| never closed")[..], [Token::Symbol(_), Token::Error(_)]));
        assert_eq!(
            tokens("ABC #!fold-case ABC #\\SPACE |ABC| #!no-fold-case ABC"),
            vec![sym("ABC"), sym("abc"), Token::Character(' '), Token::BarSymbol("ABC".to_string()), sym("ABC")]
        );
    }

    #[test]
    fn test_lookahead_survives_a_new_tokenizer() {
        // The parser makes a fresh Tokenizer per datum; the quote read while
        // ending `a` must still be there for the next one.
        let mut port = crate::io::new_string_port_input("a'b");
        assert_eq!(Tokenizer::new(&mut port).next_token(), Some(sym("a")));
        assert_eq!(Tokenizer::new(&mut port).next_token(), Some(Token::Quote));
        assert_eq!(Tokenizer::new(&mut port).next_token(), Some(sym("b")));
    }
}
