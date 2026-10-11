# 06. Directions after P10

Written 2026-10-11, after P10, from a discussion with the user. With P0-P10
done, s2 is a complete R7RS-small system, 6.2x faster than s1 on `fib` and
5.9x on the typical program. This document records the larger questions
that came up about what follows, the options considered and what was
decided.

## Where s2 stands

Measured at P10:

| Part | Rust lines |
|---|---|
| Compiler: `expand`, `syntax_rules`, `analyze`, `codegen`, `ast`, `disasm` | ~2,400 |
| The compile-time half of `libraries.rs` (import sets, `define-library`) | ~300 of 674 |
| Reader: tokenizer, parser, number syntax | ~1,500 |
| Runtime: VM, heap, values, globals, builtins, ports, printer, strings | ~6,000 |
| Total | 10,486, plus 510 lines of `scheme/core.scm` |

`s2 -e 1` peaks at about 3 MB on macOS (allocator arenas included), and
the release binary is 1.7 MB.

## Decisions

| # | Decision | Choice |
|---|---|---|
| U7 | Where the compiler in Scheme is built | In this repository, not a fork. The current state is tagged before it starts |
| U8 | Order of the next phases | P11 bytecode files, P12 comparison with other Schemes, P13 the compiler in Scheme |
| U9 | Embedded targets (ESP32) | Not planned. Least important of the options; recorded below |
| U10 | Native code | Not planned for now; recorded below |

## 1. The compiler in Scheme: this repository or a fork

The roadmap's last phase rewrites the compiler in Scheme and has s2 compile
itself (02 option E). The question was whether to do that here, or to keep
this repository as an efficient Scheme engine in its own right and fork a
copy to grow into a self-hosted compiler.

**Decided: here, with a tag.**

- **The Rust compiler is the test oracle for the Scheme one.** The plan
  ports one pass at a time: Scheme analysis feeding Rust codegen, say, with
  each pass's output checked against the Rust pass's. That only works with
  both compilers on the same runtime in the same tree. A fork loses it.
- **A fork doubles the runtime.** About 60% of the code is runtime, and
  every fix to it would need making twice. With s1 kept as the oracle, that
  would be three systems to keep in step.
- **A tag keeps the engine.** Before P13 starts, the repository is tagged
  (`rust-compiler`), so the Rust-compiled engine stays available as it was.
  During P13, a cargo feature keeps the Rust compiler buildable. Once the
  Scheme compiler passes the same tests (the 102 test files, chibi's 1,154),
  removing the Rust one is a separate decision.

A fork would be right only if both were to be maintained as products in
the long term: an embeddable Scheme with a Rust compiler, and a self-hosted
one. Nothing calls for that now.

## 2. A Scheme compiler on a reduced Rust runtime

With the compiler in Scheme, the Rust part becomes a runtime: the VM, the
GC, the builtins and a loader for bytecode files. The compiler ships as a
*boot image*: its own compiled bytecode, written by P11's serializer and
loaded at startup. `eval`, `load`, library loading and the REPL call it as
ordinary Scheme.

**It shrinks the Rust code less than it sounds.** About 2,700 of 10,500
lines go, roughly a quarter:

- the reader stays, since R7RS has `read`;
- the builtins and the printer are most of what remains, and moving them
  into Scheme makes them slower.

**The gains are structural:**

- **P8's workaround goes.** The Rust expander can't run Scheme, so a
  library file needed by an `import` is loaded *between* expansion
  attempts (`pending_load`, `%load-then-eval`). A Scheme expander just
  calls the loader.
- **Procedural macros become possible**, such as explicit renaming or
  `syntax-case`, because the expander runs where macros run.
- **A runtime without the compiler** becomes a build option: a cargo
  feature that leaves the compiler out, for a target that only runs
  bytecode compiled elsewhere. That mattered mainly for embedded targets
  (§4).

**The costs:**

- Compiling is slower, likely several times slower than the Rust compiler
  until the Scheme one is tuned. P12's benchmarks give the baseline to
  measure this against.
- Bootstrapping complicates the build. Changing the bytecode format means
  rebuilding the compiler image with the old compiler, and a broken image
  has to be recoverable. The Rust compiler (or the tagged build) is the way
  back.
- A bug can be in the compiler, or in the VM running the compiler.

None of this needs a fork: it is a build configuration of the same crate.

## 3. Comparing s2 with other Schemes (P12)

To see how s2 compares with established systems in completeness, features,
architecture and performance. It comes after P11 and before the compiler
in Scheme, so it measures the Rust-compiled s2 as a baseline, and the same
benchmarks later show what moving the compiler into Scheme costs. Details
are in the roadmap's P12.

**The systems**, chosen to cover the main designs:

| System | Design |
|---|---|
| chibi | a small C bytecode VM; R7RS's reference implementation |
| Guile | a bytecode VM with a JIT, its compiler written in Scheme (CPS); the nearest to where s2 is going |
| Chicken | Scheme to C, CPS, "Cheney on the MTA" |
| Gambit | Scheme to C, aimed at speed |
| Chez (and Racket CS, built on it) | native code, nanopass compiler |
| Gauche | a C VM |
| s7 | an embeddable interpreter |
| s1 | s2's oracle: a CEK interpreter in Rust |

**Completeness and features**, as tables: chibi's R7RS suite where it runs,
the numeric tower, macro systems (`syntax-rules` only, or also explicit
renaming or `syntax-case`), SRFIs, Unicode, threads, an FFI, debugging and
source locations, and compiled or image files.

**Architecture**: for each system, the value representation, the GC, how
continuations work, how the compiler is built and in what language, and
what it compiles to. Mostly from documentation and source, as background
for P13.

**Performance**: ecraven's `r7rs-benchmarks`, about 60 R7RS programs with
published results for around 30 Schemes. Getting s2 to run them is a
completeness test in itself; some will need features s2 lacks, and a few
need complex numbers. The systems are installed locally (most are in
Homebrew) and measured on the same machine, rather than compared with
published numbers from other hardware.

## 4. Embedded: the ESP32 (not planned)

Recorded for later. The least important of the directions discussed.

- **Chips.** The ESP32-C3 and C6 are RISC-V, supported by the standard
  Rust toolchain. The original ESP32, S2 and S3 are Xtensa, which needs
  Espressif's toolchain fork, installed with `espup`. An S3 with 2-8 MB of
  PSRAM is the comfortable choice for memory; a C6 for the standard
  toolchain.
- **Two ways to run Rust.** `esp-idf-svc` gives `std` on FreeRTOS (`Vec`,
  `Box`, threads, files through a virtual filesystem), so s2 would port
  mostly as it is. `esp-hal` is bare metal, `no_std` with `alloc`: leaner,
  but files, environment variables, `exit` and timing all have to go.
  Start with `esp-idf-svc`.
- **What changes in s2.** `mimalloc` goes (it doesn't support these
  targets). Ports, `command-line`, `exit` and environment variables sit
  behind a feature. The REPL runs over USB serial. The VM keeps its own
  stacks and the GC marks with a worklist, so neither recurses on the Rust
  stack, but the expander and codegen do: `main` runs them on a large-stack
  thread, which becomes a FreeRTOS task with a large stack.
- **Memory is the risk.** A C3 has about 400 KB of SRAM, perhaps 300 KB of
  it usable heap. s2's live heap after startup needs measuring (the 3 MB
  peak includes allocator arenas). The 16-byte `Value`, boxed heap objects,
  and compiling `core.scm` on every start all count against it. P11 lets
  `core.scm` ship compiled; the runtime without a compiler (§2) means the
  device only runs bytecode compiled on the host.
