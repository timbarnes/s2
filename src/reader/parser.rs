//! Parser for Scheme s-expressions.
//!
//! Turns the tokens of a `Tokenizer` into unevaluated s-expressions on the GC
//! heap, one datum per `parse` call. Besides lists, vectors and the quote
//! abbreviations it handles `#;` datum comments and `#n=` / `#n#` datum
//! labels, which may make a datum refer to itself.
//!
//! Errors come in two kinds. Structural ones (an unexpected `)`, a list left
//! open at end of input) stop parsing at once. Errors in a complete token
//! (an unknown character name, a bad escape, `1/2` or `#u8(...)` whose types
//! s1 doesn't have yet) are remembered while the rest of the top-level datum
//! is read, then reported. That leaves the port positioned after the datum,
//! so a bad literal deep in a list doesn't make the reader misread the rest
//! of the list as new top-level forms.

use crate::gc::{
    GcHeap, GcRef, SchemeValue, get_symbol, new_bool, new_bytevector, new_char, new_float, new_int,
    new_pair, new_rational, new_string, new_vector,
};
use crate::{gc_value, gc_value_mut};
use num_traits::ToPrimitive;
use crate::io::PortKind;
use crate::number_syntax::{Number, NumberSyntax, parse_number};
use crate::tokenizer::{Token, Tokenizer};
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
pub fn parse(heap: &mut GcHeap, port_ref: &mut PortKind) -> Result<GcRef, ParseError> {
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
    heap: &'b mut GcHeap,
    /// Datum labels defined so far in this top-level datum.
    labels: HashMap<u64, GcRef>,
    /// The first error found in a complete token; see the module docs.
    deferred: Option<String>,
}

