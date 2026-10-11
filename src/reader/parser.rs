//! Parser for Scheme s-expressions.
//!
//! Forked from s1/src/parser.rs at bc6f118, adapted to build `Value`s.
//!
//! Turns the tokens of a `Tokenizer` into data on the heap, one datum per
//! `parse` call. Besides lists, vectors and the quote abbreviations it
//! handles `#;` datum comments and `#n=` / `#n#` datum labels, which may make
//! a datum refer to itself.
//!
//! Errors come in two kinds. Structural ones (an unexpected `)`, a list left
//! open at end of input) stop parsing at once. Errors in a complete token
//! (an unknown character name, a bad escape, `1/0`, a bytevector element
//! out of range) are remembered while the rest of the top-level datum is
//! read, then reported. That leaves the port positioned after the datum, so
//! a bad literal deep in a list doesn't make the reader misread the rest of
//! the list as new top-level forms.

use crate::heap::{GcRef, Heap, Obj};
use crate::io::PortKind;
use crate::reader::number_syntax::{Number, NumberSyntax, parse_number};
use crate::reader::tokenizer::{Token, Tokenizer};
use crate::value::Value;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

/// Why `parse` returned no datum.
#[derive(Debug, PartialEq)]
pub enum ParseError {
    /// The port is at end of input.
    Eof,
    /// The input is not a valid datum; the message says why.
    Syntax(String),
}

/// Parse a single datum from `port_ref`.
pub fn parse(heap: &mut Heap, port_ref: &mut PortKind) -> Result<Value, ParseError> {
    let mut reader = Reader {
        tokens: Tokenizer::new(port_ref),
        heap,
        labels: HashMap::default(),
        deferred: None,
    };
    let token = reader.next_datum_token()?;
    let datum = reader.datum(token)?;
    match reader.deferred {
        Some(msg) => Err(ParseError::Syntax(msg)),
        None => Ok(datum),
    }
}

/// The state of reading one top-level datum.
struct Reader<'a, 'b> {
    /// The token source.
    tokens: Tokenizer<'a>,
    /// Where the datum is built.
    heap: &'b mut Heap,
    /// Datum labels defined so far in this top-level datum.
    labels: HashMap<u64, Value>,
    /// The first error found in a complete token; see the module docs.
    deferred: Option<String>,
}

impl Reader<'_, '_> {
    /// Remember `msg` (if it's the first error) and stand in a placeholder
    /// value so reading can continue to the end of the datum.
    fn defer(&mut self, msg: String) -> Value {
        self.deferred.get_or_insert(msg);
        Value::UNSPECIFIED
    }

    /// The next token that can start a datum, after skipping `#;` comments.
    fn next_datum_token(&mut self) -> Result<Token, ParseError> {
        loop {
            match self.tokens.next_token() {
                Some(Token::DatumComment) => {
                    let commented = self.next_datum_token()?;
                    self.datum(commented)?;
                }
                Some(token) => return Ok(token),
                None => return Ok(Token::Eof),
            }
        }
    }

    /// The datum that starts with `token`.
    fn datum(&mut self, token: Token) -> Result<Value, ParseError> {
        Ok(match token {
            Token::Eof => return Err(ParseError::Eof),
            Token::Number(s) => match parse_number(&s, 10) {
                NumberSyntax::Value(Number::Int(i)) => self.heap.integer(i),
                NumberSyntax::Value(Number::Float(f)) => Value::flonum(f),
                NumberSyntax::Value(Number::Rational(r)) => self.heap.rational(r),
                NumberSyntax::Error(msg) => self.defer(msg),
                NumberSyntax::NotANumber => self.defer(format!("invalid number {}", s)),
            },
            Token::String(s) => self.heap.string(&s),
            Token::Boolean(b) => Value::bool(b),
            Token::Character(c) => Value::char(c),
            Token::Symbol(s) | Token::BarSymbol(s) => self.heap.intern(&s),
            Token::LeftParen => self.list(Token::RightParen)?,
            Token::LeftBracket => {
                let elems = self.sequence(Token::RightBracket, "vector")?;
                self.heap.vector(elems)
            }
            Token::HashParen => {
                let elems = self.sequence(Token::RightParen, "vector")?;
                self.heap.vector(elems)
            }
            Token::ByteVectorStart => {
                let elems = self.sequence(Token::RightParen, "bytevector")?;
                let mut bytes = Vec::with_capacity(elems.len());
                for e in elems {
                    match e.as_fixnum().and_then(|n| u8::try_from(n).ok()) {
                        Some(b) => bytes.push(b),
                        None => {
                            return Ok(self.defer(format!(
                                "bytevector elements must be exact integers 0-255, not {}",
                                crate::printer::print_value(e)
                            )));
                        }
                    }
                }
                self.heap.bytevector(bytes)
            }
            Token::Quote => self.abbreviation("quote")?,
            Token::QuasiQuote => self.abbreviation("quasiquote")?,
            Token::Unquote => self.abbreviation("unquote")?,
            Token::UnquoteSplicing => self.abbreviation("unquote-splicing")?,
            Token::LabelDef(n) => self.labelled(n)?,
            Token::LabelRef(n) => match self.labels.get(&n) {
                Some(datum) => *datum,
                None => self.defer(format!("undefined datum label #{}#", n)),
            },
            Token::Error(msg) => self.defer(msg),
            Token::RightParen => return Err(ParseError::Syntax("Unexpected ')'".to_string())),
            Token::RightBracket => return Err(ParseError::Syntax("Unexpected ']'".to_string())),
            Token::Dot => return Err(ParseError::Syntax("Unexpected '.'".to_string())),
            Token::DatumComment => {
                // next_datum_token never hands these out
                return Err(ParseError::Syntax("Unexpected #;".to_string()));
            }
        })
    }

