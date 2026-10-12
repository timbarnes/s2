//! The value stack: one allocation that never moves or grows, so the
//! dispatch loop can keep the stack pointer in a local variable (a
//! register) and write it back only where something else reads the stack.
//! A `Vec`'s length lives in memory, and every push checks its capacity.
//!
//! The allocation is zeroed, so the system maps its pages lazily: a
//! program pays only for the depth it reaches. Zero bits are a valid
//! `Value` (the flonum 0.0).

use std::alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error};
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;

use crate::value::Value;

/// The default size, in values: 512MB of address space. Deep non-tail
/// recursion is ordinary Scheme; this allows about 20 million levels of a
/// simple procedure.
pub const SIZE: usize = 1 << 26;

/// Slots kept free above the dispatch loop's limit, for the few values
/// the slow paths push without checking (raising an error, `eval`).
pub const HEADROOM: usize = 1024;

pub struct Stack {
    ptr: NonNull<Value>,
    size: usize,
    len: usize,
    /// The highest `len` has been, for tests of tail calls.
    high: usize,
}

impl Stack {
    pub fn new(size: usize) -> Stack {
        let layout = Layout::array::<Value>(size).unwrap();
        // SAFETY: the layout isn't zero-sized, and all-zero bits are a
        // valid Value.
        let ptr = NonNull::new(unsafe { alloc_zeroed(layout) } as *mut Value).unwrap_or_else(|| handle_alloc_error(layout));
        Stack { ptr, size, len: 0, high: 0 }
    }

    /// The first slot, for the dispatch loop's direct access.
    pub fn base(&self) -> *mut Value {
        self.ptr.as_ptr()
    }

    /// How deep the dispatch loop may push before it reports an overflow.
    pub fn limit(&self) -> usize {
        self.size - HEADROOM
    }

    #[cfg(test)]
    pub fn high(&self) -> usize {
        self.high
    }

    /// Start measuring `high` again from here.
    #[cfg(test)]
    pub fn reset_high(&mut self) {
        self.high = self.len;
    }

    pub fn set_len(&mut self, len: usize) {
        assert!(len <= self.size, "stack overflow");
        self.len = len;
        self.high = self.high.max(len);
    }

    pub fn push(&mut self, v: Value) {
        let len = self.len;
        self.set_len(len + 1);
        self[len] = v;
    }

    pub fn pop(&mut self) -> Option<Value> {
        let v = self.last().copied()?;
        self.len -= 1;
        Some(v)
    }

    pub fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Remove the slots `from..to`, moving those above them down.
    pub fn remove_range(&mut self, from: usize, to: usize) {
        self.copy_within(to.., from);
        self.len -= to - from;
    }

    pub fn remove(&mut self, i: usize) -> Value {
        let v = self[i];
        self.remove_range(i, i + 1);
        v
    }

    pub fn insert(&mut self, i: usize, v: Value) {
        self.push(v);
        self[i..].rotate_right(1);
    }

    /// Remove and return the slots from `at` up.
    pub fn split_off(&mut self, at: usize) -> Vec<Value> {
        let v = self[at..].to_vec();
        self.len = at;
        v
    }

    pub fn extend_from_slice(&mut self, values: &[Value]) {
        let len = self.len;
        self.set_len(len + values.len());
        self[len..].copy_from_slice(values);
    }
}

impl Deref for Stack {
    type Target = [Value];

    fn deref(&self) -> &[Value] {
        // SAFETY: the first `len` slots are inside the allocation and
        // initialised (it was zeroed).
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl DerefMut for Stack {
    fn deref_mut(&mut self) -> &mut [Value] {
        // SAFETY: as for `deref`, and `&mut self` makes this the only
        // reference.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        // SAFETY: allocated in `new` with this layout.
        unsafe { dealloc(self.ptr.as_ptr() as *mut u8, Layout::array::<Value>(self.size).unwrap()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behaves_like_a_vec() {
        let n = |i: i64| Value::fixnum(i).unwrap();
        let mut s = Stack::new(2048);
        s.extend_from_slice(&[n(1), n(2), n(3), n(4)]);
        s.remove_range(1, 3);
        s.insert(1, n(9));
        assert_eq!(s.iter().map(|v| v.as_fixnum().unwrap()).collect::<Vec<_>>(), [1, 9, 4]);
        assert_eq!(s.remove(0).as_fixnum(), Some(1));
        assert_eq!(s.split_off(1).len(), 1);
        assert_eq!(s.pop().unwrap().as_fixnum(), Some(9));
        assert!(s.pop().is_none());
        assert_eq!((s.high(), s.limit()), (4, 1024));
    }
}
