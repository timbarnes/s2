//! `Value`, the representation of every Scheme value (plan/03 §3, D2).
//!
//! A `Value` is one 64-bit word, NaN-boxed:
//!
//! - **A flonum is its own bits.** Every NaN is stored as one canonical
//!   quiet NaN, so the other NaN bit patterns are free.
//! - **Everything else is a NaN with the sign bit set:** the top 16 bits
//!   are `0xFFF8 | tag`, and the low 48 bits the payload.
//!
//! | tag | payload |
//! |---|---|
//! | 1 | a fixnum, 48-bit two's complement |
//! | 2 | a `GcRef`: the heap object's address (user-space addresses fit in 48 bits) |
//! | 3 | a character |
//! | 4 | a constant: `()`, `#f`, `#t`, unspecified, eof, undefined |
//!
//! Tag 0 would be the NaN `0xFFF8_0000_0000_0000`, which `flonum` never
//! stores, so a word is a flonum exactly when it is below `0xFFF9 << 48`.
//!
//! The layout is private. Code outside this module builds values with the
//! constructors and constants below and takes them apart with the
//! accessors, never by looking at the bits.

use crate::heap::{GcObject, GcRef};

/// The smallest fixnum. Fixnums are 48-bit signed integers, what fits in
/// a NaN-boxed payload; integers outside the range are bignums.
pub const FIXNUM_MIN: i64 = -(1 << 47);
/// The largest fixnum.
pub const FIXNUM_MAX: i64 = (1 << 47) - 1;

/// A Scheme value.
#[derive(Clone, Copy)]
pub struct Value(u64);

const TAG_SHIFT: u32 = 48;
const PAYLOAD: u64 = (1 << TAG_SHIFT) - 1;
const fn boxed(tag: u64, payload: u64) -> u64 {
    ((0xFFF8 | tag) << TAG_SHIFT) | payload
}
const FIXNUM: u64 = 1;
const OBJ: u64 = 2;
const CHAR: u64 = 3;
const CONST: u64 = 4;
/// The one NaN a flonum holds: positive, quiet, no payload.
const CANONICAL_NAN: u64 = 0x7FF8_0000_0000_0000;

const _: () = assert!(std::mem::size_of::<Value>() == 8);

impl Value {
    pub const NIL: Value = Value(boxed(CONST, 0));
    pub const FALSE: Value = Value(boxed(CONST, 1));
    pub const TRUE: Value = Value(boxed(CONST, 2));
    /// The value of expressions whose value R7RS leaves unspecified
    /// (`(if #f #f)`, `set!`, `display`). The REPL shows nothing for it.
    pub const UNSPECIFIED: Value = Value(boxed(CONST, 3));
    pub const EOF: Value = Value(boxed(CONST, 4));
    /// The contents of a variable that has no value yet: an unbound global
    /// cell, a `letrec` variable before its initialisation.
    pub const UNDEFINED: Value = Value(boxed(CONST, 5));

    fn tag(self) -> u64 {
        (self.0 >> TAG_SHIFT) ^ 0xFFF8
    }

    /// The fixnum `n`, or None if `n` is outside the fixnum range (the
    /// caller then makes a bignum; see `Heap::integer`).
    pub fn fixnum(n: i64) -> Option<Value> {
        (FIXNUM_MIN..=FIXNUM_MAX).contains(&n).then_some(Value(boxed(FIXNUM, n as u64 & PAYLOAD)))
    }

    pub fn flonum(f: f64) -> Value {
        Value(if f.is_nan() { CANONICAL_NAN } else { f.to_bits() })
    }

    pub fn char(c: char) -> Value {
        Value(boxed(CHAR, c as u64))
    }

    pub fn bool(b: bool) -> Value {
        if b { Value::TRUE } else { Value::FALSE }
    }

    pub fn obj(r: GcRef) -> Value {
        let addr = r.addr();
        debug_assert!(addr <= PAYLOAD, "heap address {:#x} doesn't fit in 48 bits", addr);
        Value(boxed(OBJ, addr))
    }

    pub fn as_fixnum(self) -> Option<i64> {
        // Sign-extend the 48-bit payload.
        (self.tag() == FIXNUM).then(|| ((self.0 << 16) as i64) >> 16)
    }

    pub fn as_flonum(self) -> Option<f64> {
        (self.0 < boxed(1, 0)).then(|| f64::from_bits(self.0))
    }

