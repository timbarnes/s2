//! Top-level environments and their binding cells (D7, plan/03 §8).
//!
//! An environment maps each symbol to a binding cell, a heap object holding
//! the variable's value. The compiler puts the cell itself into a template's
//! constant pool, so a global reference at run time is one load from the
//! cell, with no lookup by name. Redefining a variable changes the same
//! cell, so code compiled earlier sees the new value (late binding).

use rustc_hash::FxHashMap;

use crate::heap::{GcRef, Heap, Obj};
use crate::value::Value;

/// A top-level environment. P1 has exactly one; libraries (P8) each get
/// their own.
#[derive(Default)]
pub struct Env {
    cells: FxHashMap<GcRef, GcRef>,
}

impl Env {
    /// The cell for the symbol `name`, created unbound if it doesn't exist
    /// yet. Compiling a reference to a variable that is defined later (or
    /// never) therefore works; the error comes when the code runs.
    pub fn cell(&mut self, heap: &mut Heap, name: GcRef) -> GcRef {
        *self.cells.entry(name).or_insert_with(|| heap.global(name))
    }

    /// The binding cells, which are GC roots.
    pub fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        self.cells.values().map(|&c| Value::obj(c))
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

/// The symbol a binding cell belongs to.
pub fn cell_name(cell: GcRef) -> Value {
    match &cell.get().kind {
        Obj::Global { name, .. } => Value::obj(*name),
        _ => unreachable!("not a binding cell"),
    }
}
