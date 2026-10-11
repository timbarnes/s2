//! Strings (R7RS §6.7), forked from s1's `builtin/string.rs`. A string is
//! an `SString` (which keeps character indexing fast for ASCII) in a
//! `RefCell`, since strings are mutable.

use std::cmp::Ordering;

use super::char::fold_string;
use super::{Error, Primitive, char_arg, fixnum, index_arg, kind, list_arg, prim, range_args, string_arg, vector_arg};
use crate::heap::{Heap, Obj};
use crate::value::Value;

pub static PRIMITIVES: &[Primitive] = &[
    prim("string?", 1, Some(1), |_, a| Ok(Value::bool(matches!(kind(a[0]), Some(Obj::Str(_)))))),
    prim("make-string", 1, Some(2), |h, a| {
        let n = index_arg(a[0], "make-string")?;
        let c = a.get(1).map_or(Ok(' '), |&c| char_arg(c, "make-string"))?;
        Ok(h.string(&std::iter::repeat_n(c, n).collect::<String>()))
    }),
    prim("string", 0, None, |h, a| {
        let s = a.iter().map(|&c| char_arg(c, "string")).collect::<Result<String, _>>()?;
        Ok(h.string(&s))
    }),
    prim("string-length", 1, Some(1), |_, a| Ok(fixnum(string_arg(a[0], "string-length")?.borrow().char_len()))),
    prim("string-ref", 2, Some(2), |_, a| {
        let k = index_arg(a[1], "string-ref")?;
        string_arg(a[0], "string-ref")?.borrow().char_at(k).map(Value::char).ok_or_else(|| "string-ref: index out of bounds".into())
    }),
    prim("string-set!", 3, Some(3), |_, a| {
        let k = index_arg(a[1], "string-set!")?;
        let c = char_arg(a[2], "string-set!")?;
        if string_arg(a[0], "string-set!")?.borrow_mut().set_char(k, c) {
            Ok(Value::UNSPECIFIED)
        } else {
            Err("string-set!: index out of bounds".into())
        }
    }),
    prim("string=?", 2, None, |_, a| compare(a, "string=?", false, Ordering::is_eq)),
    prim("string<?", 2, None, |_, a| compare(a, "string<?", false, Ordering::is_lt)),
    prim("string>?", 2, None, |_, a| compare(a, "string>?", false, Ordering::is_gt)),
    prim("string<=?", 2, None, |_, a| compare(a, "string<=?", false, Ordering::is_le)),
    prim("string>=?", 2, None, |_, a| compare(a, "string>=?", false, Ordering::is_ge)),
    prim("string-ci=?", 2, None, |_, a| compare(a, "string-ci=?", true, Ordering::is_eq)),
    prim("string-ci<?", 2, None, |_, a| compare(a, "string-ci<?", true, Ordering::is_lt)),
    prim("string-ci>?", 2, None, |_, a| compare(a, "string-ci>?", true, Ordering::is_gt)),
    prim("string-ci<=?", 2, None, |_, a| compare(a, "string-ci<=?", true, Ordering::is_le)),
    prim("string-ci>=?", 2, None, |_, a| compare(a, "string-ci>=?", true, Ordering::is_ge)),
    prim("substring", 3, Some(3), |h, a| copy_range(h, a, "substring")),
    prim("string-copy", 1, Some(3), |h, a| copy_range(h, a, "string-copy")),
    prim("string-append", 0, None, |h, a| {
        let mut out = String::new();
        for &s in a {
            out.push_str(string_arg(s, "string-append")?.borrow().as_str());
        }
        Ok(h.string(&out))
    }),
    prim("string-upcase", 1, Some(1), |h, a| map_string(h, a[0], "string-upcase", str::to_uppercase)),
    prim("string-downcase", 1, Some(1), |h, a| map_string(h, a[0], "string-downcase", str::to_lowercase)),
    prim("string-foldcase", 1, Some(1), |h, a| map_string(h, a[0], "string-foldcase", fold_string)),
    prim("string->list", 1, Some(3), |h, a| {
        let chars = char_range(a, "string->list")?;
        Ok(h.list(chars.into_iter().map(Value::char).collect()))
    }),
    prim("string->vector", 1, Some(3), |h, a| {
        let chars = char_range(a, "string->vector")?;
        Ok(h.vector(chars.into_iter().map(Value::char).collect()))
    }),
    prim("list->string", 1, Some(1), |h, a| {
        let s = list_arg(a[0], "list->string")?.into_iter().map(|c| char_arg(c, "list->string")).collect::<Result<String, _>>()?;
        Ok(h.string(&s))
    }),
    prim("vector->string", 1, Some(3), |h, a| {
        let items = vector_arg(a[0], "vector->string")?;
        let (start, end) = range_args(a, 1, items.len(), "vector->string")?;
        let s = items[start..end].iter().map(|c| char_arg(c.get(), "vector->string")).collect::<Result<String, _>>()?;
        Ok(h.string(&s))
    }),
    prim("string-fill!", 2, Some(4), |_, a| {
        let fill = char_arg(a[1], "string-fill!")?;
        let mut target = string_arg(a[0], "string-fill!")?.borrow_mut();
        let (start, end) = range_args(a, 2, target.char_len(), "string-fill!")?;
        target.replace_chars(start, &vec![fill; end - start]);
        Ok(Value::UNSPECIFIED)
    }),
    prim("string-copy!", 3, Some(5), |_, a| {
        // Copy the source characters out first: `to` and `from` may be the
        // same string.
        let src = char_range(&a[2..], "string-copy!")?;
        let at = index_arg(a[1], "string-copy!")?;
        if string_arg(a[0], "string-copy!")?.borrow_mut().replace_chars(at, &src) {
            Ok(Value::UNSPECIFIED)
        } else {
            Err("string-copy!: the copied characters don't fit at that index".into())
        }
    }),
];

/// `(name string [start [end]])` as a new string.
fn copy_range(heap: &mut Heap, args: &[Value], who: &str) -> Result<Value, Error> {
    let s = string_arg(args[0], who)?.borrow();
    let (start, end) = range_args(args, 1, s.char_len(), who)?;
    let copy = s.substring(start, end).expect("range checked above").to_string();
    drop(s);
    Ok(heap.string(&copy))
}

/// The characters of `args[0]`, a string, in the range `args[1..3]`.
fn char_range(args: &[Value], who: &str) -> Result<Vec<char>, Error> {
    let chars: Vec<char> = string_arg(args[0], who)?.borrow().chars().collect();
    let (start, end) = range_args(args, 1, chars.len(), who)?;
    Ok(chars[start..end].to_vec())
}

fn map_string(heap: &mut Heap, v: Value, who: &str, f: fn(&str) -> String) -> Result<Value, Error> {
    let out = f(string_arg(v, who)?.borrow().as_str());
    Ok(heap.string(&out))
}

fn compare(args: &[Value], who: &str, fold: bool, ok: fn(Ordering) -> bool) -> Result<Value, Error> {
    let strs = args
        .iter()
        .map(|&a| string_arg(a, who).map(|s| if fold { fold_string(&s.borrow()) } else { s.borrow().to_string() }))
        .collect::<Result<Vec<String>, Error>>()?;
    Ok(Value::bool(strs.windows(2).all(|w| ok(w[0].cmp(&w[1])))))
}