    /// `'x` and friends: `(name x)`.
    fn abbreviation(&mut self, name: &str) -> Result<Value, ParseError> {
        let token = self.next_datum_token()?;
        let quoted = self.datum(token)?;
        let sym = self.heap.intern(name);
        Ok(self.heap.list(vec![sym, quoted]))
    }

    /// The elements of a vector or bytevector, up to `close`.
    fn sequence(&mut self, close: Token, what: &str) -> Result<Vec<Value>, ParseError> {
        let mut elems = Vec::new();
        loop {
            match self.next_datum_token()? {
                t if t == close => return Ok(elems),
                Token::Eof => {
                    return Err(ParseError::Syntax(format!("Unclosed {} (unexpected EOF)", what)));
                }
                t => elems.push(self.datum(t)?),
            }
        }
    }

    /// A list after its opening parenthesis, possibly dotted.
    fn list(&mut self, close: Token) -> Result<Value, ParseError> {
        let mut elems = Vec::new();
        let mut tail = Value::NIL;
        loop {
            match self.next_datum_token()? {
                t if t == close => break,
                Token::Eof => {
                    return Err(ParseError::Syntax("Unclosed list (unexpected EOF)".to_string()));
                }
                Token::Dot if !elems.is_empty() => {
                    let t = self.next_datum_token()?;
                    tail = self.datum(t)?;
                    if self.next_datum_token()? != close {
                        return Err(ParseError::Syntax(
                            "Expected ')' after dotted pair".to_string(),
                        ));
                    }
                    break;
                }
                t => elems.push(self.datum(t)?),
            }
        }
        for elem in elems.into_iter().rev() {
            tail = self.heap.cons(elem, tail);
        }
        Ok(tail)
    }

    /// `#n=datum`. References to `#n#` inside the datum are read as a
    /// placeholder, which is then replaced by the datum itself.
    fn labelled(&mut self, n: u64) -> Result<Value, ParseError> {
        let placeholder = self.heap.cons(Value::NIL, Value::NIL);
        self.labels.insert(n, placeholder);
        let token = self.next_datum_token()?;
        let datum = self.datum(token)?;
        if datum.eq(placeholder) {
            return Ok(self.defer(format!("datum label #{}= refers only to itself", n)));
        }
        self.labels.insert(n, datum);
        replace_refs(datum, placeholder, datum);
        Ok(datum)
    }
}