- **Effort.** The toolchain and a first program: an afternoon
  (`cargo generate esp-rs/esp-idf-template`, `espflash flash --monitor`).
  s2 at a serial REPL: a few days, mostly on memory. GPIO, timers and
  the like would be builtins over `esp-idf-hal`.

## 5. Native code (not planned for now)

Recorded for later. Three levels, each much larger than the last:

| Level | What it is | Typical gain over a good VM |
|---|---|---|
| Baseline | each bytecode instruction becomes a machine-code template (copy-and-patch), reusing the VM's logic | 1.5-3x |
| A real backend | compile from the analysed AST to Cranelift IR; Cranelift allocates registers and selects instructions | 3-10x on `fib`-like code |
| Optimising | type and representation analysis to remove checks, unboxed floats, inlining across procedures (Chez's territory) | more, at the size of everything built so far |

The hard parts are not instruction selection:

- **Value representation.** Native code wants a value in one machine
  word: P10's deferred item 6, the tagged-word `Value`, becomes a
  prerequisite.
- **GC roots.** Live values sit in registers and on the native stack, so a
  precise GC needs stack maps. Cranelift can produce them.
- **Tail calls.** Cranelift has a `tail` calling convention with
  `return_call`.
- **`call/cc`**, the hardest. s2 copies its own VM stacks. With native
  frames the options are CPS conversion, known frame layouts so the native
  stack can be copied, or frames on the heap.
- **The runtime.** The builtins can be reused as `extern "C"` functions.
  Ahead-of-time code is linked against the runtime as a static library;
  `cranelift-jit` does it in memory.

How it interacts with the other directions:

- **Cranelift has no backend for 32-bit RISC-V or Xtensa**, so native code
  on an ESP32 would go through C, emitted and compiled with Espressif's
  toolchain, as Chicken and Gambit do.
- **The compiler's language suggests the backend.** A compiler in Scheme
  emits text easily: C or assembly. A compiler in Rust drives Cranelift's
  builder API, which Scheme could only reach through a large set of
  bindings. After P13, a C backend is the natural route.

## Why P11 comes first

Every direction above needs a serialised compiled form:

- the compiler in Scheme loads as a boot image;
- a runtime without a compiler runs bytecode compiled elsewhere;
- an embedded target ships `core.scm` and programs compiled on the host;
- ahead-of-time native code needs the same picture of what a compiled unit
  depends on.

So P11's file format should be designed with the boot image in mind: a
whole set of libraries in one file, loadable before any Scheme has run.
