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
//!
//! **Control** (plan/03 §10, D8). A few primitives are carried out by the
//! call instructions themselves, as they rearrange the stacks:
//!
//! - `apply` spreads its last argument and calls the procedure;
//! - `%call/cc` copies both stacks into a `Continuation` and calls its
//!   argument with it. Calling the continuation copies them back and
//!   returns its argument to the topmost saved frame;
//! - `%call/ec` records only the stack heights and the frame to return
//!   into (an `Escape`). Calling it cuts the stacks back, if that frame is
//!   still there.
//!
//! These are raw: `dynamic-wind`, the user's `call/cc` and exceptions are
//! written in Scheme on top of them (`scheme/core.scm`).
//!
//! - `eval` compiles its expression for its environment and calls the
//!   result in place of itself, so `eval` in tail position is a tail call.
//!
//! **Errors are raised.** When an instruction or a builtin fails, the VM
//! makes an error object and calls the Scheme procedure `%raise-error`
//! with it, as if the failing instruction had been that call. So `guard`
//! catches errors from builtins, unbound variables and bad calls alike.
//! Only an error nobody handles comes out of `run`, through `%abort`.

use crate::builtins::{Error, ErrorKind, PrimFn, Primitive, standard_ports};
use crate::compile;
use crate::globals::{TopBinding, cell_name, cell_value, env_of};
use crate::heap::{GcRef, Heap, Obj, list_to_vec};
use crate::printer::{display_value, print_value};
use crate::value::Value;
use crate::vm::op::{Op, Template};

/// A caller's registers, saved while its callee runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub closure: GcRef,
    ip: usize,
    fp: usize,
}

pub struct Vm {
    stack: Vec<Value>,
    frames: Vec<Frame>,
    /// The current input, output and error ports (`builtins::port`).
    pub ports: [Value; 3],
    /// The names of the procedures that were waiting when the last uncaught
    /// error happened, innermost first.
    pub backtrace: Vec<String>,
    /// How many full continuations `%call/cc` has captured: a statistic,
    /// for tests that escapes don't need them.
    pub captures: usize,
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

fn box_contents<'a>(b: Value) -> &'a std::cell::Cell<Value> {
    match b.as_obj().map(|o| &o.kind) {
        Some(Obj::Box(v)) => v,
        _ => unreachable!("not a box"),
    }
}

/// The name of the variable whose box the `BoxRef` just executed (at
/// `ip - 1`) found undefined. Codegen always emits `BoxRef` straight after
/// the `LocalRef` or `FreeRef` that loads the box, so the name is that
/// instruction's.
fn boxed_name(t: &Template, ip: usize) -> &str {
    match t.code[ip - 2] {
        Op::LocalRef(i) => &t.local_names[i as usize],
        Op::FreeRef(i) => &t.free_names[i as usize],
        _ => unreachable!("BoxRef not after a load"),
    }
}

/// The text of an error that nobody handled: an error object's message
/// and irritants, or any other raised object.
pub fn uncaught_message(obj: Value) -> String {
    match obj.as_obj().map(|o| &o.kind) {
        Some(Obj::Error { message, irritants, .. }) => {
            let mut text = display_value(*message);
            for irritant in list_to_vec(*irritants).unwrap_or_default() {
                text.push(' ');
                text.push_str(&print_value(irritant));
            }
            text
        }
        _ => format!("uncaught exception: {}", print_value(obj)),
    }
}

impl Vm {
    pub fn new(heap: &mut Heap) -> Vm {
        Vm { stack: Vec::new(), frames: Vec::new(), ports: standard_ports(heap), backtrace: Vec::new(), captures: 0 }
    }

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

    /// The names of the procedures in the frame stack, innermost first,
    /// leaving out anonymous ones and those of the exception machinery.
    pub fn record_backtrace(&mut self) {
        self.backtrace = self
            .frames
            .iter()
            .rev()
            .map(|f| parts(f.closure).0.name.to_string())
            .filter(|n| !n.is_empty() && !n.starts_with('%') && !matches!(n.as_str(), "raise" | "raise-continuable" | "error" | "top-level"))
            .collect();
    }

