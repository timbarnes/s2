# 08. The compiler in Scheme (P13)

Written 2026-10-12, before P13 starts. The roadmap gives P13 one
paragraph: rewrite the compiler in Scheme, run it on s2, and have s2
compile itself (04, 06 §1-2). This document turns that into a plan: what
has to be decided first, the order of the work, sub-phases that each end
with every test passing, and the risks.

**Status:** the decisions below were accepted as proposed on 2026-10-12,
and P13.0 started.

## Scope

**The Scheme compiler replaces the Rust compiler inside the running
system.** It is what s2 uses whenever it compiles anything: the REPL,
`-e`, a program, `load`, `eval`, importing a library file, and
`--compile`. It is loaded at startup as a compiled image, as `core.scm`
is now (P11), and runs on the VM like any other Scheme code. Typing a
form at the REPL means Rust reads it, the Scheme compiler compiles it,
and the VM runs the result. Guile, Chez and Gauche work this way.

| Moves to Scheme (about 2,700 lines) | Stays in Rust (about 8,000 lines) |
|---|---|
| The expander: core forms, scopes, hygiene, bodies, quasiquote | The reader (`read` is needed at run time too) |
| `syntax-rules`: parsing, matching, instantiating | The VM, the GC, the heap |
| Analysis: free variables, boxes, lambda groups | The builtins, printer, ports, strings |
| Codegen: core AST to instructions | Reading and writing compiled files |
| Libraries at compile time: import sets, `define-library`, `include`, `cond-expand` | The objects the compiler works on: environments, cells, aliases, the library registry, finding library files (C3), through about fifteen primitives |
| | `%make-template`, turning instructions into a `Template` (C1) |
| | Startup: loading the image, the command line |

**It is not only a tool for writing compiled files.** A Scheme program
that wrote compiled files while the Rust compiler kept serving the REPL,
`eval` and `load` would be smaller. But it would mean two compilers
maintained for good, and none of the structural gains of 06 §2:

- the expander loading a library when an import needs it, so
  `pending_load` goes;
- procedural macros;
- a build with no compiler in Rust.

`--compile` keeps working in this design: the Scheme compiler makes the
templates, and the Rust writer saves them.

**At the end of P13**, if P13.6 decides to remove the Rust compiler, the
s2 binary is a Rust runtime with no compiler in it. Its compiler is
Scheme bytecode, compiled by itself, loaded from an image at startup.

## Where the compiler is now

| Part | Rust lines | What it does |
|---|---|---|
| `compile/expand.rs` | 851 | datum to core AST: core forms, scopes, hygiene by aliases, bodies, quasiquote, `import`, `define-library` |
| `compile/syntax_rules.rs` | 671 | parsing, matching and instantiating `syntax-rules`; identifiers and aliases |
| `compile/codegen.rs` | 495 | core AST to a `Template`: slots, closures, tail calls, inlining, superinstructions |
| `compile/analyze.rs` | 194 | free variables, which variables need boxes, lambda groups |
| `compile/ast.rs` | 165 | the core AST, and `--expand`'s printer |
| `libraries.rs`, its compile-time half | about 300 | import sets, `define-library` declarations, `include`, `cond-expand` |
| **Total** | **about 2,700** | |

The passes are already separate, with plain interfaces:

```
datum ──expand──► core AST ──analyze──► facts ──codegen──► Template
         (uses syntax_rules,                     (Ops, constants, names)
          Env, cells, aliases)
```

**What they depend on in the runtime:**

- **Environments** (`globals::Env`, a heap object): identifier to binding,
  where a binding is a variable's cell, a macro or a core form. Also
  whether the environment is mutable, which names are imported, and the
  bindings changed while compiling (`touched`, which compiled files
  record).
- **Cells**: a global variable's value. Templates hold cells as
  constants.
- **Aliases** (`Obj::Alias`): the renamed identifiers of hygiene.
- **Macros** (`Obj::Macro`), holding a parsed `SyntaxRules`. Compiled
  files store them in that parsed form.
