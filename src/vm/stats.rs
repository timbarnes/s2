//! Instruction statistics, with `--features stats` (plan/05 §7): how often
//! each opcode runs, and each sequence of two and three consecutive
//! opcodes, printed to standard error at exit. The sequences show which
//! superinstructions would pay. Compiled out of normal builds.

use std::cell::RefCell;
use std::mem::Discriminant;

use rustc_hash::FxHashMap;

use crate::vm::Op;

/// More than the number of opcodes.
const N: usize = 64;

struct Counts {
    /// Each opcode seen so far, numbered in order of first appearance.
    ids: FxHashMap<Discriminant<Op>, usize>,
    names: Vec<String>,
    ops: Vec<u64>,
    pairs: Vec<u64>,
    triples: Vec<u64>,
    /// The two previous opcodes' numbers, N for none.
    last: [usize; 2],
}

thread_local! {
    static COUNTS: RefCell<Counts> = RefCell::new(Counts {
        ids: FxHashMap::default(),
        names: Vec::new(),
        ops: vec![0; N],
        pairs: vec![0; N * N],
        triples: vec![0; N * N * N],
        last: [N, N],
    });
}

/// Count one executed instruction.
pub fn record(op: Op) {
    COUNTS.with(|c| {
        let c = &mut *c.borrow_mut();
        let next = c.names.len();
        let id = *c.ids.entry(std::mem::discriminant(&op)).or_insert(next);
        if id == next {
            c.names.push(format!("{:?}", op).split('(').next().unwrap().to_string());
        }
        c.ops[id] += 1;
        let [a, b] = c.last;
        if b < N {
            c.pairs[b * N + id] += 1;
            if a < N {
                c.triples[(a * N + b) * N + id] += 1;
            }
        }
        c.last = [b, id];
    });
}

/// Print the counts, most frequent first: every opcode, then the pairs and
/// triples that are at least 0.1% of all instructions.
pub fn report() {
    COUNTS.with(|c| {
        let c = c.borrow();
        let total: u64 = c.ops.iter().sum();
        eprintln!("{} instructions", total);
        let print = |title: &str, counts: &[u64], width: usize| {
            let mut rows: Vec<(usize, u64)> = counts.iter().copied().enumerate().filter(|&(_, n)| n * 1000 >= total && n > 0).collect();
            rows.sort_by(|a, b| b.1.cmp(&a.1));
            eprintln!("{}:", title);
            for (i, n) in rows {
                let names: Vec<&str> = (0..width).rev().map(|k| c.names[i / N.pow(k as u32) % N].as_str()).collect();
                eprintln!("{:>12} {:5.1}%  {}", n, 100.0 * n as f64 / total as f64, names.join(" "));
            }
        };
        print("ops", &c.ops, 1);
        print("pairs", &c.pairs, 2);
        print("triples", &c.triples, 3);
    });
}
