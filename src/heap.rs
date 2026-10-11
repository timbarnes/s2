//! The heap: object layout, allocation and the symbol table (plan/03 §3-4).
//!
//! Every object is its own `Box`, and the heap keeps a list of them. Objects
//! never move, so a `GcRef` stays valid for as long as the object is alive.
//! Mutable fields are `Cell`s (or a `RefCell` for strings), so all access is
//! through shared references, unlike s1's `get_value_mut(&self)`.
//!
//! Garbage collection is non-moving mark-sweep (D3), adapted from s1:
//!
//! - **Epoch marks.** Each collection bumps `epoch`, and an object is marked
//!   when its `mark` equals it, so there is no unmark pass.
//! - **An iterative worklist**, so deep structures don't overflow the Rust
//!   stack.
//! - **Scheduling in proportion to the live heap** (`needs_gc`).
//! - **Allocation never collects.** The VM calls `collect` at its
//!   safepoints, passing the roots it holds; the heap adds the symbol table
//!   (a root, not weak: the simplest choice). A `GcRef` is valid while its
//!   object is reachable from the roots at every safepoint.
//!
//! Two debugging modes, read from the environment by `Heap::new`:
//! `S2_GC_STRESS` collects at every safepoint, and `S2_GC_POISON` turns
//! freed objects into the symbol `<<FREED>>` (and leaks them) instead of
//! freeing them, so a premature free shows up in output.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive};
use rustc_hash::FxHashMap;

use crate::builtins::Primitive;
use crate::compile::syntax_rules::SyntaxRules;
use crate::io::PortKind;
use crate::sstring::SString;
use crate::value::Value;
use crate::vm::Template;

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
    /// The GC epoch in which the object was last marked.
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
    /// An identifier introduced by a macro expansion (plan/03 §12): it
    /// renames `original` (a symbol, or another alias), and if nothing in
    /// the expansion binds it, it means what `original` meant where the
    /// macro was defined: in the first `depth` of the expander's scope
    /// frames (0 for a top-level macro). Prints as its symbol's name.
    Alias { original: Value, depth: u32 },
    /// A `syntax-rules` transformer, bound by `define-syntax`,
    /// `let-syntax` or `letrec-syntax`. Exists only at compile time.
    Macro(SyntaxRules),
    /// A variable that is both captured and assigned, or used before its
    /// `letrec` init has run (03 §7, `analyze::boxed_variables`). Closures
    /// copy the box, so they share the variable. Never seen by Scheme code.
    Box(Cell<Value>),
    /// An input or output port (R7RS §6.13). Port procedures change it in
    /// place, so every reference sees the same position and state.
    Port(RefCell<PortKind>),
    /// A record type made by `define-record-type`: its name, and its field
    /// names in order.
    RecordType { name: Value, fields: Box<[Value]> },
    /// An instance of a record type.
    Record { rtype: GcRef, fields: Box<[Cell<Value>]> },
    /// A procedure written in Rust.
    Primitive(&'static Primitive),
    /// Compiled code (`vm/op.rs`). Templates are heap objects so that a
    /// template's constant pool can hold the templates of its inner lambdas.
    Template(Template),
    /// A procedure written in Scheme: a template plus the values of its free
    /// variables, copied when the closure was made (a flat closure, D6).
    Closure { template: GcRef, free: Box<[Value]> },
}

/// The minimum number of allocations between collections (s1's default).
const GC_THRESHOLD: usize = 20_000;

/// Owns every heap object.
pub struct Heap {
    objects: Vec<GcRef>,
    symbols: FxHashMap<Box<str>, GcRef>,
    /// Allocations since the last collection.
    allocations: usize,
    /// How many objects survived the last collection.
    live_after_gc: usize,
    epoch: u64,
    /// The mark worklist, kept between collections to reuse its capacity.
    worklist: Vec<GcRef>,
    /// Collect at every safepoint.
    pub stress: bool,
    /// Poison freed objects instead of freeing them.
    pub poison: bool,
    /// Statistics, for tests: collections run, and objects marked by all
    /// of them together.
    pub collections: usize,
    pub marked: usize,
}

impl Heap {
    pub fn new() -> Heap {
        Heap {
            objects: Vec::new(),
            symbols: FxHashMap::default(),
            allocations: 0,
            live_after_gc: 0,
            epoch: 0,
            worklist: Vec::new(),
            stress: std::env::var_os("S2_GC_STRESS").is_some(),
            poison: std::env::var_os("S2_GC_POISON").is_some(),
            collections: 0,
            marked: 0,
        }
    }

    fn alloc(&mut self, kind: Obj) -> Value {
        self.allocations += 1;
        let boxed = Box::new(GcObject { mark: Cell::new(0), kind });
        let r = GcRef(NonNull::from(Box::leak(boxed)));
        self.objects.push(r);
        Value::obj(r)
    }