    pub fn as_char(self) -> Option<char> {
        // Only `char` makes a CHAR payload, so it is a valid scalar value.
        (self.tag() == CHAR).then(|| char::from_u32((self.0 & PAYLOAD) as u32).unwrap())
    }

    pub fn as_bool(self) -> Option<bool> {
        match self.0 {
            x if x == Value::TRUE.0 => Some(true),
            x if x == Value::FALSE.0 => Some(false),
            _ => None,
        }
    }

    pub fn as_ref(self) -> Option<GcRef> {
        (self.tag() == OBJ).then(|| GcRef::from_addr(self.0 & PAYLOAD))
    }

    /// The heap object, if this value is one. The lifetime is unbounded,
    /// as for `GcRef::get`.
    pub fn as_obj<'a>(self) -> Option<&'a GcObject> {
        self.as_ref().map(GcRef::get)
    }

    pub fn is_nil(self) -> bool {
        self.0 == Value::NIL.0
    }

    pub fn is_unspecified(self) -> bool {
        self.0 == Value::UNSPECIFIED.0
    }

    pub fn is_eof(self) -> bool {
        self.0 == Value::EOF.0
    }

    pub fn is_undefined(self) -> bool {
        self.0 == Value::UNDEFINED.0
    }

    /// Scheme truth: everything but `#f` is true.
    pub fn is_true(self) -> bool {
        self.0 != Value::FALSE.0
    }

    /// `eq?`: the same heap object, or the same immediate. Flonums compare
    /// by bits, so `(eq? x x)` holds for a NaN.
    pub fn eq(self, other: Value) -> bool {
        self.0 == other.0
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(n) = self.as_fixnum() {
            write!(f, "Fixnum({})", n)
        } else if let Some(x) = self.as_flonum() {
            write!(f, "Flonum({:?})", x)
        } else if let Some(c) = self.as_char() {
            write!(f, "Char({:?})", c)
        } else if let Some(r) = self.as_ref() {
            write!(f, "Obj({:?})", r)
        } else {
            let names = ["Nil", "False", "True", "Unspecified", "Eof", "Undefined"];
            f.write_str(names[(self.0 & PAYLOAD) as usize])
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

    /// Each kind round-trips, and no value is mistaken for another kind:
    /// the encoding's edge cases (negative fixnums, every special float).
    #[test]
    fn encoding_round_trips() {
        for n in [0, 1, -1, 42, -42, FIXNUM_MAX, FIXNUM_MIN] {
            let v = Value::fixnum(n).unwrap();
            assert_eq!((v.as_fixnum(), v.as_flonum(), v.as_ref().is_some()), (Some(n), None, false), "{}", n);
        }
        for x in [0.0, -0.0, 1.5, -1.5, f64::MAX, f64::MIN_POSITIVE, f64::INFINITY, f64::NEG_INFINITY] {
            let v = Value::flonum(x);
            assert_eq!(v.as_flonum().map(f64::to_bits), Some(x.to_bits()));
            assert_eq!((v.as_fixnum(), v.as_char(), v.as_bool()), (None, None, None));
        }
        // Every NaN, whatever its sign and payload, is one canonical NaN.
        for nan in [f64::NAN, -f64::NAN, f64::from_bits(0xFFF8_0000_0000_0001), f64::from_bits(0x7FF0_0000_0000_0001)] {
            let v = Value::flonum(nan);
            assert!(v.as_flonum().unwrap().is_nan());
            assert!(v.eq(Value::flonum(f64::NAN)));
        }
        for c in ['a', '\0', 'λ', '\u{10FFFF}'] {
            assert_eq!(Value::char(c).as_char(), Some(c));
            assert_eq!(Value::char(c).as_fixnum(), None);
        }
        let constants = [Value::NIL, Value::FALSE, Value::TRUE, Value::UNSPECIFIED, Value::EOF, Value::UNDEFINED];
        for (i, a) in constants.iter().enumerate() {
            assert_eq!((a.as_fixnum(), a.as_flonum(), a.as_char(), a.as_ref().is_some()), (None, None, None, false));
            for (j, b) in constants.iter().enumerate() {
                assert_eq!(a.eq(*b), i == j);
            }
        }
        assert_eq!((Value::TRUE.as_bool(), Value::FALSE.as_bool(), Value::NIL.as_bool()), (Some(true), Some(false), None));
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
