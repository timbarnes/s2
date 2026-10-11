//! `Value`, the representation of every Scheme value (plan/03 §3, D2).
//!
//! A `Value` is 16 bytes and `Copy`. Fixnums, flonums, characters,
//! booleans and the special constants are stored in the value itself;
//! everything else is a `GcRef` to a heap object (`heap.rs`).
//!
//! The layout is private. Code outside this module builds values with the
//! constructors and constants below and takes them apart with the
//! accessors, never by matching on the representation. That keeps the
//! option of replacing the enum with a tagged 64-bit word later (plan/04,
//! P10) by changing only this file.

use crate::heap::{GcObject, GcRef};

/// The smallest fixnum. Fixnums are 61-bit signed integers, the range a
/// tagged word with a 3-bit tag could hold, so a later change of
/// representation doesn't change which integers are fixnums.
pub const FIXNUM_MIN: i64 = -(1 << 60);
/// The largest fixnum.
pub const FIXNUM_MAX: i64 = (1 << 60) - 1;

/// A Scheme value.
#[derive(Clone, Copy, Debug)]
pub struct Value(Repr);

#[derive(Clone, Copy, Debug)]
enum Repr {
    Fixnum(i64),
    Flonum(f64),
    Char(char),
    Bool(bool),
    Nil,
    /// The value of expressions whose value R7RS leaves unspecified
    /// (`(if #f #f)`, `set!`, `display`). Prints as nothing at the REPL.
    Unspecified,
    Eof,
    /// The contents of a variable that has no value yet: an unbound global
    /// cell, a `letrec` variable before its initialisation.
    Undefined,
    Obj(GcRef),
}

const _: () = assert!(std::mem::size_of::<Value>() == 16);

impl Value {
    pub const NIL: Value = Value(Repr::Nil);
    pub const TRUE: Value = Value(Repr::Bool(true));
    pub const FALSE: Value = Value(Repr::Bool(false));
    pub const UNSPECIFIED: Value = Value(Repr::Unspecified);
    pub const EOF: Value = Value(Repr::Eof);
    pub const UNDEFINED: Value = Value(Repr::Undefined);

    /// The fixnum `n`, or None if `n` is outside the fixnum range (the
    /// caller then makes a bignum; see `Heap::integer`).
    pub fn fixnum(n: i64) -> Option<Value> {
        (FIXNUM_MIN..=FIXNUM_MAX).contains(&n).then_some(Value(Repr::Fixnum(n)))
    }

    pub fn flonum(f: f64) -> Value {
        Value(Repr::Flonum(f))
    }

    pub fn char(c: char) -> Value {
        Value(Repr::Char(c))
    }

    pub fn bool(b: bool) -> Value {
        Value(Repr::Bool(b))
    }

    pub fn obj(r: GcRef) -> Value {
        Value(Repr::Obj(r))
    }

    pub fn as_fixnum(self) -> Option<i64> {
        match self.0 {
            Repr::Fixnum(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_flonum(self) -> Option<f64> {
        match self.0 {
            Repr::Flonum(f) => Some(f),
            _ => None,
        }
    }

    pub fn as_char(self) -> Option<char> {
        match self.0 {
            Repr::Char(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_bool(self) -> Option<bool> {
        match self.0 {
            Repr::Bool(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_ref(self) -> Option<GcRef> {
        match self.0 {
            Repr::Obj(r) => Some(r),
            _ => None,
        }
    }

    /// The heap object, if this value is one.
    pub fn as_obj(&self) -> Option<&GcObject> {
        match &self.0 {
            Repr::Obj(r) => Some(r.get()),
            _ => None,
        }
    }

    pub fn is_nil(self) -> bool {
        matches!(self.0, Repr::Nil)
    }

    pub fn is_unspecified(self) -> bool {
        matches!(self.0, Repr::Unspecified)
    }

    pub fn is_eof(self) -> bool {
        matches!(self.0, Repr::Eof)
    }

    pub fn is_undefined(self) -> bool {
        matches!(self.0, Repr::Undefined)
    }

    /// Scheme truth: everything but `#f` is true.
    pub fn is_true(self) -> bool {
        !matches!(self.0, Repr::Bool(false))
    }

    /// `eq?`: the same heap object, or the same immediate. Flonums compare
    /// by bits, so `(eq? x x)` holds for a NaN.
    pub fn eq(self, other: Value) -> bool {
        match (self.0, other.0) {
            (Repr::Fixnum(a), Repr::Fixnum(b)) => a == b,
            (Repr::Flonum(a), Repr::Flonum(b)) => a.to_bits() == b.to_bits(),
            (Repr::Char(a), Repr::Char(b)) => a == b,
            (Repr::Bool(a), Repr::Bool(b)) => a == b,
            (Repr::Obj(a), Repr::Obj(b)) => a == b,
            (Repr::Nil, Repr::Nil)
            | (Repr::Unspecified, Repr::Unspecified)
            | (Repr::Eof, Repr::Eof)
            | (Repr::Undefined, Repr::Undefined) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixnum_range() {
        assert_eq!(Value::fixnum(FIXNUM_MAX).and_then(Value::as_fixnum), Some(FIXNUM_MAX));
        assert_eq!(Value::fixnum(FIXNUM_MIN).and_then(Value::as_fixnum), Some(FIXNUM_MIN));
        assert!(Value::fixnum(FIXNUM_MAX + 1).is_none());
        assert!(Value::fixnum(FIXNUM_MIN - 1).is_none());
    }

    #[test]
    fn eq_on_immediates() {
        assert!(Value::NIL.eq(Value::NIL));
        assert!(!Value::NIL.eq(Value::FALSE));
        assert!(Value::flonum(f64::NAN).eq(Value::flonum(f64::NAN)));
        assert!(!Value::FALSE.is_true());
        assert!(Value::NIL.is_true());
    }
}
