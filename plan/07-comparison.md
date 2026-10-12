# 07. How s2 compares with other Schemes

Written 2026-10-11, in P12 (06 §3). Everything measured here was measured
on one machine, an Apple M2 Max running macOS 27.0.1, with the scripts in
`bench/r7rs/`, so it can be run again.

In short:

- **Performance.** Of these ten systems, only chibi and s1 are slower than
  s2. Over the benchmarks both finish, chibi takes 3.6 times as long as s2,
  and s1 5.8 times. Gauche, the closest design to s2's, takes 0.68 of s2's
  time. The native-code compilers (Chez, Gambit, Racket) take a fifteenth
  to a ninth.
- **Completeness.** s2 passes everything in chibi's R7RS suite that it
  reaches, as do the others. The one gap is complex numbers, which every
  other system has.
- **Features.** s2 has none of the large systems' features beyond
  R7RS-small (FFI, threads, low-level macros, source locations), but it
  does have compiled files.
- **Architecture.** s2 is a textbook bytecode VM, in the family of chibi
  and Gauche.

## The systems

| System | Version | Installed from | Design in one line |
|---|---|---|---|
| **s2** | P11 (8452ec4 + P12) | this repository | bytecode compiler and stack VM, in Rust |
| **s1** | its last build | `../s1` | CEK-machine interpreter, in Rust; s2's oracle |
| chibi | 0.12.0 | Homebrew | small C bytecode VM; R7RS's reference implementation |
| Guile | 3.0.11 | Homebrew | compiler written in Scheme; bytecode VM with a JIT |
| Gauche | 0.9.15 | Homebrew | C bytecode VM, compiler written in Scheme |
| Chicken | 6.0.0 | Homebrew | compiles Scheme to C |
| Gambit | 4.9.8 | Homebrew | compiles Scheme to C |
| Chez | 10.4.1 | Homebrew | compiles Scheme to native code |
| Racket | 9.3 (CS) | Homebrew `minimal-racket`, and its `r7rs` package | Racket's expander over Chez |
| s7 | 11.9 (9-Oct-2026) | built from source, `clang -O2` | embeddable C interpreter |

## Architecture

How each system answers the questions in 03. Rows marked † were checked on
the installed systems here (what they link, what they print); the rest are
from each system's documentation and published papers.

| | Compiles to | Compiler written in | Garbage collector | Continuations | Macros |
|---|---|---|---|---|---|
| **s2** | bytecode for a stack VM (an `enum` per instruction) | Rust | precise, non-moving mark-sweep | copy the VM's stacks (D8) | `syntax-rules`, hygiene by renaming |
| **s1** | nothing: it interprets the code as a CEK machine | — | precise mark-sweep | heap-allocated continuation frames: capture is O(1) | `syntax-rules`, and a non-hygienic `macro` |
| chibi | bytecode for a stack VM | C, with libraries in Scheme | its own precise, non-moving collector † (Boehm is an option, off) | copies of the VM stack | syntactic closures; `syntax-rules` and explicit renaming built on them |
| Guile | bytecode for a register VM, JIT-compiled to machine code | Scheme: Tree-IL, then CPS, then bytecode | Boehm, conservative † (links `libgc`) | delimited continuations (prompts) are primitive; `call/cc` copies the stack | `syntax-case` (psyntax) |
| Gauche | bytecode for a stack VM | Scheme, precompiled to C | Boehm, conservative † (bundled) | frames moved to the heap when captured | `syntax-rules`, explicit renaming, traditional macros |
| Chicken | C, after CPS conversion | Scheme | generational: the C stack is the nursery ("Cheney on the M.T.A.") | free: after CPS, every continuation is a closure | `syntax-rules`, explicit and implicit renaming |
| Gambit | C, through its own virtual machine (GVM) | Scheme | copying | heap-allocated frames, with a stack cache | `syntax-rules`, `define-macro`, `syntax-case` |
| Chez | native machine code | Scheme (nanopass) | generational, copying | segmented stack: capture is O(1) | `syntax-case` |
| Racket | native code, through Chez | Racket (the expander), then Chez's compiler | Chez's | Chez's, under Racket's delimited control | sets of scopes; `syntax-case`, `syntax-parse` |
| s7 | nothing: an interpreter that rewrites calls into specialised operations as it runs them | — | mark-sweep | copies of the stack | `define-macro` (not hygienic), first-class environments |