- **The library registry** (`heap.libraries`), and library files found
  on the search path.

**How compiling is invoked today.** All from Rust:

- the REPL, `-e`, `-f` and programs (`Session::eval`);
- loading `core.scm` (`load_core`);
- `--compile`;
- library files (`%compiled-library`, a primitive);
- `eval` (handled by the VM's call instruction).

## Five constraints found in the code

**1. The VM isn't re-entrant.** A primitive runs inside the VM's call
instruction. It can't run Scheme code: `Vm::run` starts by clearing the
stacks. So:

- Rust code called from Scheme can never call the Scheme compiler.
  `%compiled-library` and `eval`'s path through the VM have to move into
  Scheme.
- The Rust expander can't call a `syntax-rules` matcher written in
  Scheme. The matcher compares literals by asking the expander what an
  identifier means (`free-identifier=?`), so the two are tied together.
  **The expander, `syntax-rules` and the library declarations must switch
  to Scheme as one unit.** Only the back end (analysis, codegen) can be
  swapped a pass at a time.

Making the VM re-entrant is possible (nested runs, with `call/cc`
forbidden across them), but it is a VM project of its own, and nothing
else needs it.

**2. s2 has no hash tables.** The expander looks up a top-level
identifier on every reference. In Scheme without hash tables that is an
association list, linear in the size of the environment (about 400 names
in the system environment). That argues for keeping environments as Rust
objects behind primitives.

**3. Macros are stored as Rust data.** A compiled file writes a macro as
its parsed `SyntaxRules`. If matching moves into Scheme, a macro becomes
Scheme data, and the file format must store that instead.

**4. Compiled files aren't reproducible.** P11 noted that the bindings
section follows hash-table order. Checking that the compiler reproduces
itself (a fixed point) needs the same input to give the same bytes.

**5. Compilation can collect garbage once it is Scheme.** The Rust
expander never reaches a safepoint, so the Rust code around it doesn't
root what it holds. A Scheme compiler allocates on the VM's heap, so
every Rust caller that holds a datum or template across a compile must
pin it (as `Session::expand` already does for a form while a library
loads).

## Decisions

Proposed, then accepted as they stand.

| # | Decision | Choice |
|---|---|---|
| C1 | What the Scheme compiler produces | Instructions as data. One primitive builds the `Template` |
| C2 | The core AST in Scheme | Tagged vectors, with accessors |
| C3 | Environments, cells, aliases, the library registry | Stay Rust objects, used through primitives |
| C4 | What a macro is | `syntax-rules` as Scheme data, matched by Scheme code |
| C5 | Bootstrapping | The Rust compiler, behind a cargo feature, builds the image at first run during P13. Whether to commit an image is decided in P13.7 |
| C6 | Who calls the compiler | Only Scheme code, or Rust at the top level. Never Rust inside a primitive |
| C7 | The oracle | Identical `--dis` and `--expand` output, in separate processes, over a corpus |
| C8 | The order of the work | Back end first, a pass at a time; then the front end as one switch; then the driver and self-compilation |
| C9 | Compile errors | Raised as Scheme errors with the same messages |
| C10 | Where the compiler's code lives | `scheme/compiler/*.scm`, loaded into the system environment, its names `%`-prefixed |
| C11 | Hash tables | Not added for P13, unless compile speed needs them |

### C1. What the Scheme compiler produces

| Option | For | Against |
|---|---|---|
| **a. Instructions as data**, e.g. `(local-ref 0)`, `(jump-if-not lt 7)`, built into a `Template` by one primitive, `%make-template` | The `Op` encoding stays in one place, in Rust. The output is readable and printable while debugging | One conversion per template, cheap next to compiling |
| b. Fasl bytes, written by Scheme | No new primitive | Duplicates the encoding in Scheme, and the template is still made by the loader |
| c. Build `Op`s through a primitive per instruction | No intermediate list | Many primitive calls; the encoding leaks into Scheme anyway |

**Proposal: a.** `%make-template` takes the template's fields: name,
required count, rest flag, number of free variables, instructions,
constants, and the local and free-variable names. It checks each
instruction's operands, which P11 noted the fasl loader doesn't do.

### C2. The core AST in Scheme

| Option | For | Against |
|---|---|---|
| Records (`define-record-type`) | Self-describing, checked accessors | The record types live in Scheme, so Rust can't easily build or read them, which P13.1's mixed pipeline needs |
| **Tagged vectors**, `#(lambda name params rest body)` | Rust can build and read them. Cheap, printable, and stored in compiled files as plain data | Accessors by convention: a wrong index is a silent bug |
| Tagged lists, `(lambda name params rest body)` | Easiest to print and pattern-match by hand | Slower field access; easy to confuse with source code |

**Proposal: tagged vectors**, behind accessor procedures defined in one
file (`scheme/compiler/ast.scm`), so no other code indexes them. A
variable is `#(var id name)`. A global is the cell itself, as in the
Rust AST.

### C3. Environments and the rest of the runtime's compile-time state

**Proposal: keep them in Rust**, used through about fifteen primitives:

- looking up an identifier;
- making or finding a cell;
- define, `define-syntax` and import;
- making and resolving aliases;
- the library registry and the search path;
- `touched`.

These objects are already shared by the VM (cells), compiled files
(environments, aliases, cells) and the libraries. Moving them into
Scheme would mean rewriting those three as well, and would need hash
tables (constraint 2). The expander's local scopes, which are small and
short-lived, become Scheme lists.

### C4. What a macro is

**Proposal:** a macro's rules become Scheme data: its literals, its
(pattern, template) pairs, the ellipsis, the definition depth and the
environment. Scheme code matches and instantiates them.

- `Obj::Macro` holds that datum instead of a Rust `SyntaxRules`.
- Compiled files store it as ordinary data, which removes their special
  case for macros.
- This is also the step that makes procedural macros possible later. A
  macro could hold a procedure instead, though compiled files would then
  need to store procedures (templates and their free variables). That is
  left for after P13.

**Changed in P13.3: macros stay Rust objects.** The Scheme expander
reads a macro's parts with `%macro-parts`, matches and instantiates them
in Scheme, and makes macros with `%make-macro`. The `SyntaxRules` struct
holds the same data as before. While both front ends are selectable they
must share macros, in environments and in compiled files, and this lets
them do that without changing the fasl format. Revisit with procedural
macros, or when the Rust front end goes (P13.6).

### C5. Bootstrapping

The compiler in Scheme must be compiled by something before it can run.

| Option | For | Against |
|---|---|---|
| **a. The Rust compiler compiles it**, at the first run after a build, and the image is cached next to the executable, as `core.scm` is now (P11) | No binary in git. The image can't be stale. The Rust compiler stays the way back from a broken image | Needs the Rust compiler in the binary, at least as a build option |
| b. A committed boot image, rebuilt by a script | How Guile and Chez do it. The Rust compiler can then go | A binary in git that must be regenerated whenever the instruction set or the compiler changes. Recovery from a bad image needs the tagged build |

**Proposal: a, for all of P13.** Whether to switch to b, and remove the
Rust compiler, is P13.7's decision, made with the measurements. The
repository is tagged `rust-compiler` before P13.0, as 06 decided. A cargo
feature, `rust-compiler`, on by default, keeps the Rust compiler
buildable throughout.

### C6. Who calls the compiler

**Proposal:** only Scheme code, or Rust at the top level (a `Vm::run`
with nothing else running). This follows from constraint 1:

- `eval` becomes Scheme: compile, make a closure, call it. A tail call
  to `eval` is still a tail call.
- `load` and `%load-library` are already Scheme. They call the compiler
  directly instead of the `%compiled-library` primitive.
- The REPL, `-e`, `-f`, programs and `--compile` call one Scheme
  procedure per form, `%compile-form`, through `Vm::run`, then run the
  result, as now.
- **`pending_load` goes.** An import of an unloaded library loads it
  then and there, because the expander is now Scheme that can call the
  loader.

### C7. The oracle

The strongest check available is that **the Scheme compiler produces
exactly the code the Rust compiler does**. That means the same
instructions, constants in the same order, the same slot and variable
names, and the same variable numbering in `--expand`.

- **How:** run each file of a corpus twice, once with each compiler
  (chosen by an environment variable, `S2_COMPILER=rust|scheme`), with
  `--expand --dis`, and diff the output. Separate processes share no
  state, so the two compilers never see each other's side effects
  (definitions, macros, imports).
- **The corpus:**
  - the Scheme test files;
  - chibi's R7RS suite;
  - `core.scm`;
  - the benchmarks in `bench/` and `bench/r7rs`;
  - the compiler's own source.
- **The cost:** the Scheme passes must reproduce details that don't
  affect behaviour, such as the order variables are numbered and
  constants are added. That is extra care, but it turns every difference
  into a bug report with the exact form attached.
- **Once the code matches**, the existing tests check behaviour, and the
  07 benchmarks need no rerun: identical bytecode runs identically.

### C8. The order of the work

| Option | For | Against |
|---|---|---|
| **Back to front**: codegen, analysis, then the front end | The first parts are pure functions over data, with the strongest oracle (C7). The AST interface gets settled on easy code first | The structural gains (`pending_load` removed, procedural macros possible) only arrive with the front end, at the end |
| Front end first | The gains come early | The hardest part first, with the most runtime coupling, and the Scheme front end would have to feed the Rust back end through a converter for longer |

**Proposal: back to front.** The front end is one sub-phase because of
constraint 1, but it is built and tested in steps before it is switched
on (P13.3).

### C9. Compile errors

Compile errors are Scheme errors, raised with exactly the messages the
Rust compiler gives, through a primitive that makes an error from a
message alone. Tests that check messages pass unchanged.

One visible change: `guard` and `with-exception-handler` can now catch a
syntax error from `eval`. R7RS allows that, and it is arguably better.

### C10. Where the compiler's code lives

`scheme/compiler/`:

- `ast.scm`
- `codegen.scm`
- `analyze.scm`
- `syntax-rules.scm`
- `expand.scm`
- `libraries.scm`
- `driver.scm`

**Changed in P13.1: the files are the body of a library, `(s2
compiler)`,** which exports only what Rust calls. The proposal was
`%`-prefixed names in the system environment, because a library would
need the library machinery running before the compiler that implements
it. That isn't so: the compiler loads from a compiled image, and loading
a compiled library runs no compiler. As a library, its internal names
stay private, the files read as ordinary Scheme, and `(s2)` doesn't grow.
Rust wraps the files in the `define-library` (`compiler_source` in
`main.rs`), since the files are built into the binary, where `include`
couldn't find them.

### C11. Hash tables

Not added. With environments in Rust (C3), what's left needs only small
maps:

- local scopes, as lists;
- the slot of each variable, as a vector indexed by variable number
  (numbers are dense within a top-level form);
- constant pools, de-duplicated with `eqv?`, which is quadratic but over
  dozens of entries.

If P13.7's compile-speed measurements point at one of these, hash tables
(SRFI 69 or R6RS style) are a good feature for users too.

## Sub-phases

Each ends with every test passing: release, debug, GC stress, and
chibi's suite at 1154.

| Phase | Builds | Exit test |
|---|---|---|
| P13.0 | Groundwork | Both compilers selectable; the diff harness runs |
| P13.1 | Codegen and analysis in Scheme | Identical `--dis` over the corpus |
| P13.2 | (folded into P13.1) | |
| P13.3 | The front end in Scheme: `syntax-rules`, the expander, library declarations | Identical `--expand` and `--dis` |
| P13.4 | The driver: every compile through Scheme; `eval` and loading in Scheme | All tests with `S2_COMPILER=scheme` |
| P13.5 | Self-compilation and the image | The fixed point |
| P13.6 | Measurement and the Rust compiler's fate | 07's tables, compile times |

### P13.0. Groundwork

- **Tag** the repository `rust-compiler`. (The cargo feature
  `rust-compiler` moved to P13.5: until then the Rust compiler is what
  builds the Scheme one, so there is nothing for the feature to leave
  out.)
- **`%make-template`** (C1), and the instruction table it reads: a name
  for each `Op`, with its operands.
- **The AST as data** (C2): a Rust converter from `Expr` to tagged
  vectors and back, so P13.1 can feed the Rust front end's output to the
  Scheme back end.
- **Reproducible compiled files** (constraint 4): write the bindings in
  a fixed order.
- **The harness**: `S2_COMPILER` selects the compiler, and a script runs
  the corpus both ways and diffs the output.
- **Exit:** the round trip Rust AST to vector and back gives identical
  `--dis` over the corpus; compiling a file twice gives identical bytes.

### P13.1. Codegen and analysis in Scheme

**Changed in P13.1: this includes analysis**, which was P13.2. The
Scheme codegen needs free variables, boxes and lambda groups. Reaching
the Rust analysis from Scheme would have needed primitives written only
to be thrown away a sub-phase later, and analysis is under 200 lines.

- `ast.scm` and `codegen.scm`: slots and depth, closures, `letrec`
  lambda groups, tail calls, inlining (whether a global is inlinable is
  asked of Rust through a primitive), superinstructions, and the
  constant pool.
- `analyze.scm`: free variables in first-reference order (codegen
  depends on it), boxed variables and lambda groups.
- The Rust front end feeds the Scheme codegen, through P13.0's
  converter.
- The Scheme codegen runs where the VM can call it: the REPL, `-e`,
  `-f`, programs and `--compile`. Library files and `eval` compile inside
  a primitive (constraint 1), so they keep the Rust codegen until P13.4.
- How the Scheme compiler is loaded: compiled by the Rust compiler at
  startup and cached as an image next to `s2-core.s2o` (C5).
- **Exit:** identical `--dis` over the corpus with `S2_COMPILER=scheme`.
  **Measure** compile time, as the first figure for P13.6.

### P13.2. (Folded into P13.1)

### P13.3. The front end in Scheme

**Changed in P13.3:**

- Macros stay Rust objects (C4 changed), so step 5 goes.
- Step 2's separate test of the matcher was skipped. The oracle covers
  every macro use in the corpus, and it passed first time.
- `pending_load` stays: library files and `eval` still compile inside
  a primitive, with the Rust front end, which needs it. The Scheme front
  end calls `%load-library` directly. (P13.4 moved its removal on to
  P13.5.)

The largest sub-phase, built in steps, each tested before the switch:

1. **Identifiers and aliases** through primitives (C3). `strip` and
   `strip-datum`.
2. **`syntax-rules`** in Scheme (C4): parsing, matching (with ellipses,
   literals, vectors, improper patterns) and instantiation. Tested on
   its own against the Rust matcher, over every macro use the corpus
   makes, with a literal-comparison function supplied by the test.
3. **The expander** (`expand.scm`): core forms, bodies with two passes,
   named `let`, quasiquote, `let-syntax`, and the variable numbering the
   Rust expander uses.
4. **Library declarations** (`libraries.scm`): import sets, `define-library`,
   `include`, `cond-expand`. Loading a library file is now a direct call,
   so `pending_load` and `%load-then-eval` go.
5. **Compiled files** store macros as data (C4). The fasl version is
   bumped.

- **Exit:** identical `--expand` and `--dis` over the corpus. The macro
  and library tests pass, including errors from macro expansion.

### P13.4. The driver

- Every caller of the compiler goes through Scheme (C6): the REPL, `-e`,
  `-f`, programs, `--compile`, `load`, library files and `eval`.
- Compile errors as Scheme errors (C9). Rust callers pin what they hold
  across a compile (constraint 5).
- **Exit:** all tests with `S2_COMPILER=scheme`. The GC stress runs
  matter most here: this is where rooting bugs appear.

**Changed in P13.4:**

- **The two front ends stay selectable**, so the switch is two cells.
  With `S2_COMPILER=scheme`, s2 binds the system's `eval` and
  `%load-library` to the compiler's `eval-form` and `load-library`.
  `load`, imports, library files, and Rust's own loading all call
  through those cells, so no compile reaches Rust.
- **`pending_load` and `%load-then-eval` stay, unused in that mode**,
  for the Rust front end. They go with it, in P13.5. The fasl reader
  uses `pending_load` too, when a compiled file needs a library loaded
  first: it will report that need as an error value instead.
- **A library file compiles one form at a time**, each run before the
  next is compiled, so a later `define-library` can import an earlier
  one. The Rust path compiles the whole file first, and falls back to
  `eval`, form by form, if that fails.

### P13.5. Self-compilation

- The Scheme compiler, built by the Rust compiler (stage 1), compiles
  its own source to an image (stage 2). Stage 2 compiles it again
  (stage 3).
- The cargo feature `rust-compiler`, on by default, leaves the Rust
  compiler out when off. `pending_load` and `%load-then-eval` go with it
  (moved from P13.4).
- **Exit:** stages 1, 2 and 3 are byte-identical. Identical code (C7)
  makes this expected rather than hoped for. With
  `--no-default-features` (no Rust compiler), s2 starts from a stage-2
  image and passes all tests.

### P13.6. Measurement and decisions

- **Compile speed**, Scheme against Rust:
  - startup;
  - the REPL's time per form;
  - `--compile` of chibi's suite and of the compiler itself;
  - loading a library file the first time.
- **Run-time speed:** unchanged by construction. Checked on a few of
  07's benchmarks.
- **Decides:**
  - whether the Rust compiler is removed or kept behind the feature;
  - whether the image is committed (C5 b);
  - whether a runtime without a compiler is offered (06 §2);
  - what follows: procedural macros, hash tables, tuning the compiler.

## Risks

| Risk | Mitigation |
|---|---|
| **Compiling is much slower.** 06 expected several times slower. The REPL and first loads would feel it; programs and libraries compiled ahead would not | Measured from P13.1 on. The likely costs are lookups through primitives and allocation. The image keeps startup fast whatever compile speed is |
| **Matching the Rust compiler byte for byte is fiddly** (variable numbering, constant order) | The diff names the exact form. Where matching would distort the Scheme code badly, a difference can be accepted on purpose, recorded, and checked by hand |
| **Bad code reaching the VM.** Since P12's fixed stack, the dispatch loop reads stack slots unchecked in release builds, so a wrong slot from a codegen bug is a memory error, not a panic. `%make-template` checks constants, jumps and free variables, not slots | The oracle (C7) catches wrong code before it ships. The test suites also run in debug builds, where every slot access is checked. A full check needs the stack depth at each instruction: a verifier, worth adding if the Rust compiler goes |
| **Rooting bugs** in Rust code that now spans a collection | GC stress and poisoning on every sub-phase. Pinning at the few Rust call sites (C6 keeps them few) |
| **A broken image** stops s2 from starting | During P13 the Rust compiler rebuilds it (C5 a). The tag is the last resort |
| **Deep recursion** in the compiler | It now runs on the VM's stack, which allows millions of frames. The large Rust stack in `main` and `.cargo/config.toml` can go once the Rust compiler does |
| **The front end switch is large** | It's built in tested steps (P13.3), and the old front end stays selectable throughout |

## What P13 doesn't do

- **Procedural macros** (explicit renaming, `syntax-case`). C4 makes
  them possible; they are a phase of their own.
- **Optimisations beyond the Rust compiler's.** The code must match it
  first (C7). Optimising in Scheme (constant folding, inlining known
  procedures) is easier afterwards, and is a natural next phase.
- **A re-entrant VM.** Not needed once every compile is called from
  Scheme (C6).
- **Moving the reader or builtins into Scheme.** 06 §2: they stay in
  Rust, being faster there.
