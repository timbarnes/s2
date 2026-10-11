//! The dispatch loop (D4): a stack machine with frames (plan/03 §5, §9).
//!
//! ```text
//! value stack:  … │ callee │ arg0 … argN-1 │ let variables │ temporaries │
//!                            ▲ fp
//! frame stack:  Frame { closure, ip, fp }   the caller's state, saved by Call
//! ```
//!
//! - `Call n` on a closure checks the arity, saves the caller's registers
//!   in a `Frame`, and starts the callee with `fp` at its first argument.
//! - `Return` drops the callee's slots (from `fp - 1`, the callee itself),
//!   pushes the result, and restores the caller's registers.
//! - `TailCall n` first moves the callee and its arguments down over the
//!   current frame's slots, then enters the callee *without* saving a
//!   frame. The callee then returns straight to the current procedure's
//!   caller, so a loop written as a tail call runs in constant space.
//!
//! **Safepoints** (plan/03 §4): the VM collects garbage only on entry to
//! `Call` and `TailCall`. Every loop is a tail call, and every allocation
//! happens in a call or between two of them, so these are enough. There,
//! every live value is in the roots `safepoint` passes to the heap.

use crate::builtins::{PrimFn, Primitive};
use crate::globals::{Env, cell_name, cell_value};
use crate::heap::{GcRef, Heap, Obj};
use crate::printer::print_value;
use crate::value::Value;
use crate::vm::op::{Op, Template};

/// A caller's registers, saved while its callee runs.
struct Frame {
    closure: GcRef,
    ip: usize,
    fp: usize,
}

#[derive(Default)]
pub struct Vm {
    stack: Vec<Value>,
    frames: Vec<Frame>,
}

/// The template and free variables of a closure.
fn parts<'a>(closure: GcRef) -> (&'a Template, &'a [Value]) {
    match &closure.get().kind {
        Obj::Closure { template, free } => (template_of(*template), free),
        _ => unreachable!("not a closure"),
    }
}

fn template_of<'a>(t: GcRef) -> &'a Template {
    match &t.get().kind {
        Obj::Template(t) => t,
        _ => unreachable!("not a template"),
    }
}

impl Vm {
    /// The number of frames waiting for a callee to return.
    pub fn frame_depth(&self) -> usize {
        self.frames.len()
    }

    /// The largest the value and frame stacks have been (their capacities,
    /// which never shrink), for tests of tail calls.
    #[cfg(test)]
    pub fn high_water(&self) -> (usize, usize) {
        (self.stack.capacity(), self.frames.capacity())
    }

    /// Run a top-level template (a procedure of no arguments) and return
    /// its value. `env` is the environment it was compiled for, which VM
    /// primitives such as `dis` need. An error aborts the whole form; there
    /// is no `raise` until P7.
    pub fn run(&mut self, heap: &mut Heap, env: &mut Env, template: GcRef) -> Result<Value, String> {
        let closure = heap.closure(template, Box::new([]));
        self.stack.clear();
        self.frames.clear();
        self.stack.push(closure);
        let result = self.execute(heap, env, closure.as_ref().unwrap());
        if result.is_err() {
            self.stack.clear();
            self.frames.clear();
        }
        result
    }