Where s2 sits: in the family of chibi and Gauche (a compiler to bytecode
for a stack VM, no JIT), with the compiler in its implementation language
like chibi, not in Scheme like Gauche and Guile. P13 moves it to Guile's
and Gauche's side.

## Completeness

### Chibi's R7RS suite

`bench/r7rs/conformance.sh` runs the suite as `tests/r7rs/run.sh` does: one
section at a time, through the same stand-in for `(chibi test)`
(`tests/r7rs/shim.scm`). Here every system runs each section as a program,
so a top-level error ends the section: one unsupported feature can cost
every test after it. s2's own runner uses `-f`, which carries on, so its
1,154 is higher than its 794 here.

| section | s2 | s1 | chibi | Guile | Gauche | Chicken | Gambit | Racket |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 4.1 Primitive expression types | 27 | 27 | 27 | 27 | 27 | 27 | 27 | 27 |
| 4.2 Derived expression types | 74 | 74 | 74 | 68 | 74 | 74 | 74 | 74 |
| 4.3 Macros | 25 | 25 | 25 | stopped | 25 | stopped | stopped | stopped |
| 5 Program structure | 15 | 15 | 15 | 15 | 15 | 15 | stopped | 15 |
| 6.1 Equivalence predicates | 25 | 25 | 25 | 25 | 25 | 25 | 25 | 25 |
| 6.2 Numbers | stopped | stopped | stopped | 215 | 218 | 214 | 217 | 217 |
| 6.3 Booleans | 18 | 18 | 18 | 18 | 18 | 18 | 18 | 18 |
| 6.4 Lists | 65 | 65 | 65 | 63 | 65 | 65 | 65 | 65 |
| 6.5 Symbols | 17 | 17 | 17 | 17 | 17 | 17 | 17 | 17 |
| 6.6 Characters | 79 | 79 | 79 | 79 | 79 | 74 | 79 | 79 |
| 6.7 Strings | 130 | 130 | 130 | 129 | 130 | 126 | 130 | 130 |
| 6.8 Vectors | 43 | 43 | 43 | 41 | 43 | 43 | 43 | 43 |
| 6.9 Bytevectors | 39 | 39 | 39 | 39 | 39 | 37 | 39 | 39 |
| 6.10 Control features | 34 | 34 | 34 | 31 | stopped | 33 | 34 | 34 |
| 6.11 Exceptions | 30 | 30 | 30 | 29 | 30 | 30 | stopped | 29 |
| 6.12 Environments and evaluation | 4 | 4 | 4 | 4 | 4 | 4 | 0 | 4 |
| 6.13 Input and output | 63 | 63 | 63 | 62 | 63 | stopped | 58 | 63 |
| Read syntax | 93 | 93 | 93 | stopped | 93 | 83 | 87 | 90 |
| Numeric syntax | stopped | stopped | 220 | 201 | 201 | 209 | 206 | 214 |
| 6.14 System interface | 13 | 13 | 13 | 12 | 13 | 13 | 12 | 12 |
| **total** | **794** | **794** | **1,014** | **1,075** | **1,179** | **1,107** | **1,131** | **1,195** |

Why sections stopped:

- **s2 and s1**: a complex-number literal (`3+4i`, `1+2i`), in Numbers and
  Numeric syntax. The two lack only complex numbers (P9).
- **chibi**: its reader rejects a character literal in its own Numbers
  section ("invalid numeric syntax: #\\n").