    /// The number of objects in the heap, live or not yet collected.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Whether the next safepoint should collect: in stress mode always;
    /// otherwise once there have been more allocations than the threshold
    /// *and* than the objects that survived the last collection. The second
    /// condition keeps collection time proportional to allocation, however
    /// large the live heap grows. With a fixed interval, a computation whose
    /// live data keeps growing (a deep non-tail recursion) re-marks all of
    /// it every interval, which is quadratic.
    #[inline]
    pub fn needs_gc(&self) -> bool {
        self.stress || self.allocations > GC_THRESHOLD.max(self.live_after_gc)
    }

    /// Collect: mark everything reachable from `roots` and the symbol
    /// table, then free the rest.
    pub fn collect(&mut self, roots: impl IntoIterator<Item = Value>) {
        self.epoch += 1;
        let mut m = Marker { epoch: self.epoch, grey: std::mem::take(&mut self.worklist), count: 0 };
        roots.into_iter().for_each(|v| m.value(v));
        self.symbols.values().for_each(|&r| m.obj(r));
        m.trace();
        self.worklist = m.grey;
        self.marked += m.count;
        self.collections += 1;

        let (epoch, poison) = (self.epoch, self.poison);
        self.objects.retain(|&r| {
            let live = r.get().mark.get() == epoch;
            if !live {
                let p = r.0.as_ptr();
                // Safety: the object is unreachable from the roots, so
                // nothing that will be used again refers to it. (A stray
                // reference is exactly the bug poison mode exists to show.)
                unsafe {
                    if poison {
                        (*p).kind = Obj::Symbol("<<FREED>>".into());
                    } else {
                        drop(Box::from_raw(p));
                    }
                }
            }
            live
        });
        self.allocations = 0;
        self.live_after_gc = self.objects.len();
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

    pub fn template(&mut self, t: Template) -> GcRef {
        self.alloc(Obj::Template(t)).as_ref().unwrap()
    }

    pub fn alias(&mut self, original: Value, depth: u32) -> Value {
        self.alloc(Obj::Alias { original, depth })
    }

    pub fn make_macro(&mut self, rules: SyntaxRules) -> GcRef {
        self.alloc(Obj::Macro(rules)).as_ref().unwrap()
    }

    pub fn port(&mut self, port: PortKind) -> Value {
        self.alloc(Obj::Port(RefCell::new(port)))
    }

    pub fn record_type(&mut self, name: Value, fields: Vec<Value>) -> Value {
        self.alloc(Obj::RecordType { name, fields: fields.into() })
    }

    pub fn record(&mut self, rtype: GcRef, fields: Vec<Value>) -> Value {
        self.alloc(Obj::Record { rtype, fields: fields.into_iter().map(Cell::new).collect() })
    }

    pub fn make_box(&mut self, v: Value) -> Value {
        self.alloc(Obj::Box(Cell::new(v)))
    }

    pub fn closure(&mut self, template: GcRef, free: Box<[Value]>) -> Value {
        self.alloc(Obj::Closure { template, free })
    }

    /// The list of `items`.
    pub fn list(&mut self, items: Vec<Value>) -> Value {
        items.into_iter().rev().fold(Value::NIL, |tail, v| self.cons(v, tail))
    }
}

/// The mark phase of one collection. An object is marked when it is pushed
/// on `grey`, so an object reachable by several paths is traced once.
struct Marker {
    epoch: u64,
    grey: Vec<GcRef>,
    count: usize,
}

impl Marker {
    fn value(&mut self, v: Value) {
        if let Some(r) = v.as_ref() {
            self.obj(r);
        }
    }

    fn obj(&mut self, r: GcRef) {
        let mark = &r.get().mark;
        if mark.get() != self.epoch {
            mark.set(self.epoch);
            self.count += 1;
            self.grey.push(r);
        }
    }