    fn execute(&mut self, heap: &mut Heap, env: &mut Env, mut closure: GcRef) -> Result<Value, String> {
        let (mut t, mut free) = parts(closure);
        let mut ip = 0;
        let mut fp = 1;

        // Return `v` from the running procedure to its caller.
        macro_rules! do_return {
            ($v:expr) => {{
                let v = $v;
                self.stack.truncate(fp - 1);
                match self.frames.pop() {
                    Some(frame) => {
                        closure = frame.closure;
                        (t, free) = parts(closure);
                        ip = frame.ip;
                        fp = frame.fp;
                        self.stack.push(v);
                    }
                    None => return Ok(v),
                }
            }};
        }

        loop {
            let op = t.code[ip];
            ip += 1;
            match op {
                Op::Const(k) => self.stack.push(t.consts[k as usize]),
                Op::Fixnum(n) => self.stack.push(Value::fixnum(n as i64).unwrap()),
                Op::Nil => self.stack.push(Value::NIL),
                Op::True => self.stack.push(Value::TRUE),
                Op::False => self.stack.push(Value::FALSE),
                Op::Unspecified => self.stack.push(Value::UNSPECIFIED),
                Op::LocalRef(i) => self.stack.push(self.stack[fp + i as usize]),
                Op::FreeRef(i) => self.stack.push(free[i as usize]),
                Op::GlobalRef(k) => {
                    let cell = t.consts[k as usize].as_ref().unwrap();
                    let v = cell_value(cell).get();
                    if v.is_undefined() {
                        return Err(format!("unbound variable: {}", print_value(cell_name(cell))));
                    }
                    self.stack.push(v);
                }
                Op::GlobalDefine(k) => {
                    let cell = t.consts[k as usize].as_ref().unwrap();
                    cell_value(cell).set(self.stack.pop().unwrap());
                    self.stack.push(cell_name(cell));
                }
                Op::Jump(target) => ip = target as usize,
                Op::JumpIfFalse(target) => {
                    if !self.stack.pop().unwrap().is_true() {
                        ip = target as usize;
                    }
                }
                Op::Pop => {
                    self.stack.pop();
                }
                Op::Slide(n) => {
                    let v = self.stack.pop().unwrap();
                    self.stack.truncate(self.stack.len() - n as usize);
                    self.stack.push(v);
                }
                Op::MakeClosure(k) => {
                    let template = t.consts[k as usize].as_ref().unwrap();
                    let at = self.stack.len() - template_of(template).nfree as usize;
                    let captured: Box<[Value]> = self.stack.drain(at..).collect();
                    let c = heap.closure(template, captured);
                    self.stack.push(c);
                }
                Op::Call(argc) => {
                    self.safepoint(heap, env, closure);
                    let base = self.stack.len() - argc as usize - 1;
                    let f = self.stack[base];
                    match f.as_obj().map(|o| &o.kind) {
                        Some(Obj::Closure { template, free: captured }) => {
                            let callee = template_of(*template);
                            self.bind_arguments(heap, callee, base)?;
                            self.frames.push(Frame { closure, ip, fp });
                            closure = f.as_ref().unwrap();
                            (t, free) = (callee, captured);
                            ip = 0;
                            fp = base + 1;
                        }
                        Some(Obj::Primitive(p)) => {
                            let result = self.call_primitive(heap, env, p, base)?;
                            self.stack.truncate(base);
                            self.stack.push(result);
                        }
                        _ => return Err(format!("not a procedure: {}", print_value(f))),
                    }
                }
                Op::TailCall(argc) => {
                    self.safepoint(heap, env, closure);
                    let base = self.stack.len() - argc as usize - 1;
                    let f = self.stack[base];
                    match f.as_obj().map(|o| &o.kind) {
                        Some(Obj::Closure { template, free: captured }) => {
                            // Slide the callee and its arguments down to
                            // where the current procedure's callee slot is.
                            self.stack.drain(fp - 1..base);
                            let callee = template_of(*template);
                            self.bind_arguments(heap, callee, fp - 1)?;
                            closure = f.as_ref().unwrap();
                            (t, free) = (callee, captured);
                            ip = 0;
                        }
                        Some(Obj::Primitive(p)) => {
                            let result = self.call_primitive(heap, env, p, base)?;
                            do_return!(result);
                        }
                        _ => return Err(format!("not a procedure: {}", print_value(f))),
                    }
                }
                Op::Return => do_return!(self.stack.pop().unwrap()),
            }
        }
    }

    /// Collect garbage if a collection is due. The roots are the value
    /// stack, the closures saved in frames, the running closure, and the
    /// environment's binding cells (the heap adds the symbol table).
    #[inline]
    fn safepoint(&self, heap: &mut Heap, env: &Env, closure: GcRef) {
        if heap.needs_gc() {
            let frames = self.frames.iter().map(|f| Value::obj(f.closure));
            heap.collect(self.stack.iter().copied().chain(frames).chain([Value::obj(closure)]).chain(env.roots()));
        }
    }

    /// Check the argument count for a call to `callee`, whose closure is in
    /// slot `base`, and collect any rest arguments into a list.
    fn bind_arguments(&mut self, heap: &mut Heap, callee: &Template, base: usize) -> Result<(), String> {
        let argc = self.stack.len() - base - 1;
        let required = callee.required as usize;
        if argc < required || (!callee.rest && argc > required) {
            let name = if callee.name.is_empty() { "#<procedure>" } else { &callee.name };
            let expected = if callee.rest { format!("at least {}", required) } else { required.to_string() };
            return Err(format!("{}: wrong number of arguments: {} (expected {})", name, argc, expected));
        }
        if callee.rest {
            let extra = self.stack.split_off(base + 1 + required);
            let list = heap.list(extra);
            self.stack.push(list);
        }
        Ok(())
    }

    /// Call primitive `p`, whose arguments are above slot `base`.
    fn call_primitive(&mut self, heap: &mut Heap, env: &mut Env, p: &Primitive, base: usize) -> Result<Value, String> {
        let n = self.stack.len() - base - 1;
        if n < p.min || p.max.is_some_and(|max| n > max) {
            return Err(format!("{}: wrong number of arguments: {}", p.name, n));
        }
        match p.f {
            PrimFn::Builtin(f) => f(heap, &self.stack[base + 1..]),
            PrimFn::Vm(f) => {
                let args = self.stack[base + 1..].to_vec();
                f(self, heap, env, &args)
            }
        }
    }
}