- **Guile**: Macros, at a body that defines a name and then uses
  `let-syntax` ("invalid or duplicate identifier in definition"); Read
  syntax, at a string escape it doesn't accept.
- **Gauche**: `(test-error (apply +))`. Gauche checks `apply`'s argument
  count while compiling, so the error ends the section instead of being
  caught by the test.
- **Chicken**: Macros, at an ellipsis after a nested ellipsis pattern
  ("too many ellipses"); Input and output produced nothing within the
  60-second limit.
- **Gambit**: Macros, at an ill-formed special form; Program structure, at
  `except` in an import set; Exceptions produced nothing within the
  limit.
- **Racket**: Macros, at a `define-values` that binds a name twice.

Read this table with care:

- **It is chibi's suite.** s2 was tested against it from P6 to P9, so
  s2's numbers flatter it more than the others'.
- **It counts sections that ran.** It doesn't measure what the systems do
  beyond R7RS-small, which is most of what the larger ones are for.

Within those limits: every system passes nearly everything in the
sections it reaches. Where s2 falls short, it is complex numbers, which
every other system here has.

**Chez is not in the table.** It is an R6RS system: its reader rejects
R7RS syntax such as `#u8(...)`, and it has no R7RS libraries. The layer in
`bench/r7rs/chez/` provides just what the benchmarks need, which is far
from a complete R7RS. **s7** has no `import`.

### Probes

A short program, `bench/r7rs/probe.scm`, asked each system for bignums,
exact ratios, complex numbers, Unicode case mapping, the shortest float
that reads back, and R7RS's `(features)`:

| | bignums | ratios | complex | `(char-upcase #\λ)` | `(display 0.1)` |
|---|---|---|---|---|---|
| s2, s1 | yes | yes | **no** | `#\Λ` | `0.1` |
| chibi, Guile, Gauche, Chicken, Gambit, Racket, Chez | yes | yes | yes | `#\Λ` | `0.1` (Gambit `.1`; Chicken prints 15 digits, not the shortest) |

## Features beyond R7RS-small

From each system's documentation:

| | Standards beyond R7RS-small | Low-level macros | Threads | FFI | Debugging | Compiled files |
|---|---|---|---|---|---|---|
| **s2** | none | no | no | no | backtrace of procedure names; no source locations (P9) | yes, `.s2o` (P11) |
| **s1** | none | `macro` (not hygienic) | no | no | debugger, tracer, `help` | no |
| chibi | many SRFIs, R7RS-large libraries | syntactic closures, ER | yes (green) | C | REPL | images |
| Guile | R6RS, many SRFIs | `syntax-case` | yes (OS) | C (`libffi`) | debugger, profiler, source locations | yes, `.go` |
| Gauche | many SRFIs | ER, traditional | yes (OS) | C (generated stubs) | REPL, profiler | precompiled modules |
| Chicken | many SRFIs, eggs | ER, IR | yes (green) | C, inline | REPL | native executables |
| Gambit | many SRFIs | `define-macro`, `syntax-case` | yes (green, SMP optional) | C, inline | REPL debugger | native executables |
| Chez | R6RS | `syntax-case` | yes (OS) | C | inspector, debugger, profiler | `.so` boot and object files |
| Racket | its own large language and libraries | `syntax-case`, `syntax-parse` | yes (green and OS) | C | debugger, profiler, DrRacket | `.zo` |
| s7 | its own | `define-macro` | no | C (designed for embedding) | REPL | no |

s2 sits at the small end on every count but compiled files. That is by
design (README non-goals: the core stays pure), and these are the
features that would make it practical rather than educational:

- source locations in errors (deferred in P9);
- complex numbers (left out since P9);
- low-level macros, which P13's expander in Scheme makes possible (06
  §2);
- an FFI.

## Performance

