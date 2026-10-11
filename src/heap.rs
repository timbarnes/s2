//! The heap: object layout, allocation and the symbol table (plan/03 §3-4).
//!
//! Every object is its own `Box`, and the heap keeps a list of them. Objects
//! never move, so a `GcRef` stays valid for as long as the object is alive.
//! Mutable fields are `Cell`s (or a `RefCell` for strings), so all access is
//! through shared references, unlike s1's `get_value_mut(&self)`.
//!
//! ponytail: P0 never collects. Objects live until the heap is dropped. The
//! mark-sweep collector comes in P3, and with it the rule that a `GcRef` is
//! only valid while the object is reachable from the roots.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive};
use rustc_hash::FxHashMap;

use crate::builtins::Primitive;
use crate::sstring::SString;
use crate::value::Value;

/// A reference to a heap object. Compared and hashed by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GcRef(NonNull<GcObject>);

impl GcRef {
    /// The object. Valid while the object is alive: in P0 that is until the
    /// heap is dropped, and from P3, while it is reachable from the roots.
    /// The lifetime is unbounded because the GC, not the borrow checker,
    /// decides how long that is.
    pub fn get<'a>(self) -> &'a GcObject {
        // Safety: see above; the heap owns the allocation and never moves it.
        unsafe { self.0.as_ref() }
    }
}

/// A heap object.
pub struct GcObject {
    /// The GC epoch in which the object was last marked (used from P3).
    pub mark: Cell<u64>,
    pub kind: Obj,
}

/// The kinds of heap object.
pub enum Obj {
    Pair { car: Cell<Value>, cdr: Cell<Value> },
    /// An interned symbol. Its name never changes.
    Symbol(Box<str>),
    Str(RefCell<SString>),
    Vector(Box<[Cell<Value>]>),
    Bytevector(Box<[Cell<u8>]>),
    /// An exact integer outside the fixnum range. Never one that fits.
    BigInt(BigInt),
    /// An exact non-integer, in lowest terms.
    Rational(BigRational),
    /// A top-level binding cell (D7): a global variable. Holds
    /// `Value::UNDEFINED` until the variable is defined.
    Global { name: GcRef, value: Cell<Value> },
    /// A procedure written in Rust.
    Primitive(&'static Primitive),
}

/// Owns every heap object.
#[derive(Default)]
pub struct Heap {
    objects: Vec<GcRef>,
    symbols: FxHashMap<Box<str>, GcRef>,
}

impl Heap {
    pub fn new() -> Heap {
        Heap::default()
    }

    fn alloc(&mut self, kind: Obj) -> Value {
        let boxed = Box::new(GcObject { mark: Cell::new(0), kind });
        let r = GcRef(NonNull::from(Box::leak(boxed)));
        self.objects.push(r);
        Value::obj(r)
    }

    pub fn cons(&mut self, car: Value, cdr: Value) -> Value {
        self.alloc(Obj::Pair { car: Cell::new(car), cdr: Cell::new(cdr) })
    }

    /// The symbol named `name`, the same object every time.
    pub fn intern(&mut self, name: &str) -> Value {
        if let Some(&r) = self.symbols.get(name) {
            return Value::obj(r);
        }
        let v = self.alloc(Obj::Symbol(name.into()));
        self.symbols.insert(name.into(), v.as_ref().unwrap());
        v
    }

    pub fn string(&mut self, s: &str) -> Value {
        self.alloc(Obj::Str(RefCell::new(SString::from(s))))
    }

    pub fn vector(&mut self, items: Vec<Value>) -> Value {
        self.alloc(Obj::Vector(items.into_iter().map(Cell::new).collect()))
    }

    pub fn bytevector(&mut self, bytes: Vec<u8>) -> Value {
        self.alloc(Obj::Bytevector(bytes.into_iter().map(Cell::new).collect()))
    }

    /// An exact integer: a fixnum if it fits, otherwise a bignum. Every
    /// integer therefore has exactly one representation.
    pub fn integer(&mut self, n: BigInt) -> Value {
        match n.to_i64().and_then(Value::fixnum) {
            Some(v) => v,
            None => self.alloc(Obj::BigInt(n)),
        }
    }

    /// An exact rational, as an integer if its denominator is 1.
    pub fn rational(&mut self, r: BigRational) -> Value {
        if r.denom().is_one() {
            self.integer(r.to_integer())
        } else {
            self.alloc(Obj::Rational(r))
        }
    }

    /// A new, unbound binding cell for the symbol `name`.
    pub fn global(&mut self, name: GcRef) -> GcRef {
        let v = self.alloc(Obj::Global { name, value: Cell::new(Value::UNDEFINED) });
        v.as_ref().unwrap()
    }

    pub fn primitive(&mut self, p: &'static Primitive) -> Value {
        self.alloc(Obj::Primitive(p))
    }

    /// The list of `items`.
    pub fn list(&mut self, items: Vec<Value>) -> Value {
        items.into_iter().rev().fold(Value::NIL, |tail, v| self.cons(v, tail))
    }
}

/// The elements of a proper list, or None if `v` is not one.
pub fn list_to_vec(mut v: Value) -> Option<Vec<Value>> {
    let mut items = Vec::new();
    while let Some(Obj::Pair { car, cdr }) = v.as_obj().map(|o| &o.kind) {
        items.push(car.get());
        v = cdr.get();
    }
    v.is_nil().then_some(items)
}

impl Drop for Heap {
    fn drop(&mut self) {
        for r in self.objects.drain(..) {
            // Safety: every GcRef in `objects` came from Box::leak in alloc,
            // and each is freed once. Values outliving the heap dangle,
            // which P0's single long-lived heap never does.
            drop(unsafe { Box::from_raw(r.0.as_ptr()) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{FIXNUM_MAX, FIXNUM_MIN};

    #[test]
    fn integers_normalise() {
        let mut heap = Heap::new();
        assert_eq!(heap.integer(BigInt::from(FIXNUM_MAX)).as_fixnum(), Some(FIXNUM_MAX));
        assert_eq!(heap.integer(BigInt::from(FIXNUM_MIN)).as_fixnum(), Some(FIXNUM_MIN));
        let big = heap.integer(BigInt::from(FIXNUM_MAX) + 1);
        assert!(matches!(big.as_obj().map(|o| &o.kind), Some(Obj::BigInt(_))));
        let six = heap.rational(BigRational::new(BigInt::from(12), BigInt::from(2)));
        assert_eq!(six.as_fixnum(), Some(6));
    }

    #[test]
    fn symbols_are_interned() {
        let mut heap = Heap::new();
        let a = heap.intern("abc");
        let b = heap.intern("abc");
        let c = heap.intern("abd");
        assert!(a.eq(b));
        assert!(!a.eq(c));
    }
}
