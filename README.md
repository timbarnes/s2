# s2 Scheme

A Scheme compiler and bytecode virtual machine, written in Rust. It
implements [R7RS-small](https://standards.scheme.org/corrected-r7rs/r7rs.html),
except for complex numbers.

s2 is a learning project and the successor to the interpreter
[s1](https://github.com/timbarnes/s1). It was built in small phases, and
each one records what it built, what it decided, and what it measured.

Documentation:

- the [user guide](https://timbarnes.github.io/s2/), with the source in
  `docs/`;
- the [design notes](plan/README.md): the plan, the architecture, and a
  note for each phase;
- the [internal docs](https://timbarnes.github.io/s2/api/s2/), for working
  on s2 itself.

## Getting started

```bash
cargo build --release
target/release/s2                      # the REPL
target/release/s2 prog.scm arg ...     # run a program
target/release/s2 --compile prog.scm   # compile it to prog.s2o
target/release/s2 prog.s2o arg ...     # run the compiled program
target/release/s2 -e '(+ 1 2)'         # evaluate an expression
```

## What it has

- **R7RS-small.** It passes chibi-scheme's R7RS test suite except for
  complex numbers.
  - **Language:** hygienic `syntax-rules` macros, libraries with
    `define-library` and `.sld` files, re-entrant continuations,
    `dynamic-wind`, exceptions, parameters and records.
  - **Data:** exact integers of any size and exact rationals, full Unicode
    characters and strings, bytevectors, and binary and textual ports.
- **A bytecode compiler.** Each top-level form is macro-expanded, compiled
  to bytecode for a stack VM and run. `--expand` and `--dis` show each
  step.
- **Compiled files.** Programs can be compiled ahead (`--compile`), and
  library files are compiled and cached next to their source.
- **A precise mark-sweep garbage collector.**

## Speed

s2 is about 6 times faster than s1. Against established Schemes, measured
on ecraven's r7rs-benchmarks, it is:

- 3.6 times faster than chibi;
- two-thirds the speed of Gauche;
- a fifth of Guile's speed;
- a ninth to a fifteenth of the speed of the native-code compilers.

[plan/07-comparison.md](plan/07-comparison.md) has the details.

## Repository

| Path | |
|---|---|
| `src/` | the reader, compiler (`compile/`), VM (`vm/`), heap and builtins |
| `scheme/core.scm` | the derived syntax and the procedures written in Scheme |
| `tests/` | unit, golden, process and Scheme tests, and chibi's R7RS suite (`tests/r7rs/run.sh`) |
| `bench/` | s2 against s1 (`bench.sh`), and against other Schemes (`r7rs/`) |
| `docs/` | the user guide (MkDocs) |
| `plan/` | the design notes |

`cargo test` runs the tests.

The R7RS suite in `tests/r7rs/` is chibi-scheme's, under its own license
(`tests/r7rs/LICENSE-chibi`).