    /// Mark everything reachable from the grey objects. The match has no
    /// `_` arm, so a new kind of object must say what it refers to.
    fn trace(&mut self) {
        while let Some(r) = self.grey.pop() {
            match &r.get().kind {
                Obj::Pair { car, cdr } => {
                    self.value(car.get());
                    self.value(cdr.get());
                }
                Obj::Vector(items) => items.iter().for_each(|c| self.value(c.get())),
                Obj::Box(value) => self.value(value.get()),
                Obj::RecordType { name, fields } => {
                    self.value(*name);
                    fields.iter().for_each(|&f| self.value(f));
                }
                Obj::Record { rtype, fields } => {
                    self.obj(*rtype);
                    fields.iter().for_each(|f| self.value(f.get()));
                }
                Obj::Alias { original, .. } => self.value(*original),
                Obj::Macro(m) => {
                    m.ellipsis.iter().chain(&m.literals).for_each(|&r| self.obj(r));
                    m.rules.iter().for_each(|&(p, t)| {
                        self.value(p);
                        self.value(t);
                    });
                }
                Obj::Global { name, value } => {
                    self.obj(*name);
                    self.value(value.get());
                }
                Obj::Template(t) => t.consts.iter().for_each(|&k| self.value(k)),
                Obj::Closure { template, free } => {
                    self.obj(*template);
                    free.iter().for_each(|&v| self.value(v));
                }
                Obj::Symbol(_) | Obj::Str(_) | Obj::Bytevector(_) | Obj::BigInt(_) | Obj::Rational(_) | Obj::Primitive(_) | Obj::Port(_) => {}
            }
        }
    }
}

/// `eqv?`: `eq?`, or numbers with the same exactness and value.
pub fn eqv(a: Value, b: Value) -> bool {
    a.eq(b)
        || match (a.as_obj().map(|o| &o.kind), b.as_obj().map(|o| &o.kind)) {
            (Some(Obj::BigInt(x)), Some(Obj::BigInt(y))) => x == y,
            (Some(Obj::Rational(x)), Some(Obj::Rational(y))) => x == y,
            _ => false,
        }
}

/// `equal?`: `eqv?`, or pairs, strings, vectors and bytevectors with equal
/// contents. Terminates on cyclic data, as R7RS requires: after many steps
/// it starts recording which pairs of nodes are being compared, and treats
/// meeting one again as equal (that comparison is already under way higher
/// up). Forked from s1's `gc::equal`.
pub fn equal(a: Value, b: Value) -> bool {
    equal_with(a, b, &mut EqualState { steps: 0, seen: None })
}

/// Cycle detection for `equal`.
struct EqualState {
    /// Node pairs compared so far.
    steps: usize,
    /// Node pairs under comparison; only kept once `steps` is large.
    seen: Option<rustc_hash::FxHashSet<(GcRef, GcRef)>>,
}

/// Steps after which `equal` starts tracking visited node pairs.
const EQUAL_TRACKING_AFTER: usize = 10_000;

impl EqualState {
    /// Note a visit to the node pair (a, b); false if it was seen before.
    fn first_visit(&mut self, a: Value, b: Value) -> bool {
        self.steps += 1;
        if self.steps > EQUAL_TRACKING_AFTER {
            let seen = self.seen.get_or_insert_with(Default::default);
            return seen.insert((a.as_ref().unwrap(), b.as_ref().unwrap()));
        }
        true
    }
}

/// `equal` with explicit state; loops on the cdr so long lists don't
/// recurse.
fn equal_with(mut a: Value, mut b: Value, st: &mut EqualState) -> bool {
    loop {
        match (a.as_obj().map(|o| &o.kind), b.as_obj().map(|o| &o.kind)) {
            (Some(Obj::Pair { car: a1, cdr: d1 }), Some(Obj::Pair { car: a2, cdr: d2 })) => {
                if !st.first_visit(a, b) {
                    return true;
                }
                if !equal_with(a1.get(), a2.get(), st) {
                    return false;
                }
                a = d1.get();
                b = d2.get();
            }
            (Some(Obj::Vector(x)), Some(Obj::Vector(y))) => {
                if x.len() != y.len() {
                    return false;
                }
                if !st.first_visit(a, b) {
                    return true;
                }
                return x.iter().zip(y.iter()).all(|(p, q)| equal_with(p.get(), q.get(), st));
            }
            (Some(Obj::Str(x)), Some(Obj::Str(y))) => return *x.borrow() == *y.borrow(),
            (Some(Obj::Bytevector(x)), Some(Obj::Bytevector(y))) => return x == y,
            _ => return eqv(a, b),
        }
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
            // and each is freed once (sweep removes what it frees). Values
            // outliving the heap dangle, which s2's single long-lived heap
            // never does.
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

    #[test]
    fn collect_frees_only_the_unreachable() {
        let mut heap = Heap::new();
        let sym = heap.intern("s");
        let kept = heap.list(vec![sym, Value::fixnum(1).unwrap()]);
        let cycle = heap.cons(Value::NIL, Value::NIL);
        if let Some(Obj::Pair { cdr, .. }) = cycle.as_obj().map(|o| &o.kind) {
            cdr.set(cycle);
        }
        heap.string("garbage");
        heap.collect([kept]);
        // The symbol (a root), the two pairs of `kept`; the cycle and the
        // string are gone.
        assert_eq!(heap.len(), 3);
        assert_eq!(crate::printer::print_value(kept), "(s 1)");
    }
}