> **These tables were rerun on 2026-10-12**, after s2's values were
> NaN-boxed (D2, commit ffbe13f). Against P12's run, s2 is 1.24 times
> faster (geometric mean over 56 benchmarks; 1.00-1.60x each, none
> slower), and every other system's times are within 3% of P12's. So
> Gauche's lead over s2 fell from 1.8x to 1.5x, and s2's over chibi grew
> from 2.9x to 3.6x.
>
> **Since then**, flonum fast paths in the arithmetic instructions and
> new inlined `*`, `/`, `vector-length`, `vector-ref` and `vector-set!`
> made s2 1.15 times faster again (geometric mean over 56, against Gauche
> as a control). The floating-point and vector benchmarks gained most:
> `mbrot` 4.3x, `sumfp` 4.1x, `fft` 3.1x, `fibfp` 2.9x, `pnpoly` 2.3x,
> `array1` and `puzzle` 1.4x. Gauche now takes 0.81 of s2's time. The
> tables below don't include this.

ecraven's [r7rs-benchmarks](https://github.com/ecraven/r7rs-benchmarks),
57 programs, run by `bench/r7rs/run.sh`:

```
bench/r7rs/run.sh -n 3 -t 120 "s2 chibi guile gauche s7 chicken gambit chez racket"
bench/r7rs/run.sh -n 1 -t 120 "s2 s1"
```

**How it was run:**

- **Scale.** Each benchmark's iteration count is divided by 10. The suite
  sizes its inputs for native-code Schemes, about a second each; at full
  size s2 would need an hour per round. Benchmarks that already do one
  iteration (`tak`, `takl`, `ntakl`, `cpstak`, `fib`, `ack`) run at full
  size.
- **Timing.** Each program times itself, so startup and compiling aren't
  counted: Chicken's and Gambit's C compiles, and Guile's and Racket's
  compiles to their own formats, all happen beforehand.
- **Medians.** The systems take turns, three rounds, and the table shows
  each one's median. s1 was run in a separate round of its own, against
  s2.
- **Limit.** A run that takes more than 120 seconds is stopped (TIMEOUT).
- **Compiler settings.** Each system runs as the suite's own scripts run
  it (`guild compile -O3`, `csc -O3`, `gsc -exe`, `raco make`, Chez at
  `--optimize-level 2`). Guile's and Chez's settings are those of the
  suite's published results.

**Relative to s2.** This is the geometric mean, over the benchmarks both
systems finished, of each system's time divided by s2's. Below 1 is
faster.

| system | time relative to s2 | benchmarks |
|---|---:|---:|
| Chez | 0.066 (15x faster) | 56 |
| Gambit | 0.077 (13x) | 56 |
| Racket | 0.115 (9x) | 56 |
| Guile | 0.184 (5x) | 55 |
| s7 | 0.286 (3.5x) | 51 |
| Chicken | 0.290 (3.4x) | 54 |
| Gauche | 0.678 (1.5x) | 56 |
| **s2** | **1** | 56 |
| chibi | 3.61 (3.6x slower) | 50 |
| s1 | 5.79 (5.8x slower) | 47 |

