//! Characters (R7RS §6.6), forked from s1's `builtin/char.rs`.

use std::cmp::Ordering;

use super::{Primitive, char_arg, prim};
use crate::value::Value;

pub static PRIMITIVES: &[Primitive] = &[
    prim("char?", 1, Some(1), |_, a| Ok(Value::bool(a[0].as_char().is_some()))),
    prim("char=?", 2, None, |_, a| compare(a, "char=?", same, Ordering::is_eq)),
    prim("char<?", 2, None, |_, a| compare(a, "char<?", same, Ordering::is_lt)),
    prim("char>?", 2, None, |_, a| compare(a, "char>?", same, Ordering::is_gt)),
    prim("char<=?", 2, None, |_, a| compare(a, "char<=?", same, Ordering::is_le)),
    prim("char>=?", 2, None, |_, a| compare(a, "char>=?", same, Ordering::is_ge)),
    prim("char-ci=?", 2, None, |_, a| compare(a, "char-ci=?", fold_char, Ordering::is_eq)),
    prim("char-ci<?", 2, None, |_, a| compare(a, "char-ci<?", fold_char, Ordering::is_lt)),
    prim("char-ci>?", 2, None, |_, a| compare(a, "char-ci>?", fold_char, Ordering::is_gt)),
    prim("char-ci<=?", 2, None, |_, a| compare(a, "char-ci<=?", fold_char, Ordering::is_le)),
    prim("char-ci>=?", 2, None, |_, a| compare(a, "char-ci>=?", fold_char, Ordering::is_ge)),
    prim("char-alphabetic?", 1, Some(1), |_, a| test(a[0], "char-alphabetic?", char::is_alphabetic)),
    prim("char-numeric?", 1, Some(1), |_, a| test(a[0], "char-numeric?", |c| digit_value(c).is_some())),
    prim("char-whitespace?", 1, Some(1), |_, a| test(a[0], "char-whitespace?", char::is_whitespace)),
    prim("char-upper-case?", 1, Some(1), |_, a| test(a[0], "char-upper-case?", char::is_uppercase)),
    prim("char-lower-case?", 1, Some(1), |_, a| test(a[0], "char-lower-case?", char::is_lowercase)),
    prim("digit-value", 1, Some(1), |_, a| {
        Ok(digit_value(char_arg(a[0], "digit-value")?).map_or(Value::FALSE, |d| Value::fixnum(d as i64).unwrap()))
    }),
    prim("char->integer", 1, Some(1), |_, a| Ok(Value::fixnum(char_arg(a[0], "char->integer")? as i64).unwrap())),
    prim("integer->char", 1, Some(1), |_, a| {
        a[0].as_fixnum()
            .and_then(|n| u32::try_from(n).ok())
            .and_then(char::from_u32)
            .map(Value::char)
            .ok_or_else(|| super::expected("integer->char", "a Unicode scalar value", a[0]))
    }),
    prim("char-upcase", 1, Some(1), |_, a| Ok(Value::char(upcase(char_arg(a[0], "char-upcase")?)))),
    prim("char-downcase", 1, Some(1), |_, a| Ok(Value::char(downcase(char_arg(a[0], "char-downcase")?)))),
    prim("char-foldcase", 1, Some(1), |_, a| Ok(Value::char(fold_char(char_arg(a[0], "char-foldcase")?)))),
];

/// The code points of the zero digit of each Unicode decimal digit range
/// (general category Nd), each followed by nine more.
const DIGIT_ZEROS: &[u32] = &[
    0x0030, 0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6, 0x0D66, 0x0DE6, 0x0E50, 0x0ED0,
    0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0, 0x1A80, 0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900,
    0xA9D0, 0xA9F0, 0xAA50, 0xABF0, 0xFF10, 0x104A0, 0x10D30, 0x11066, 0x110F0, 0x11136, 0x111D0, 0x112F0, 0x11450, 0x114D0,
    0x11650, 0x116C0, 0x11730, 0x118E0, 0x11950, 0x11C50, 0x11D50, 0x11DA0, 0x16A60, 0x16AC0, 0x16B50, 0x1D7CE, 0x1D7D8, 0x1D7E2,
    0x1D7EC, 0x1D7F6, 0x1E140, 0x1E2F0, 0x1E950, 0x1FBF0,
];

/// The value of a decimal digit in any script.
pub fn digit_value(c: char) -> Option<u32> {
    let cp = c as u32;
    DIGIT_ZEROS.iter().find(|&&zero| cp >= zero && cp < zero + 10).map(|zero| cp - zero)
}

/// `f(c)` if it is a single character, else `c` (R7RS: `char-upcase` of
/// `ß` is `ß`).
fn single(c: char, mapped: impl Iterator<Item = char>) -> char {
    let mut chars = mapped;
    match (chars.next(), chars.next()) {
        (Some(m), None) => m,
        _ => c,
    }
}

pub fn upcase(c: char) -> char {
    single(c, c.to_uppercase())
}

pub fn downcase(c: char) -> char {
    single(c, c.to_lowercase())
}

/// Simple case folding, for `char-foldcase` and the `-ci` comparisons.
pub fn fold_char(c: char) -> char {
    match c {
        '\u{17F}' => 's',       // long s
        '\u{3C2}' => '\u{3C3}', // final sigma to sigma
        '\u{1E9E}' => '\u{DF}', // capital sharp s to sharp s
        c => downcase(c),
    }
}

/// Full case folding, for `string-foldcase` and the string `-ci`
/// comparisons.
pub fn fold_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{DF}' | '\u{1E9E}' => out.push_str("ss"),
            '\u{17F}' => out.push('s'),
            '\u{3C2}' => out.push('\u{3C3}'),
            c => out.extend(c.to_lowercase()),
        }
    }
    out
}

fn same(c: char) -> char {
    c
}

fn compare(args: &[Value], who: &str, key: fn(char) -> char, ok: fn(Ordering) -> bool) -> Result<Value, String> {
    let cs = args.iter().map(|&a| char_arg(a, who)).collect::<Result<Vec<_>, _>>()?;
    Ok(Value::bool(cs.windows(2).all(|w| ok(key(w[0]).cmp(&key(w[1]))))))
}

fn test(v: Value, who: &str, f: fn(char) -> bool) -> Result<Value, String> {
    Ok(Value::bool(f(char_arg(v, who)?)))
}