    /// Run a top-level template (a procedure of no arguments) and return
    /// its value. An error nobody handles aborts the whole form.
    pub fn run(&mut self, heap: &mut Heap, template: GcRef) -> Result<Value, String> {
        let closure = heap.closure(template, Box::new([]));
        self.stack.clear();
        self.frames.clear();
        self.backtrace.clear();
        self.stack.push(closure);
        let result = self.execute(heap, closure.as_ref().unwrap());
        if result.is_err() {
            self.stack.clear();
            self.frames.clear();
        }
        result
    }

    fn execute(&mut self, heap: &mut Heap, mut closure: GcRef) -> Result<Value, String> {
        let (mut t, mut free) = parts(closure);
        let mut ip = 0;
        let mut fp = 1;

        // Continue in the topmost frame, with `v` as the value returned to
        // it; with no frames left, the top-level form is finished.
        macro_rules! resume {
            ($v:expr) => {{
                let v = $v;
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

        // Return `v` from the running procedure to its caller.
        macro_rules! do_return {
            ($v:expr) => {{
                let v = $v;
                self.stack.truncate(fp - 1);
                resume!(v);
            }};
        }

        // Raise the error `e` here: call `%raise-error` with an error
        // object, saving this frame (for the backtrace; a raise never
        // returns). Before `scheme/core.scm` has defined `%raise-error`,
        // the error just aborts the form. `$run` is the label of the
        // instruction loop (labels in macros are hygienic, so each use names
        // it).
        macro_rules! fail {
            ($run:lifetime, $e:expr) => {{
                let e: Error = $e.into();
                let Some(handler) = (e.kind != ErrorKind::Uncaught).then(|| system_procedure(heap, "%raise-error")).flatten() else {
                    return Err(e.message);
                };
                let message = heap.string(&e.message);
                let irritants = heap.list(e.irritants);
                let obj = heap.error_object(e.kind, message, irritants);
                self.frames.push(Frame { closure, ip, fp });
                let base = self.stack.len();
                self.stack.push(handler);
                self.stack.push(obj);
                closure = handler.as_ref().unwrap();
                (t, free) = parts(closure);
                ip = 0;
                fp = base + 1;
                continue $run;
            }};
        }

        'run: loop {
            let op = t.code[ip];
            ip += 1;
            match op {
                Op::Const(k) => self.stack.push(t.consts[k as usize]),
                Op::Fixnum(n) => self.stack.push(Value::fixnum(n as i64).unwrap()),
                Op::Nil => self.stack.push(Value::NIL),
                Op::True => self.stack.push(Value::TRUE),
                Op::False => self.stack.push(Value::FALSE),
                Op::Unspecified => self.stack.push(Value::UNSPECIFIED),
                Op::Undefined => self.stack.push(Value::UNDEFINED),
                Op::LocalRef(i) => self.stack.push(self.stack[fp + i as usize]),
                Op::LocalSet(i) => self.stack[fp + i as usize] = self.stack.pop().unwrap(),
                Op::FreeRef(i) => self.stack.push(free[i as usize]),
                Op::MakeBox(i) => {
                    let slot = fp + i as usize;
                    self.stack[slot] = heap.make_box(self.stack[slot]);
                }
                Op::BoxRef => {
                    let v = box_contents(self.stack.pop().unwrap()).get();
                    if v.is_undefined() {
                        fail!('run, format!("variable used before its initialisation: {}", boxed_name(t, ip)));
                    }
                    self.stack.push(v);
                }
                Op::BoxSet => {
                    let v = self.stack.pop().unwrap();
                    box_contents(self.stack.pop().unwrap()).set(v);
                }
                Op::GlobalRef(k) => {
                    let cell = t.consts[k as usize].as_ref().unwrap();
                    let v = cell_value(cell).get();
                    if v.is_undefined() {
                        fail!('run, format!("unbound variable: {}", print_value(cell_name(cell))));
                    }
                    self.stack.push(v);
                }
                Op::GlobalSet(k) => {
                    let cell = t.consts[k as usize].as_ref().unwrap();
                    if cell_value(cell).get().is_undefined() {
                        fail!('run, format!("set!: unbound variable: {}", print_value(cell_name(cell))));
                    }
                    cell_value(cell).set(self.stack.pop().unwrap());
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
                Op::Call(argc) | Op::TailCall(argc) => {
                    let tail = matches!(op, Op::TailCall(_));
                    self.safepoint(heap, closure);
                    let base = self.stack.len() - argc as usize - 1;
                    // A loop, because apply, %call/cc and %call/ec rewrite
                    // the call into another one.
                    loop {
                        let f = self.stack[base];
                        match f.as_obj().map(|o| &o.kind) {
                            Some(Obj::Closure { template, free: captured }) => {
                                let callee = template_of(*template);
                                let base = if tail {
                                    // Slide the callee and its arguments down to
                                    // where the current procedure's callee slot is.
                                    self.stack.drain(fp - 1..base);
                                    fp - 1
                                } else {
                                    base
                                };
                                if let Err(e) = self.bind_arguments(heap, callee, base) {
                                    fail!('run, e);
                                }
                                if !tail {
                                    self.frames.push(Frame { closure, ip, fp });
                                    fp = base + 1;
                                }
                                closure = f.as_ref().unwrap();
                                (t, free) = (callee, captured);
                                ip = 0;
                            }
                            Some(Obj::Primitive(p)) => match p.f {
                                PrimFn::Apply => {
                                    if let Err(e) = self.spread_apply(base) {
                                        fail!('run, e);
                                    }
                                    continue;
                                }
                                PrimFn::CallCc | PrimFn::CallEc => {
                                    let n = self.stack.len() - base - 1;
                                    if n != 1 {
                                        fail!('run, format!("{}: wrong number of arguments: {}", p.name, n));
                                    }
                                    let here = Frame { closure, ip, fp };
                                    let k = if matches!(p.f, PrimFn::CallCc) {
                                        self.capture(heap, tail, base, here)
                                    } else {
                                        self.capture_escape(heap, tail, base, here)
                                    };
                                    // Now call the argument with the continuation.
                                    self.stack[base] = self.stack[base + 1];
                                    self.stack[base + 1] = k;
                                    continue;
                                }
                                PrimFn::Eval => {
                                    let n = self.stack.len() - base - 1;
                                    if n != 2 {
                                        fail!('run, format!("eval: wrong number of arguments: {}", n));
                                    }
                                    let (expr, env) = (self.stack[base + 1], self.stack[base + 2]);
                                    if !matches!(env.as_obj().map(|o| &o.kind), Some(Obj::Environment(_))) {
                                        fail!('run, "eval: the second argument must be an environment");
                                    }
                                    match compile::compile(heap, env.as_ref().unwrap(), expr) {
                                        Ok(template) => {
                                            // Call the compiled thunk in place of eval.
                                            let thunk = heap.closure(template, Box::new([]));
                                            self.stack.truncate(base);
                                            self.stack.push(thunk);
                                        }
                                        // An import needs a library file loaded first:
                                        // call (%load-then-eval "file" expr env) instead.
                                        Err(e) => match heap.libraries.pending_load.take() {
                                            Some(path) => match system_procedure(heap, "%load-then-eval") {
                                                Some(f) => {
                                                    let path = heap.string(&path.to_string_lossy());
                                                    self.stack[base] = f;
                                                    self.stack.insert(base + 1, path);
                                                }
                                                None => fail!('run, e),
                                            },
                                            None => fail!('run, e),
                                        },
                                    }
                                    continue;
                                }
                                _ => match self.call_primitive(heap, p, base) {
                                    Ok(result) if tail => do_return!(result),
                                    Ok(result) => {
                                        self.stack.truncate(base);
                                        self.stack.push(result);
                                    }
                                    Err(e) => fail!('run, e),
                                },
                            },
                            Some(Obj::Continuation { stack, frames }) => {
                                let v = self.arguments_as_value(heap, base);
                                self.stack.clear();
                                self.stack.extend_from_slice(stack);
                                self.frames.clear();
                                self.frames.extend_from_slice(frames);
                                resume!(v);
                            }
                            Some(Obj::Escape { stack_len, frames_len, frame }) => {
                                let (stack_len, frames_len, frame) = (*stack_len, *frames_len, *frame);
                                let live = match frame {
                                    Some(fr) => self.frames.get(frames_len) == Some(&fr) && self.stack.len() >= stack_len,
                                    None => true,
                                };
                                if !live {
                                    fail!('run, "escape continuation invoked after its extent has ended");
                                }
                                let v = self.arguments_as_value(heap, base);
                                self.stack.truncate(stack_len);
                                self.frames.truncate(frames_len + frame.is_some() as usize);
                                resume!(v);
                            }
                            _ => fail!('run, format!("not a procedure: {}", print_value(f))),
                        }
                        break;
                    }
                }
                Op::Return => do_return!(self.stack.pop().unwrap()),
            }
        }
    }

    /// Capture the continuation of a call to `%call/cc` in slot `base`:
    /// copies of the stacks as they will be when the call returns, with the
    /// frame to return into (`here`, the running procedure, unless the call
    /// is a tail call) on top.
    fn capture(&mut self, heap: &mut Heap, tail: bool, base: usize, here: Frame) -> Value {
        self.captures += 1;
        let (stack_len, mut frames) = (if tail { here.fp - 1 } else { base }, self.frames.clone());
        if !tail {
            frames.push(here);
        }
        heap.continuation(self.stack[..stack_len].to_vec(), frames)
    }

    /// Capture an escape-only continuation of a call to `%call/ec`: the same
    /// as `capture`, but recording only where the stacks will be.
    fn capture_escape(&mut self, heap: &mut Heap, tail: bool, base: usize, here: Frame) -> Value {
        if !tail {
            return heap.escape(base, self.frames.len(), Some(here));
        }
        match self.frames.last() {
            Some(&caller) => heap.escape(here.fp - 1, self.frames.len() - 1, Some(caller)),
            None => heap.escape(0, 0, None),
        }
    }

    /// The arguments above slot `base`, as the one value a continuation
    /// returns: the argument itself, or a `values` object.
    fn arguments_as_value(&mut self, heap: &mut Heap, base: usize) -> Value {
        let args = self.stack.split_off(base + 1);
        heap.values(args)
    }

    /// Collect garbage if a collection is due. The roots are the value
    /// stack, the closures saved in frames, the running closure and the
    /// current ports (the heap adds the symbol table, the environments and
    /// the libraries).
    #[inline]
    fn safepoint(&self, heap: &mut Heap, closure: GcRef) {
        if heap.needs_gc() {
            let frames = self.frames.iter().map(|f| Value::obj(f.closure));
            let roots = self.stack.iter().chain(&self.ports).copied().chain(frames).chain([Value::obj(closure)]);
            heap.collect(roots);
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

    /// `(apply f arg ... list)`, with `apply` in slot `base`: replace it
    /// with `f arg ... element ...`, ready to be called the same way. So a
    /// tail call to `apply` is a tail call to `f`.
    fn spread_apply(&mut self, base: usize) -> Result<(), String> {
        let n = self.stack.len() - base - 1;
        if n < 2 {
            return Err(format!("apply: wrong number of arguments: {}", n));
        }
        let list = self.stack.pop().unwrap();
        let items = list_to_vec(list).ok_or_else(|| format!("apply: not a proper list: {}", print_value(list)))?;
        self.stack.remove(base);
        self.stack.extend(items);
        Ok(())
    }

    /// Call primitive `p`, whose arguments are above slot `base`.
    fn call_primitive(&mut self, heap: &mut Heap, p: &Primitive, base: usize) -> Result<Value, Error> {
        let n = self.stack.len() - base - 1;
        if n < p.min || p.max.is_some_and(|max| n > max) {
            return Err(format!("{}: wrong number of arguments: {}", p.name, n).into());
        }
        match p.f {
            PrimFn::Builtin(f) => f(heap, &self.stack[base + 1..]),
            PrimFn::Vm(f) => {
                let args = self.stack[base + 1..].to_vec();
                f(self, heap, &args)
            }
            PrimFn::Apply | PrimFn::CallCc | PrimFn::CallEc | PrimFn::Eval => unreachable!("handled by the call instructions"),
        }
    }
}

/// The procedure `name` in the system environment, if `scheme/core.scm` has
/// defined it: `%raise-error`, which errors are raised through, and
/// `%load-then-eval`.
fn system_procedure(heap: &mut Heap, name: &str) -> Option<Value> {
    let name = heap.intern(name).as_ref().unwrap();
    match env_of(heap.system?).borrow().lookup(name)? {
        TopBinding::Variable(cell) => {
            let v = cell_value(cell).get();
            matches!(v.as_obj().map(|o| &o.kind), Some(Obj::Closure { .. })).then_some(v)
        }
        _ => None,
    }
}