/// Replace every reference to `from` inside the pairs and vectors reachable
/// from `root` with `to`. The structure may already be cyclic, so each
/// object is visited once.
fn replace_refs(root: Value, from: Value, to: Value) {
    let mut seen: HashSet<GcRef> = HashSet::default();
    let mut work = vec![root];
    let swap = |slot: &std::cell::Cell<Value>| {
        if slot.get().eq(from) {
            slot.set(to);
        }
        slot.get()
    };
    while let Some(v) = work.pop() {
        let Some(r) = v.as_ref() else { continue };
        if !seen.insert(r) {
            continue;
        }
        match &r.get().kind {
            Obj::Pair { car, cdr } => {
                work.push(swap(car));
                work.push(swap(cdr));
            }
            Obj::Vector(elems) => work.extend(elems.iter().map(swap)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printer::print_value;

    /// Read every datum in `s`, each written back with `write`.
    fn read_all(s: &str) -> Vec<Result<String, ParseError>> {
        let mut heap = Heap::new();
        let mut port = crate::io::new_string_port_input(s);
        let mut out = Vec::new();
        loop {
            match parse(&mut heap, &mut port) {
                Err(ParseError::Eof) => return out,
                r => out.push(r.map(print_value)),
            }
        }
    }

    fn read_ok(s: &str) -> Vec<String> {
        read_all(s).into_iter().map(|r| r.unwrap()).collect()
    }

    #[test]
    fn parse_atoms() {
        assert_eq!(
            read_ok(r#"42 hello "hello world" () #t #f #\a #\space 3.14"#),
            ["42", "hello", "\"hello world\"", "()", "#t", "#f", "#\\a", "#\\space", "3.14"]
        );
    }

    #[test]
    fn parse_number_kinds() {
        let mut heap = Heap::new();
        let mut port = crate::io::new_string_port_input("42 3.5 1/2 4/2 123456789012345678901234567890");
        let mut next = || parse(&mut heap, &mut port).unwrap();
        assert_eq!(next().as_fixnum(), Some(42));
        assert_eq!(next().as_flonum(), Some(3.5));
        assert!(matches!(next().as_obj().map(|o| &o.kind), Some(Obj::Rational(_))));
        assert_eq!(next().as_fixnum(), Some(2), "4/2 reads as the integer 2");
        assert!(matches!(next().as_obj().map(|o| &o.kind), Some(Obj::BigInt(_))));
    }

    #[test]
    fn parse_lists_and_vectors() {
        assert_eq!(
            read_ok("(1 2 3) (1 . 2) (1 2 . 3) #(1 2 3) [a b] #u8(0 255) 'hello"),
            ["(1 2 3)", "(1 . 2)", "(1 2 . 3)", "#(1 2 3)", "#(a b)", "#u8(0 255)", "(quote hello)"]
        );
    }

    #[test]
    fn parse_datum_comments() {
        assert_eq!(read_ok("(1 #;(2 3) 4) #;5 6 (#;7)"), ["(1 4)", "6", "()"]);
    }

    #[test]
    fn parse_datum_labels() {
        let mut heap = Heap::new();
        let mut port = crate::io::new_string_port_input("#0=(a b . #0#)");
        let expr = parse(&mut heap, &mut port).unwrap();
        let cdr = |v: Value| match &v.as_obj().unwrap().kind {
            Obj::Pair { cdr, .. } => cdr.get(),
            _ => panic!("not a pair"),
        };
        assert!(cdr(cdr(expr)).eq(expr), "the tail refers back to the list");

        assert_eq!(read_ok("(#1=(x) #1#)"), ["((x) (x))"]);
        assert!(matches!(read_all("#2#")[..], [Err(ParseError::Syntax(_))]));
    }

    #[test]
    fn parse_unsupported_values_keep_reader_in_sync() {
        // Each error is reported after its whole datum, so the next datum
        // reads normally.
        let results = read_all("(1 1/0 3) ok1 #u8(1 256) ok2 (#\\bogus x) ok3 1+2i ok4");
        let msgs: Vec<String> = results
            .iter()
            .map(|r| match r {
                Ok(s) => s.clone(),
                Err(ParseError::Syntax(m)) => format!("ERR {}", m),
                Err(ParseError::Eof) => "EOF".to_string(),
            })
            .collect();
        assert_eq!(msgs.len(), 8, "{:?}", msgs);
        assert!(msgs[0].contains("division by zero"));
        assert_eq!(msgs[1], "ok1");
        assert!(msgs[2].contains("bytevector elements must be"));
        assert_eq!(msgs[3], "ok2");
        assert!(msgs[4].contains("unknown character name"));
        assert_eq!(msgs[5], "ok3");
        assert!(msgs[6].contains("complex numbers are not supported"));
        assert_eq!(msgs[7], "ok4");
    }

    #[test]
    fn parse_nil_and_bar_symbols() {
        assert_eq!(read_ok("nil |nil| |a b|"), ["nil", "nil", "|a b|"]);
    }

    #[test]
    fn structural_errors() {
        assert!(matches!(read_all(")")[..], [Err(ParseError::Syntax(_)), ..]));
        assert!(matches!(read_all("(1 2")[..], [Err(ParseError::Syntax(_))]));
    }

    /// The P0 exit test: `write` of what was read gives back the text, for
    /// every lexical form the reader handles. Pairs are (input, expected)
    /// where the written form is canonical rather than identical.
    #[test]
    fn round_trip() {
        let same = [
            "(a b c)", "(a . b)", "(a b . c)", "#(1 \"two\" #\\3)", "#u8(1 2 255)", "#u8()",
            "\"a\\nb\\t\\\"q\\\" \\\\\"", "#\\newline", "#\\λ", "|a b|", "||", "abc->def",
            "1/2", "-3/4", "-0.0", "+inf.0", "-inf.0", "+nan.0", "1.0e+21", "123456789012345678901234567890",
            "-1152921504606846976", "1152921504606846976", "#t", "#f", "()", "#()",
            "#0=(a . #0#)", "#0=#(1 #0#)", "((nested (lists)) #(in #(vectors)))",
        ];
        for text in same {
            assert_eq!(read_ok(text), [text], "round trip of {}", text);
        }
        let canonical = [
            ("'x", "(quote x)"),
            ("`(a ,b ,@c)", "(quasiquote (a (unquote b) (unquote-splicing c)))"),
            ("#true #false", "#t"),
            ("#x1F", "31"),
            ("#e1.5", "3/2"),
            ("#i1/2", "0.5"),
            ("4/2", "2"),
            ("1e2", "100.0"),
            ("#\\x41", "#\\A"),
            ("#\\x3bb", "#\\λ"),
            ("\"\\x41;\"", "\"A\""),
            ("#| block #| nested |# |# x", "x"),
            ("; comment\n y", "y"),
            ("#!fold-case ABC", "abc"),
            ("(1 #;2 3)", "(1 3)"),
            ("[1 2]", "#(1 2)"),
        ];
        for (text, written) in canonical {
            assert_eq!(read_ok(text)[0], written, "reading {}", text);
        }
    }
}
