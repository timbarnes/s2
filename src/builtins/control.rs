//! Control (R7RS §6.10) and exceptions (§6.11): the primitives that
//! `scheme/core.scm` builds `call/cc`, `dynamic-wind`, `values`,
//! `with-exception-handler`, `raise`, `guard` and the rest on (plan/03 §10).

use super::{Error, ErrorKind, Primitive, PrimFn, err, expected, kind, prim, vm_prim};
use crate::compile;
use crate::heap::{Heap, Obj};
use crate::value::Value;
use crate::vm::Vm;
use crate::vm::machine::uncaught_message;

pub static PRIMITIVES: &[Primitive] = &[
    Primitive { name: "apply", min: 2, max: None, f: PrimFn::Apply },
    Primitive { name: "%call/cc", min: 1, max: Some(1), f: PrimFn::CallCc },
    Primitive { name: "%call/ec", min: 1, max: Some(1), f: PrimFn::CallEc },
    Primitive { name: "eval", min: 2, max: Some(2), f: PrimFn::Eval },
    prim("procedure?", 1, Some(1), |_, a| {
        Ok(Value::bool(matches!(kind(a[0]), Some(Obj::Closure { .. } | Obj::Primitive(_) | Obj::Continuation { .. } | Obj::Escape { .. }))))
    }),
    prim("values", 0, None, |h, a| Ok(h.values(a.to_vec()))),
    // What call-with-values applies its consumer to.
    prim("%values->list", 1, Some(1), |h, a| match kind(a[0]) {
        Some(Obj::Values(items)) => Ok(h.list(items.to_vec())),
        _ => Ok(h.list(vec![a[0]])),
    }),
    prim("%make-error-object", 2, Some(2), |h, a| {
        super::list_arg(a[1], "error")?;
        Ok(h.error_object(ErrorKind::General, a[0], a[1]))
    }),
    prim("error-object?", 1, Some(1), |_, a| Ok(Value::bool(matches!(kind(a[0]), Some(Obj::Error { .. }))))),
    prim("error-object-message", 1, Some(1), |_, a| Ok(error_object(a[0], "error-object-message")?.1)),
    prim("error-object-irritants", 1, Some(1), |_, a| Ok(error_object(a[0], "error-object-irritants")?.2)),
    prim("file-error?", 1, Some(1), |_, a| Ok(Value::bool(error_object(a[0], "").is_ok_and(|e| e.0 == ErrorKind::File)))),
    prim("read-error?", 1, Some(1), |_, a| Ok(Value::bool(error_object(a[0], "").is_ok_and(|e| e.0 == ErrorKind::Read)))),
    vm_prim("%abort", 1, Some(1), abort),
    vm_prim("dis", 1, Some(1), dis),
    // Like s1's %kont-depth: for tests that tail calls don't grow the stack.
    vm_prim("%frame-depth", 0, Some(0), |vm, _, _| Ok(Value::fixnum(vm.frame_depth() as i64).unwrap())),
];

/// An error object's kind, message and irritants.
fn error_object(v: Value, who: &str) -> Result<(ErrorKind, Value, Value), Error> {
    match kind(v) {
        Some(Obj::Error { kind, message, irritants }) => Ok((*kind, *message, *irritants)),
        _ => Err(expected(who, "an error object", v)),
    }
}

/// `(%abort obj)`: `raise` found no handler. End the top-level form with
/// `obj` as its error, recording the backtrace first.
fn abort(vm: &mut Vm, _: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    vm.record_backtrace();
    Err(Error { kind: ErrorKind::Uncaught, message: uncaught_message(args[0]), irritants: Vec::new() })
}

/// `(dis datum)`: compile `datum` as a top-level form for the interaction
/// environment and print its disassembly, without running it.
fn dis(_: &mut Vm, heap: &mut Heap, args: &[Value]) -> Result<Value, Error> {
    let env = heap.interaction.unwrap();
    let template = compile::compile(heap, env, args[0]).map_err(|e| err!("{}", e))?;
    print!("{}", compile::disasm::disassemble(template));
    Ok(Value::UNSPECIFIED)
}