**Medians, in seconds** (at a tenth of the suite's iteration counts):

| benchmark | s2 | s1 | chibi | Guile | Gauche | s7 | Chicken | Gambit | Chez | Racket |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| browse | 2.113 | 22.319 | 75.456 | 0.285 | 1.299 | 0.891 | 0.662 | 0.094 | 0.081 | 0.184 |
| deriv | 2.545 | 19.083 | 4.960 | 0.381 | 1.824 | 0.896 | 0.354 | 0.105 | 0.074 | 0.101 |
| destruc | 2.768 | 22.018 | 4.999 | 0.201 | 1.497 | 0.701 | 0.652 | 0.153 | 0.111 | 0.185 |
| diviter | 2.021 | 19.857 | 2.995 | 0.361 | 0.976 | 0.292 | 0.189 | 0.158 | 0.094 | 0.136 |
| divrec | 2.129 | 22.418 | 2.865 | 0.582 | 1.366 | 0.467 | 0.478 | 0.179 | 0.117 | 0.167 |
| puzzle | 4.947 | 34.492 | 14.340 | 0.447 | 1.937 | 0.759 | 1.034 | 0.183 | 0.156 | 0.193 |
| triangl | 3.303 | 28.211 | 4.886 | 0.470 | 1.421 | 1.256 | 1.109 | 0.120 | 0.165 | 0.150 |
| tak | 26.617 | TIMEOUT | 34.653 | 2.208 | 12.830 | 19.879 | 10.634 | 1.286 | 1.214 | 1.284 |
| takl | 69.954 | TIMEOUT | TIMEOUT | 3.794 | 54.636 | 6.699 | 10.181 | 1.515 | 1.754 | 2.907 |
| ntakl | 59.004 | TIMEOUT | 58.841 | 3.876 | 50.454 | 6.906 | 10.181 | 1.497 | 1.758 | 2.902 |
| cpstak | 53.999 | TIMEOUT | 115.780 | 8.182 | 57.108 | 32.713 | 9.710 | 3.037 | 1.663 | 2.134 |
| ctak | 18.925 | 47.171 | TIMEOUT | 41.748 | 11.822 | 8.544 | 2.231 | 1.710 | 0.430 | 1.283 |
| fib | 10.338 | 73.815 | 9.677 | 1.033 | 5.780 | 0.907 | 5.239 | 0.409 | 0.507 | 0.533 |
| fibc | 1.701 | 6.085 | 9.120 | 2.222 | 1.097 | 0.597 | 0.201 | 0.106 | 0.032 | 0.104 |
| fibfp | 2.843 | 5.998 | 2.189 | 0.636 | 0.661 | 0.078 | 0.581 | 0.089 | 0.150 | 0.163 |
| sum | 4.816 | 44.873 | 6.015 | 0.390 | 2.230 | 0.410 | 1.704 | 0.171 | 0.221 | 0.254 |
| sumfp | 5.050 | 10.266 | 4.331 | 1.227 | 0.883 | 0.184 | 0.597 | 0.244 | 0.216 | 0.247 |
| fft | 1.799 | 16.200 | 3.412 | 0.256 | 0.605 | 0.810 | 0.256 | 0.150 | 0.135 | 0.149 |
| mbrot | 6.174 | 27.965 | 9.726 | 1.214 | 1.304 | 0.909 | 0.589 | 0.295 | 0.417 | 0.404 |
| mbrotZ | ERROR | ERROR | 21.650 | 1.185 | 1.975 | 0.604 | 1.434 | 0.509 | 0.312 | 0.326 |
| nucleic | 2.227 | 9.887 | 4.127 | 0.318 | 1.051 | 0.684 | 0.285 | 0.090 | 0.120 | 0.137 |
| pi | 0.039 | 0.041 | 52.592 | 0.006 | 0.230 | ERROR | 0.036 | 0.043 | 0.031 | 0.042 |
| pnpoly | 6.256 | 23.704 | 13.629 | 0.936 | 1.462 | 1.085 | 1.078 | 0.188 | 0.294 | 0.332 |
| ray | 2.852 | 13.709 | 10.329 | 0.376 | 1.141 | 0.861 | 0.343 | 0.090 | 0.152 | 0.278 |
| simplex | 4.142 | 49.082 | 10.052 | 0.392 | 2.153 | 1.911 | 0.602 | 0.097 | 0.136 | 0.164 |
| ack | 21.108 | TIMEOUT | 21.876 | 3.426 | 25.579 | 3.641 | 11.618 | 0.868 | 0.746 | 1.065 |
| array1 | 4.203 | 26.487 | 3.944 | 0.341 | 1.504 | 0.207 | 0.845 | 0.120 | 0.285 | 0.307 |
| string | 0.054 | 0.062 | 0.475 | 0.014 | 0.086 | 0.018 | 0.027 | 0.043 | 0.079 | 0.115 |
| sum1 | 0.093 | 0.159 | 3.862 | 0.273 | 0.066 | 0.016 | 0.708 | 0.029 | 0.043 | 0.725 |
| cat | 1.848 | 55.966 | 3.482 | 0.976 | 0.873 | 0.067 | 1.754 | 0.168 | 0.196 | 0.422 |
| tail | 0.226 | 0.879 | 0.535 | 0.408 | 0.339 | 0.034 | 0.220 | 0.192 | 0.200 | 0.210 |
| wc | 2.415 | 16.795 | 23.162 | 0.587 | 1.222 | 0.391 | 0.833 | 0.104 | 0.093 | 0.253 |
| read1 | 0.079 | 0.084 | 11.997 | 0.424 | 0.160 | 0.018 | 1.859 | 0.077 | 0.052 | 1.257 |
| compiler | 1.783 | 14.772 | 6.029 | 0.259 | 1.485 | 1.212 | 0.401 | 0.221 | 0.107 | 0.161 |
| conform | 3.720 | 27.410 | 7.984 | 0.438 | 2.725 | 2.536 | 0.793 | 0.192 | 0.124 | 0.317 |
| dynamic | 1.788 | 13.083 | 13.873 | 0.405 | 1.146 | ERROR | TIMEOUT | 0.279 | 0.127 | 1.003 |
| earley | 22.380 | TIMEOUT | TIMEOUT | 4.628 | 7.159 | 26.902 | 6.661 | 2.644 | 1.881 | 5.818 |
| graphs | 23.208 | TIMEOUT | TIMEOUT | 2.212 | 18.562 | 17.925 | 2.652 | 2.352 | 0.437 | 0.591 |
| lattice | 8.039 | 85.119 | 63.284 | 0.685 | 3.766 | 5.341 | 1.161 | 0.303 | 0.209 | 0.263 |
| matrix | 2.697 | 28.113 | 11.612 | 0.291 | 2.007 | 1.658 | 0.488 | 0.110 | 0.072 | 0.102 |
| maze | 1.640 | 15.189 | 4.692 | 0.175 | 1.213 | 0.942 | 0.432 | 0.038 | 0.057 | 0.067 |
| mazefun | 2.678 | 17.494 | 5.929 | 0.343 | 2.195 | 0.783 | 0.861 | 0.144 | 0.125 | 0.189 |
| nqueens | 6.010 | 98.611 | 9.863 | 0.701 | 4.570 | 2.214 | 1.990 | 0.412 | 0.357 | 0.403 |
| paraffins | 0.809 | 8.326 | TIMEOUT | 0.440 | 1.346 | 0.289 | 1.085 | 0.366 | 0.330 | 0.611 |
| parsing | 2.867 | 60.153 | 100.732 | 0.469 | 7.134 | 1.565 | 0.738 | 0.132 | 0.170 | 0.146 |
| peval | 3.253 | 37.845 | 9.440 | 0.409 | 2.442 | INCORRECT | 0.622 | 0.131 | 0.116 | 0.159 |
| primes | 0.968 | 10.193 | 1.093 | 0.198 | 0.771 | 0.273 | 0.297 | 0.057 | 0.052 | 0.074 |
| quicksort | 3.841 | 35.980 | 15.282 | 0.542 | 2.386 | 3.875 | 1.011 | 0.257 | 0.252 | 0.675 |
| scheme | 2.666 | 15.045 | 6.077 | 0.549 | 1.795 | INCORRECT | 0.966 | 0.262 | 0.143 | 0.238 |
| slatex | 1.331 | 5.611 | 7.273 | 1.254 | 1.310 | 0.904 | 0.990 | 0.803 | 0.758 | 1.230 |
| chudnovsky | 0.106 | 0.183 | 11.385 | 0.008 | 0.071 | ERROR | 0.018 | 0.018 | 0.013 | 0.018 |
| nboyer | 16.050 | TIMEOUT | 20.370 | 1.897 | 8.067 | 16.265 | 6.043 | 1.768 | 1.083 | 2.828 |
| sboyer | 16.160 | TIMEOUT | 21.032 | 1.762 | 9.551 | 14.238 | 3.856 | 0.675 | 0.519 | 0.811 |
| gcbench | 34.254 | 83.100 | TIMEOUT | 0.994 | 12.509 | 7.464 | 2.964 | 4.317 | 0.454 | 0.943 |
| mperm | 6.143 | 49.221 | 15.587 | 0.995 | 2.614 | 1.934 | 1.843 | 0.596 | 0.592 | 1.779 |
| equal | 0.506 | 0.758 | 22.130 | TIMEOUT | 7.450 | 0.448 | TIMEOUT | 0.171 | 0.381 | 1.568 |
| bv2string | 0.772 | 7.171 | 0.587 | 0.106 | 0.575 | 0.339 | 0.369 | 0.082 | 0.052 | 0.079 |

Failures:

- **s2 and s1**: `mbrotZ` needs complex numbers.
- **s1** timed out on the nine slowest benchmarks.
- **chibi** timed out on six.
- **s7**:
  - failed `pi` and `chudnovsky` (its bignums);
  - failed `dynamic`;
  - gave wrong answers for `peval` and `scheme`.
- **Guile** timed out on `equal`.
- **Chicken** timed out on `dynamic` and `equal`.

These match the suite's published results for those systems. On the
benchmarks both sets of results cover, the times here are close to the
published ones: Chicken's `tak` takes 11.0s here and 10.8s there.

**Reproducibility** (the phase's exit test):

- Within a run, s2's three times for a benchmark differ by at most 7%
  (`paraffins`); 46 of 56 are within 3%.
- Between the two runs, s2's medians agree within 0.95-1.04x, 0.999 on
  average. That is the same noise as in P10 and P12.

### What the numbers say about s2

**Where s2 stands.** s2 is between chibi and Gauche, the two systems built
the same way:

- **chibi** is a C bytecode VM that keeps its interpreter simple, and s2
  beats it on most benchmarks. The difference is largest where chibi's
  strings and symbols are slow (`browse`, `parsing`, `read1`, `wc`).
- **Gauche** has had twenty years of tuning: specialised instructions,
  inlined primitives, and a compiler that optimises before generating code.
  It is 1.5 times as fast as s2 overall.
- **Guile** compiles through CPS with real optimisation, and JIT-compiles
  its bytecode to machine code. It is 5 times faster.
- **The native-code compilers** are 9-15 times faster. That is the
  usual factor between a good bytecode VM and native code.

**Where s2 does well.** It beats Gauche on:

- `equal` (15x);
- `pi` (6x), on Rust's bignums;
- `read1`, `parsing`, `string` and `tail`, on its reader and string
  operations;
- `paraffins`, `ack` and `cpstak`.

**Where it does worst.** Against Gauche, the biggest gaps are:

- **floating-point code**: `sumfp` (5.7x slower), `mbrot` (4.7x),
  `pnpoly` and `fibfp` (4.3x). NaN-boxing sped these up only 1.12-1.24x. s2's flonums are unboxed (D2), but its arithmetic
  instructions have an inline path only for fixnums
  (`src/vm/machine.rs`). Every flonum operation leaves it for the general
  numeric code;
- `earley`, `fft`, `array1` and `puzzle` (2.6-3.1x): vector access, and
  loops of generic arithmetic;
- `gcbench` (2.7x): allocation and collection, which NaN-boxing barely
  changed.

These suggest where s2's time goes:

- flonums on the slow path of every arithmetic instruction;
- the cost of a generic instruction even on fixnums;
- a non-generational collector.

The first is a small change. The others are larger, and none of them is in
the plan yet. P13 is
measured against this table: s2 running a compiler written in Scheme
should produce code as fast as this, and be no slower to compile than 06
estimates.
