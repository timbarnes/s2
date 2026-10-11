//! Top-level environments and their binding cells (D7, plan/03 §8).
//!
//! An environment maps each top-level identifier to what it means: a
//! variable, a macro, or a core form of the expander (plan/03 §12), so a
//! program can redefine `if` or define a macro named like a procedure.
//!
//! A variable is a binding cell, a heap object holding the variable's
//! value. The compiler puts the cell itself into a template's constant
//! pool, so a global reference at run time is one load from the cell, with
//! no lookup by name. Redefining a variable changes the same cell, so code
//! compiled earlier sees the new value (late binding).
//!
//! The keys are identifiers: usually symbols, but a top-level definition
//! introduced by a macro expansion is keyed by its alias, so only that
//! expansion can see it (as in s1).

use rustc_hash::FxHashMap;

use crate::compile::expand::{CORE_FORMS, Core};
use crate::heap::{GcRef, Heap, Obj};
use crate::value::Value;

#[derive(Clone, Copy)]
pub enum TopBinding {
    /// A variable: its binding cell.
    Variable(GcRef),
    /// A macro: an `Obj::Macro`.
    Macro(GcRef),
    Core(Core),
}

/// A top-level environment. P1-P7 have exactly one; libraries (P8) each get
/// their own.
pub struct Env {
    bindings: FxHashMap<GcRef, TopBinding>,
}

impl Env {
    /// An environment with the core forms bound.
    pub fn new(heap: &mut Heap) -> Env {
        let mut env = Env { bindings: FxHashMap::default() };
        for &(name, core) in CORE_FORMS {
            let sym = heap.intern(name).as_ref().unwrap();
            env.bindings.insert(sym, TopBinding::Core(core));
        }
        env
    }

    pub fn lookup(&self, id: GcRef) -> Option<TopBinding> {
        self.bindings.get(&id).copied()
    }

    /// The cell for the variable `name`, created unbound if `name` isn't a
    /// variable yet. Compiling a reference to a variable that is defined
    /// later (or never) therefore works; the error comes when the code
    /// runs. A macro or core form of that name is replaced: this is how
    /// `define` redefines one.
    pub fn cell(&mut self, heap: &mut Heap, name: GcRef) -> GcRef {
        match self.bindings.get(&name) {
            Some(&TopBinding::Variable(cell)) => cell,
            _ => {
                let cell = heap.global(name);
                self.bindings.insert(name, TopBinding::Variable(cell));
                cell
            }
        }
    }

    /// Bind `name` to the macro `m`, as a top-level `define-syntax` does.
    pub fn define_syntax(&mut self, name: GcRef, m: GcRef) {
        self.bindings.insert(name, TopBinding::Macro(m));
    }

    /// The GC roots: every key (aliases among them) and every cell and
    /// macro.
    pub fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        self.bindings.iter().flat_map(|(&k, b)| {
            let v = match *b {
                TopBinding::Variable(r) | TopBinding::Macro(r) => Value::obj(r),
                TopBinding::Core(_) => Value::NIL,
            };
            [Value::obj(k), v]
        })
    }

    /// Bind `name` to `value`, as a top-level `define` does.
    pub fn define(&mut self, heap: &mut Heap, name: &str, value: Value) {
        let sym = heap.intern(name).as_ref().unwrap();
        cell_value(self.cell(heap, sym)).set(value);
    }
}

/// The value slot of a binding cell.
pub fn cell_value<'a>(cell: GcRef) -> &'a std::cell::Cell<Value> {
    match &cell.get().kind {
        Obj::Global { value, .. } => value,
        _ => unreachable!("not a binding cell"),
    }
}

/// The identifier a binding cell belongs to.
pub fn cell_name(cell: GcRef) -> Value {
    match &cell.get().kind {
        Obj::Global { name, .. } => Value::obj(*name),
        _ => unreachable!("not a binding cell"),
    }
}