impl Reader<'_, '_> {
    /// Remember `msg` (if it's the first error) and stand in a placeholder
    /// value so reading can continue to the end of the datum.
    fn defer(&mut self, msg: String) -> GcRef {
        self.deferred.get_or_insert(msg);
        self.heap.unspecified()
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
    fn datum(&mut self, token: Token) -> Result<GcRef, ParseError> {
        Ok(match token {
            Token::Eof => return Err(ParseError::Eof),
            Token::Number(s) => match parse_number(&s, 10) {
                NumberSyntax::Value(Number::Int(i)) => new_int(self.heap, i),
                NumberSyntax::Value(Number::Float(f)) => new_float(self.heap, f),
                NumberSyntax::Value(Number::Rational(r)) => new_rational(self.heap, r),
                NumberSyntax::Error(msg) => self.defer(msg),
                NumberSyntax::NotANumber => self.defer(format!("invalid number {}", s)),
            },
            Token::String(s) => new_string(self.heap, &s),
            Token::Boolean(b) => new_bool(self.heap, b),
            Token::Character(c) => new_char(self.heap, c),
            Token::Symbol(s) | Token::BarSymbol(s) => get_symbol(self.heap, &s),
            Token::LeftParen => self.list(Token::RightParen)?,
            Token::LeftBracket => {
                let elems = self.sequence(Token::RightBracket, "vector")?;
                new_vector(self.heap, elems)
            }
            Token::HashParen => {
                let elems = self.sequence(Token::RightParen, "vector")?;
                new_vector(self.heap, elems)
            }
            Token::ByteVectorStart => {
                let elems = self.sequence(Token::RightParen, "bytevector")?;
                let mut bytes = Vec::with_capacity(elems.len());
                for e in elems {
                    match gc_value!(e) {
                        SchemeValue::Int(i) if i.to_u8().is_some() => bytes.push(i.to_u8().unwrap()),
                        _ => {
                            return Ok(self.defer(format!(
                                "bytevector elements must be exact integers 0-255, not {}",
                                crate::printer::print_value(&e)
                            )));
                        }
                    }
                }
                new_bytevector(self.heap, bytes)
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
    fn abbreviation(&mut self, name: &str) -> Result<GcRef, ParseError> {
        let token = self.next_datum_token()?;
        let quoted = self.datum(token)?;
        let sym = get_symbol(self.heap, name);
        let nil = self.heap.nil_s();
        let tail = new_pair(self.heap, quoted, nil);
        Ok(new_pair(self.heap, sym, tail))
    }

    /// The elements of a vector or bytevector, up to `close`.
    fn sequence(&mut self, close: Token, what: &str) -> Result<Vec<GcRef>, ParseError> {
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
    fn list(&mut self, close: Token) -> Result<GcRef, ParseError> {
        let mut elems = Vec::new();
        let mut tail = self.heap.nil_s();
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
            tail = new_pair(self.heap, elem, tail);
        }
        Ok(tail)
    }

    /// `#n=datum`. References to `#n#` inside the datum are read as a
    /// placeholder, which is then replaced by the datum itself.
    fn labelled(&mut self, n: u64) -> Result<GcRef, ParseError> {
        let nil = self.heap.nil_s();
        let placeholder = new_pair(self.heap, nil, nil);
        self.labels.insert(n, placeholder);
        let token = self.next_datum_token()?;
        let datum = self.datum(token)?;
        if datum == placeholder {
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
fn replace_refs(root: GcRef, from: GcRef, to: GcRef) {
    let mut seen = HashSet::default();
    let mut work = vec![root];
    let swap = |slot: &mut GcRef| {
        if *slot == from {
            *slot = to;
        }
    };
    while let Some(obj) = work.pop() {
        if !seen.insert(obj) {
            continue;
        }
        match gc_value_mut!(obj) {
            SchemeValue::Pair(car, cdr) => {
                swap(car);
                swap(cdr);
                work.push(*car);
                work.push(*cdr);
            }
            SchemeValue::Vector(elems) => {
                for elem in elems.iter_mut() {
                    swap(elem);
                    work.push(*elem);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn parse_number() {
        use crate::gc::SchemeValue;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("42");
        let expr = parse(ec.heap, &mut port).unwrap();
        match &ec.heap.get_value(expr) {
            SchemeValue::Int(i) => assert_eq!(i.to_string(), "42"),
            _ => panic!(
                "Expected integer, got {}",
                crate::printer::print_value(&expr)
            ),
        }
    }

    #[test]
    fn parse_symbol() {
        use crate::printer::print_value;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("hello");
        let expr = parse(ec.heap, &mut port).unwrap();
        match &ec.heap.get_value(expr) {
            crate::gc::SchemeValue::Symbol(s) => assert_eq!(s, "hello"),
            _ => panic!("Expected symbol, got {}", print_value(&expr)),
        }
    }

    #[test]
    fn parse_string() {
        use crate::printer::print_value;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("\"hello world\"");
        let expr = parse(ec.heap, &mut port).unwrap();
        match &ec.heap.get_value(expr) {
            crate::gc::SchemeValue::Str(s) => assert_eq!(s, "hello world"),
            _ => panic!("Expected string, got {}", print_value(&expr)),
        }
    }

    #[test]
    fn parse_empty_list() {
        use crate::printer::print_value;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("()");
        let expr = parse(ec.heap, &mut port).unwrap();
        match &ec.heap.get_value(expr) {
            crate::gc::SchemeValue::Nil => (),
            _ => panic!("Expected (), got {}", print_value(&expr)),
        }
    }

    #[test]
    fn parse_list() {
        use crate::gc::SchemeValue;
        use crate::printer::print_value;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut str_port = crate::io::new_string_port_input("(1 2 3)");
        let expr = parse(ec.heap, &mut str_port).unwrap();

        match &ec.heap.get_value(expr) {
            SchemeValue::Pair(car, cdr) => {
                match &ec.heap.get_value(*car) {
                    SchemeValue::Int(i) => assert_eq!(i.to_string(), "1"),
                    _ => panic!("Expected integer 1, got {}", print_value(car)),
                }
                match &ec.heap.get_value(*cdr) {
                    SchemeValue::Pair(car2, cdr2) => {
                        match &ec.heap.get_value(*car2) {
                            crate::gc::SchemeValue::Int(i) => assert_eq!(i.to_string(), "2"),
                            _ => {
                                panic!("Expected integer 2, got {}", print_value(car2))
                            }
                        }
                        match &ec.heap.get_value(*cdr2) {
                            crate::gc::SchemeValue::Pair(car3, cdr3) => {
                                match &ec.heap.get_value(*car3) {
                                    crate::gc::SchemeValue::Int(i) => {
                                        assert_eq!(i.to_string(), "3")
                                    }
                                    _ => {
                                        panic!("Expected integer 3, got {}", print_value(car3))
                                    }
                                }
                                match &ec.heap.get_value(*cdr3) {
                                    SchemeValue::Nil => (),
                                    _ => panic!("Expected nil, got {}", print_value(cdr3)),
                                }
                            }
                            _ => panic!("Expected pair, got {}", print_value(cdr2)),
                        }
                    }
                    _ => panic!("Expected pair, got {}", print_value(cdr)),
                }
            }
            _ => panic!("Expected pair, got {}", print_value(&expr)),
        }
    }

    #[test]
    fn parse_booleans() {
        use crate::printer::print_value;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("#t #f");

        let expr = parse(ec.heap, &mut port).unwrap();
        match &ec.heap.get_value(expr) {
            crate::gc::SchemeValue::Bool(b) => assert_eq!(*b, true),
            _ => panic!("Expected true, got {}", print_value(&expr)),
        }

        let expr = parse(ec.heap, &mut port).unwrap();
        match &ec.heap.get_value(expr) {
            crate::gc::SchemeValue::Bool(b) => assert_eq!(*b, false),
            _ => panic!("Expected false, got {}", print_value(&expr)),
        }
    }

    #[test]
    fn parse_character() {
        use crate::gc::SchemeValue;
        use crate::printer::print_value;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("#\\a #\\space");

        let expr = parse(ec.heap, &mut port).unwrap();
        match &ec.heap.get_value(expr) {
            SchemeValue::Char(c) => assert_eq!(*c, 'a'),
            _ => panic!("Expected character 'a', got {}", print_value(&expr)),
        }

        let expr = parse(ec.heap, &mut port).unwrap();
        match &ec.heap.get_value(expr) {
            SchemeValue::Char(c) => assert_eq!(*c, ' '),
            _ => panic!("Expected character ' ', got {}", print_value(&expr)),
        }
    }

    #[test]
    fn parse_quoted() {
        use crate::gc::SchemeValue;
        use crate::printer::print_value;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("'hello");
        let expr = parse(ec.heap, &mut port).unwrap();

        match &ec.heap.get_value(expr) {
            SchemeValue::Pair(quote_sym, quoted_expr) => {
                match &ec.heap.get_value(*quote_sym) {
                    SchemeValue::Symbol(s) => assert_eq!(s, "quote"),
                    _ => panic!("Expected symbol 'quote', got {}", print_value(quote_sym)),
                }
                match &ec.heap.get_value(*quoted_expr) {
                    SchemeValue::Pair(hello_sym, nil) => {
                        match &ec.heap.get_value(*hello_sym) {
                            SchemeValue::Symbol(s) => assert_eq!(s, "hello"),
                            _ => panic!("Expected symbol 'hello', got {}", print_value(hello_sym)),
                        }
                        match &ec.heap.get_value(*nil) {
                            SchemeValue::Nil => (),
                            _ => panic!("Expected nil, got {}", print_value(nil)),
                        }
                    }
                    _ => panic!("Expected pair, got {}", print_value(quoted_expr)),
                }
            }
            _ => panic!("Expected pair, got {}", print_value(&expr)),
        }
    }

    #[test]
    fn parse_dotted_pair() {
        use crate::gc::SchemeValue;
        use crate::printer::print_value;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("(1 . 2)");
        let expr = parse(ec.heap, &mut port).unwrap();

        match &ec.heap.get_value(expr) {
            SchemeValue::Pair(car, cdr) => {
                match &ec.heap.get_value(*car) {
                    SchemeValue::Int(i) => assert_eq!(i.to_string(), "1"),
                    _ => panic!("Expected integer 1, got {}", print_value(car)),
                }
                match &ec.heap.get_value(*cdr) {
                    SchemeValue::Int(i) => assert_eq!(i.to_string(), "2"),
                    _ => panic!("Expected integer 2, got {}", print_value(cdr)),
                }
            }
            _ => panic!("Expected pair, got {}", print_value(&expr)),
        }
    }

    #[test]
    fn parse_vector() {
        use crate::gc::SchemeValue;
        use crate::printer::print_value;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("#(1 2 3)");
        let expr = parse(ec.heap, &mut port).unwrap();

        match &ec.heap.get_value(expr) {
            SchemeValue::Vector(v) => {
                assert_eq!(v.len(), 3);
                match &ec.heap.get_value(v[0]) {
                    SchemeValue::Int(i) => assert_eq!(i.to_string(), "1"),
                    _ => panic!("Expected integer 1, got {}", print_value(&v[0])),
                }
                match &ec.heap.get_value(v[1]) {
                    SchemeValue::Int(i) => assert_eq!(i.to_string(), "2"),
                    _ => panic!("Expected integer 2, got {}", print_value(&v[1])),
                }
                match &ec.heap.get_value(v[2]) {
                    SchemeValue::Int(i) => assert_eq!(i.to_string(), "3"),
                    _ => panic!("Expected integer 3, got {}", print_value(&v[2])),
                }
            }
            _ => panic!("Expected vector, got {}", print_value(&expr)),
        }
    }

    #[test]
    fn parse_float() {
        use crate::gc::SchemeValue;
        use crate::printer::print_value;
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("3.14");
        let expr = parse(ec.heap, &mut port).unwrap();
        match &ec.heap.get_value(expr) {
            SchemeValue::Float(f) => assert_eq!(*f, 3.14),
            _ => panic!("Expected float, got {}", print_value(&expr)),
        }
    }

    fn read_all(s: &str) -> Vec<Result<String, ParseError>> {
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input(s);
        let mut out = Vec::new();
        loop {
            match parse(ec.heap, &mut port) {
                Err(ParseError::Eof) => return out,
                r => out.push(r.map(|v| crate::printer::print_value(&v))),
            }
        }
    }

    #[test]
    fn parse_datum_comments() {
        assert_eq!(
            read_all("(1 #;(2 3) 4) #;5 6 (#;7)"),
            vec![Ok("(1 4)".to_string()), Ok("6".to_string()), Ok("()".to_string())]
        );
    }

    #[test]
    fn parse_datum_labels() {
        let mut ev = crate::eval::RunTimeStruct::new();
        let ec = crate::eval::RunTime::from_eval(&mut ev);
        let mut port = crate::io::new_string_port_input("#0=(a b . #0#)");
        let expr = parse(ec.heap, &mut port).unwrap();
        let third = crate::gc::cdr(crate::gc::cdr(expr).unwrap()).unwrap();
        assert!(std::ptr::eq(third, expr), "the tail refers back to the list");

        assert_eq!(read_all("(#1=(x) #1#)"), vec![Ok("((x) (x))".to_string())]);
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
        // nil is an ordinary symbol (s1-core.scm binds it to '() as a variable).
        assert_eq!(read_all("nil |nil| |a b|"), vec![
            Ok("nil".to_string()),
            Ok("nil".to_string()),
            Ok("|a b|".to_string()),
        ]);
    }
}
