//! Instruction statistics, with `--features stats` (plan/05 §7): how often
//! each opcode runs, and each pair of consecutive opcodes, printed to
//! standard error at exit. The pairs show which superinstructions would
//! pay. Compiled out of normal builds.

use std::cell::RefCell;

use rustc_hash::FxHashMap;

use crate::vm::Op;

#[derive(Default)]
struct Counts {
    ops: FxHashMap<&'static str, u64>,
    pairs: FxHashMap<(&'static str, &'static str), u64>,
    last: &'static str,
}

thread_local! {
    static COUNTS: RefCell<Counts> = RefCell::default();
}

/// The opcode's name, without its operand.
fn name(op: Op) -> &'static str {
    let text = format!("{:?}", op);
    let name = text.split('(').next().unwrap().to_string();
    Box::leak(name.into_boxed_str())
}

/// Count one executed instruction.
pub fn record(op: Op) {
    COUNTS.with(|c| {
        let mut c = c.borrow_mut();
        // ponytail: interns names through a leaked string per call site; a
        // stats build is for measuring, not for speed.
        let n = name(op);
        let n = *c.ops.keys().find(|k| **k == n).unwrap_or(&n);
        *c.ops.entry(n).or_default() += 1;
        let last = c.last;
        if !last.is_empty() {
            *c.pairs.entry((last, n)).or_default() += 1;
        }
        c.last = n;
    });
}

/// Print the counts, most frequent first: every opcode, and the top 25
/// pairs.
pub fn report() {
    COUNTS.with(|c| {
        let c = c.borrow();
        let total: u64 = c.ops.values().sum();
        let mut ops: Vec<_> = c.ops.iter().collect();
        ops.sort_by(|a, b| b.1.cmp(a.1));
        eprintln!("{} instructions", total);
        for (op, n) in ops {
            eprintln!("{:>12} {:5.1}%  {}", n, 100.0 * *n as f64 / total as f64, op);
        }
        let mut pairs: Vec<_> = c.pairs.iter().collect();
        pairs.sort_by(|a, b| b.1.cmp(a.1));
        eprintln!("pairs:");
        for ((a, b), n) in pairs.into_iter().take(25) {
            eprintln!("{:>12} {:5.1}%  {} {}", n, 100.0 * *n as f64 / total as f64, a, b);
        }
    });
}
