# 03. Architecture

The shape of the compiler and VM, and the design decisions behind it. Each
section lists the options, the trade-offs, and a recommendation marked
**Decision Dn**, which matches the table in the README. Nothing here is
fixed until the phase that needs it (04 says which phase settles each
decision). The point is that, by then, the trade-offs are already known.

---

## 1. The pipeline

```
 source text
     │  reader (forked from s1)                         reader/
     ▼
 datum (Values on the heap: pairs, symbols, literals)
     │  expander: macros, hygiene, scope resolution     expand/
     ▼
 core AST (a dozen node kinds; every variable resolved)
     │  analysis: assigned vars, captured vars,         compile/analyze.rs
     │  free vars of each lambda, tail positions
     ▼
 annotated core AST
     │  code generation                                 compile/codegen.rs
     ▼
 procedure templates (instructions + constants)
     │  VM: dispatch loop, value stack, frames          vm/
     ▼
 values ──► printer (forked from s1)
```

Every top-level form goes through the whole pipeline, one form at a time,
at the REPL, in `load`, and in `eval`. The output of compiling a top-level
form is a procedure of no arguments (a *thunk template*), which the VM then
calls. This one mechanism serves the REPL, `load`, `eval`, library bodies
and, later, bytecode files.

Why have both an expander and a separate core AST, rather than compiling
straight from s-expressions?

- **Separation of concerns.** The expander's job is syntax: macros, hygiene,
  scope, R7RS's body rules. The compiler's job is code: variables, closures,
  calls. Mixing them is what makes s1's `special_forms.rs` hard to follow.
- **Hygiene ends at the expander.** The expander gives every variable a
  unique identity (a `VarId`), so later passes never see aliases or renamed
  symbols, and never have to think about capture.
- **Analysis needs a whole lambda body.** Deciding whether a variable is
  assigned or captured requires seeing all its uses before generating code
  for any of them.
- **Each stage can be tested on its own:** expand and print the core AST,
  compile and print the disassembly.

---

## 2. The core language (Decision D11)

Every R7RS form expands into these node kinds. This is the language the
compiler actually compiles, and it is small on purpose.

```rust
enum Expr {
    Const(Value),                          // quote, self-evaluating data
    LocalRef(VarId),
    LocalSet(VarId, Box<Expr>),
    GlobalRef(GlobalId),                   // a binding cell (§8)
    GlobalSet(GlobalId, Box<Expr>),
    GlobalDefine(GlobalId, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Seq(Vec<Expr>),                        // begin
    Lambda(Rc<Lambda>),                    // params, optional rest, body
    CaseLambda(Vec<Rc<Lambda>>),
    Call(Box<Expr>, Vec<Expr>),
    Let(Vec<(VarId, Expr)>, Box<Expr>),    // an immediately applied lambda, kept as a let
    Letrec(Vec<(VarId, Expr)>, Box<Expr>), // letrec* semantics
}
```

- `Let` is not strictly needed, since `((lambda (x) body) e)` means the same
  thing. But keeping it avoids creating a closure for every `let`, and s1
  measured exactly that cost (61K closures in its regression suite).
- `Letrec` carries the `letrec*` semantics that R7RS gives internal
  definitions. How it compiles is a phase-by-phase story (§7).
- Everything else is derived: `cond`, `case`, `and`, `or`, `when`,
  `unless`, `do`, named `let`, `let*`, `let-values`, `define-record-type`,
  `parameterize`, `guard`, `delay`, `quasiquote`, and so on. Most become
  `syntax-rules` definitions in `scheme/core.scm`, as R7RS §7.3 shows.
  A few stay as Rust expander cases because they are core syntax, or
  because the error messages or performance are better that way: `lambda`,
  `define`, `set!`, `if`, `begin`, `quote`, `let-syntax`, `letrec-syntax`,
  `define-syntax`, `syntax-rules`, `define-library`, `import`, `include`,
  `cond-expand`. Probably also `quasiquote` (s1's lowering in
  `special_forms.rs:748-910` ports directly) and named `let`.

Alternatives considered:

- **Compile directly from s-expressions** (as many small Schemes do). Fewer
  types, but the expander and compiler get tangled, and analysis has nowhere
  to store its results.
- **A lower IR as well** (an ANF or CPS form between the AST and the
  bytecode). This is useful for optimisation, and it is how Guile's
  compiler tower and Chez's nanopass work. It is not needed to start; it can
  be added between analysis and codegen later if optimisation passes want
  it.

---

## 3. Value representation (Decision D2)

The single most important decision for speed (01 §4).

| Option | Layout | For | Against |
|---|---|---|---|
| **a. Rust enum** | `enum Value { Fixnum(i64), Flonum(f64), Char(char), Bool(bool), Nil, Unspecified, Eof, Undefined, Obj(GcRef) }`, 16 bytes, `Copy` | Safe, idiomatic, `match` works, unboxed floats come free | Twice the size of a word, so the stack and vectors use more memory |
| **b. Tagged 64-bit word** | Low bits as a tag: `...000` = fixnum (61 bits), `...001` = pointer, immediates elsewhere | 8 bytes. Fast type tests. This is what Chez, Guile, Chicken and most Lisps use | `unsafe` everywhere. Floats must be boxed. Harder to debug |
| **c. NaN-boxing** | Doubles stored directly; everything else hidden in NaN payloads | 8 bytes and unboxed floats. LuaJIT and many JS engines use it | 48-bit pointers only. Complex. Integers are awkward |
| d. s1's `GcRef` | Every value is a heap pointer | Reuse | Every integer allocates. Rejected |

**Recommendation: (a) behind an opaque interface.**

- Start with the enum. Never match on it outside a small `value.rs` module:
  everything else uses methods such as `v.as_fixnum()`, `v.is_pair()`,
  `Value::fixnum(n)`. Then option (b) can replace it in P10, if
  measurement says the extra 8 bytes matter, by changing one module.
- Define the fixnum range by constants (`FIXNUM_MIN`/`FIXNUM_MAX`) and
  never rely on full `i64`, so moving to (b) doesn't change behaviour.
- Arithmetic: `checked_add` on fixnums, with overflow promoting to a `BigInt`
  heap object. Bignum results that fit are normalised back to fixnums, so a
  number has exactly one representation and `eqv?` stays simple. The
  `num-bigint` and `num-rational` code from s1's `builtin/number.rs` carries
  over for the slow paths.

**Heap objects.** `GcRef` points at a `GcObject { mark: Cell<u64>, kind: ObjKind }`
with variants: `Pair`, `Symbol`, `String`, `Vector`, `Bytevector`,
`BigInt`, `Rational`, `Closure`, `Template` (compiled code), `Box`,
`Primitive`, `Continuation`, `Values`, `Record`, `RecordType`, `Port`,
`ErrorObject`, `Environment`, `Promise`, and so on.

**Fix s1's aliasing problem.** s1 hands out `&mut` from `&self`
(`heap.rs:237`), which is unsound. s2 can avoid this cheaply: because
`Value` is `Copy`, mutable fields can be `Cell<Value>`. A pair is
`{ car: Cell<Value>, cdr: Cell<Value> }`, and a vector is
`Box<[Cell<Value>]>`. Larger mutable payloads (strings, ports) use
`RefCell`. Then all access goes through shared references, and the only
`unsafe` is dereferencing a `GcRef`, in one place, which is valid as long as
the GC's invariant (§4) holds.

---

## 4. Heap and garbage collection (Decision D3)

| Option | For | Against |
|---|---|---|
| **Non-moving mark-sweep** (s1's design) | Simple. Pointers never change, so a `GcRef` in Rust code stays valid. Proven in s1 | Fragmentation. Allocation goes through `malloc` (mimalloc makes this acceptable) |
| Copying (Cheney) | Fast allocation (bump pointer). Compacts. Cost is proportional to live data | Every pointer held anywhere, including in Rust locals, must be updatable. Much harder in Rust |
| Generational | Best throughput for typical Scheme allocation | All of the above, plus write barriers on every mutation |
| `Rc` reference counting | No collector | Leaks cycles, which Scheme makes everywhere (closures, `letrec`) |

**Recommendation: keep s1's design,** which already works:

- one `Box` per object, with a list of all objects;
- an epoch counter instead of mark bits, so there is no unmark pass;
- an iterative mark worklist;
- collection scheduled in proportion to the live heap
  (`GcHeap::needs_gc`);
- a poison-sweep mode for debugging (`S1_GC_POISON`).

What changes is the **root set**, which is now the VM's:

1. the value stack;
2. the frame stack (each frame's closure, which reaches its template and
   constants);
3. the global table: binding cells of every top-level environment and
   library;
4. the symbol table;
5. the VM registers: current closure, the dynamic-wind list, current ports,
   the pending `Values`;
6. anything the expander or compiler holds *while* a collection could
   happen. That is never, under rule 1 below.

**The safepoint rule (from s1's `nested-evaluation.md`):**

1. **Allocation never collects.** `heap.alloc` only records that a
   collection is due.
2. The VM collects only **at specific instructions**: procedure entry
   (`CALL`/`TAILCALL`, which includes every loop, since Scheme loops are
   tail calls) and returning from a builtin. At those points every live
   value is in the roots above.
3. The reader, expander and compiler run entirely between safepoints. Their
   Rust locals are therefore safe without rooting. Their results become
   reachable (in a template, reached from the frame stack) before the VM
   reaches its next safepoint.
4. A builtin never calls back into the VM (§11), so it can't reach a
   safepoint while it holds values in Rust locals.

Upgrade path: generational or copying collection is a possible P10
experiment, but only if profiling shows the collector matters. s1's
measurements say that allocation as a whole was about 30% of run time, and
Scheme objects were a small part of that. Fixnums alone remove most
allocation in numeric code.

---

## 5. VM model (Decision D4)

| Model | How it works | For | Against |
|---|---|---|---|
| **Stack** | Instructions take operands from, and push results onto, a value stack. `(+ a b)` → `push a; push b; add` | The compiler is simple: generate code for subexpressions in post-order. Small instructions. Used by the JVM, CPython, clox, early Guile | More instructions per expression, so more dispatches |
| **Register** | Instructions name virtual registers (frame slots): `add r3, r1, r2` | Lua 5 measured about 47% fewer instructions than its stack VM. Fewer copies | The compiler needs register allocation (simple for a tree, but extra work). Wider instructions |
| **Accumulator + stack** | One accumulator register holds the most recent value, and a stack holds arguments. Dybvig's stack model | Fewer pushes than a pure stack. A very good match for Scheme | Somewhat less familiar. Two places a value can be |

**Recommendation: a stack VM.**

- It gives the clearest correspondence between source and instructions,
  which matters most for learning. Code generation is a direct recursive
  walk.
- The performance gap to a register machine is real, but it can be closed
  partly by superinstructions (fused common sequences) in P10.
- Converting to a register VM later would be a large change to codegen. If
  that becomes the goal, it is better done as a separate experiment once
  everything works.

**Layout.** Two stacks in the VM:

```
value stack (Vec<Value>):
   ... │ caller's temporaries │ callee │ arg0 … argN-1 │ locals │ temporaries │
                                ▲ fp

frame stack (Vec<Frame>):
   Frame { closure: GcRef, ip: usize, fp: usize }   // the caller's state, saved at CALL
```

This is clox's arrangement. It keeps the value stack free of return
addresses, makes frames simple to inspect for error messages, and makes
continuation capture a copy of two vectors (§10).

---

## 6. Instruction encoding and set (Decision D5)

| Encoding | For | Against |
|---|---|---|
| **`enum Op` ("wordcode")**: `enum Op { Const(u32), LocalRef(u16), Jump(i32), Call(u16), ... }`, `Vec<Op>` per template, 8 bytes per instruction | Readable, type-checked, dispatch by `match` (Rust compiles a dense `match` to a jump table), trivial to disassemble | Larger than bytes. Not a file format |
| Packed bytes: opcode byte + operand bytes | Compact, cache-friendly, needed for files anyway | Operand decoding in the loop, and a hand-written encoder and decoder |
| Threaded code (computed goto, or a tail call per handler) | The fastest dispatch for interpreters | Not available in stable Rust (`become` is unstable). Closure-threaded code is possible but changes everything |

**Recommendation: `enum Op` first.** It is still bytecode in the sense that
matters: a linear instruction sequence interpreted by a dispatch loop. A
byte encoding comes with bytecode files (P11). Measure then whether running
from bytes directly is faster. In Rust it often isn't, and the file loader
can decode into `Op`s.

**First instruction set.** About 25 instructions, grown phase by phase in 04.

| Group | Instructions |
|---|---|
| Constants | `Const(k)` (pool index), `Nil`, `True`, `False`, `Unspecified`, `Fixnum(i32)` (small literal, no pool entry) |
| Locals | `LocalRef(i)`, `LocalSet(i)` (slot relative to `fp`) |
| Closure fields | `FreeRef(i)` (field of the current closure) |
| Boxes (§7) | `MakeBox(i)` (box the local in slot `i` in place), `BoxRef`, `BoxSet` |
| Globals (§8) | `GlobalRef(k)`, `GlobalSet(k)`, `GlobalDefine(k)` (k names a cell in the pool) |
| Control | `Jump(off)`, `JumpIfFalse(off)`, `Pop` |
| Procedures | `MakeClosure(k, nfree)` (template `k`; captures the `nfree` values on the stack), `Call(argc)`, `TailCall(argc)`, `Return` |
| Arity | checked at entry from the template's arity, not by an instruction; `case-lambda` templates hold several entry points |
| Added in P7 | `CaptureCont`, `ResumeCont`, `Values` support (§10) |
| Added in P10 | inlined primitives (`Car`, `Cdr`, `Cons`, `Add`, `Sub`, `Lt`, `NumEq`, `EqP`, `NullP`, `PairP`, …; after P12 also `Mul`, `Div` and the vector accessors) and superinstructions (`JumpIfNot`; after P12 `CarLocal`, `CdrLocal`, `AddImm`, `SubImm`, `JumpIfNotImm`) |

**A worked example.** `(define (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2)))))`
after P2, where every primitive is a global call:

```
template fib  arity 1  nfree 0
  constants: k0 = cell <   k1 = cell fib   k2 = cell +   k3 = cell -
  0  GlobalRef k0        ; <
  1  LocalRef 0          ; n
  2  Fixnum 2
  3  Call 2
  4  JumpIfFalse →7
  5  LocalRef 0          ; n
  6  Return
  7  GlobalRef k2        ; +
  8  GlobalRef k1        ; fib
  9  GlobalRef k3        ; -
 10  LocalRef 0
 11  Fixnum 1
 12  Call 2              ; (- n 1)
 13  Call 1              ; (fib …)
 14  GlobalRef k1
 15  GlobalRef k3
 16  LocalRef 0
 17  Fixnum 2
 18  Call 2
 19  Call 1
 20  TailCall 2          ; (+ …) in tail position
```

And after P10's primitive inlining (§11):

```
  0  LocalRef 0
  1  Fixnum 2
  2  Lt                  ; inline, fixnum fast path
  3  JumpIfFalse →6
  4  LocalRef 0
  5  Return
  6  GlobalRef k1
  7  LocalRef 0
  8  Fixnum 1
  9  Sub
 10  Call 1
 11  GlobalRef k1
 12  LocalRef 0
 13  Fixnum 2
 14  Sub
 15  Call 1
 16  Add
 17  Return
```

Comparing these two listings shows where much of the speed of a bytecode
Scheme comes from.

---

## 7. Variables and closures (Decision D6)

| Option | How it works | For | Against |
|---|---|---|---|
| Linked environments | Each call allocates a frame on the heap, linked to the defining frame. Variables are (depth, index) | Simple. Closure creation is O(1) | Every call allocates. Access walks the chain. This is roughly s1's model, compiled |
| Upvalues (Lua, clox) | Locals live on the stack. Closures capture *open upvalues* that point into the stack, and are *closed* (copied to the heap) when the frame returns | Fast. Shares mutable variables correctly | Closing upvalues on every return. Interacts badly with copying stacks for `call/cc` (the pointers into the stack) |
| **Flat closures + assignment conversion** (Dybvig's stack model) | A closure copies the *values* of its free variables when created. A variable that is ever `set!` and also captured is put in a heap box, and the closure copies the box | Calls don't allocate. Captured access is one indexed load. No pointers into the stack, so copying the stack for `call/cc` is safe. Very common in Scheme compilers | Closure creation is O(free variables). Assigned captured variables cost a box |

**Recommendation: flat closures with assignment conversion.** It fits a
stack that `call/cc` copies (§10), it is the standard Scheme technique, and
it gives the analysis pass a clear job:

1. **Assigned variables:** any `VarId` that is the target of a `LocalSet`.
2. **Captured variables:** any `VarId` referenced inside a lambda other than
   the one that binds it.
3. A variable that is both is **boxed**. Its binding allocates a box
   (`MakeBox`), every reference becomes `BoxRef`, and every assignment
   becomes `BoxSet`. Assigned but uncaptured variables stay in stack slots.
4. Each lambda's **free variables** are the captured variables it
   references, from enclosing scopes. `MakeClosure` copies them into the
   closure, and inside the body they are accessed with `FreeRef(i)`.

**`letrec` and internal definitions, step by step:**

- **P4 (simple and correct):** `(letrec* ((v init) …) body)` becomes
  "bind each `v` to *undefined*, then `set!` each in order, then body". The
  assignment makes recursive closures boxed. Referencing a variable before
  its init is evaluated can be detected cheaply: `BoxRef` on an undefined
  value raises an error.
- **P10 (faster, from *Fixing Letrec*):** when every init is a `lambda`
  (the usual case for internal procedure definitions), allocate all the
  closures first, then fill in each one's free-variable slots. No boxes are
  needed, and mutually recursive local procedures cost nothing extra.

---

## 8. Global variables (Decision D7)

R7RS requires (01 §6, and s1's `precompilation-design.md`):

- late binding at the REPL (a procedure may refer to a global defined
  later, and a redefinition is seen by existing code);
- imports that share a binding with the library that exports it;
- one top level per library;
- environments as first-class values for `eval`.

**Recommendation: binding cells, as in s1.**

- A top-level environment maps names to cells. A cell holds a `Value`,
  which is `Undefined` while unbound.
- When the compiler meets a global reference, it resolves the name in the
  environment being compiled for, creating an unbound cell if none exists
  yet. It then puts the cell into the template's constant pool.
  `GlobalRef(k)` is one load from the cell, plus a check for unbound.
- `import` binds a name to an *existing* cell, so libraries and importers
  share it.
- `define` at top level fills the cell (`GlobalDefine`). Redefining a name
  at the REPL changes the same cell, so compiled code that refers to it sees
  the new value.
- Cells are heap objects in s2, so a template's constant pool can hold them
  as ordinary values and the GC handles them. s1 used `Rc` cells instead,
  for reasons specific to its environment frames.

Things that would fight R7RS, and must be avoided (from s1's
`precompilation-design.md`):

- caching or inlining the *value* of a mutable global;
- one global symbol-to-value table for everything (libraries each need
  their own top level);
- compiling without knowing which environment the code is for.

---

## 9. Procedure calls

- **Call sequence:** push the procedure, push the arguments, then
  `Call(argc)`. The VM checks the callee type:
  - **Closure:** check its arity, then collect any rest arguments into a
    list. Push a `Frame` with the caller's `ip`, `fp` and closure. Set
    `fp` to the callee's slot, and jump to the template's entry.
  - **Primitive** (a Rust builtin): call it with a slice of the arguments.
    Replace the procedure and arguments with the result, and continue.
  - **Continuation:** resume it (§10).
  - **Parameter object, record constructor, and so on:** these are closures
    or primitives in s2, so there are no extra cases.
  - **Anything else:** raise "not a procedure".
- **`Return`:** take the result, pop the frame, truncate the value stack to
  the callee slot, push the result, and continue at the saved `ip`.
- **Proper tail calls:** `TailCall(argc)` moves the procedure and arguments
  down over the current frame's slots, then enters the callee *without*
  pushing a `Frame`. A loop written as a tail call therefore runs in
  constant space. R7RS lists the tail contexts (§3.5); s1's
  `scheme/tail_tests.scm` checks all 31 of them, and the analysis pass marks
  them.
- **Rest arguments:** a template has `(required: u16, rest: bool)`.
  `case-lambda` is a template with several `(arity, entry)` pairs, and the
  VM picks the first that matches.
- **`apply`:** a VM-level operation (§11) that spreads its list onto the
  stack, then performs a `TailCall`.

---

## 10. Continuations and dynamic control (Decision D8)

R7RS needs re-entrant `call/cc`, `dynamic-wind`, multiple values,
`with-exception-handler`, `raise`, `raise-continuable`, `guard`,
`parameterize`, and `error`. s1 supports all of these, with re-entrant
continuations tested in `scheme/call_cc_tests.scm`.

**Capturing a continuation: the options.**

| Strategy | Capture | Invoke | Normal calls | Notes |
|---|---|---|---|---|
| Heap frames (s1's `Rc<Kont>` chain; Dybvig's heap model) | O(1): share the chain | O(1) | Each call allocates a frame | Fast continuations, slow calls |
| **Contiguous stack, copy on capture** (Dybvig's stack model) | O(depth): copy the stacks | O(depth): copy back | No allocation | Fast calls, slower continuations |
| Segmented stack (Hieb, Dybvig, Bruggeman) | O(1): seal the current segment and start a new one | O(segment) | No allocation, and a small cost at segment boundaries | What Chez uses. Best of both, but more complex |
| CPS conversion | Free: a continuation is an ordinary closure | O(1) | Allocates a closure per non-tail call, unless optimised heavily | Changes the whole compiler |

**Recommendation: copy on capture first, segmented stack as an upgrade.**

- `call/cc` copies the value stack and the frame stack into a
  `Continuation` heap object. Invoking it copies them back, pushes the
  value or values, and continues from the saved frame.
- This is simple and correct for re-entry and multi-shot use. Its cost is
  proportional to stack depth, which only matters for code that captures
  continuations deep in the stack many times (generators, coroutines).
- **Keep an escape-only form**, `call/ec` (s1's `%call/ec`). It records
  only the stack heights and a frame marker, and invoking it truncates the
  stacks after checking that the marker is still live. That is O(1).
  `guard` and other non-local exits in the library use it, as in s1.
- P10 measures a generator benchmark. If copying is a problem, move to a
  segmented stack. Clinger et al. compares the variants, and s1's
  `kont-flat-stack-design.md` ("Option 2: a chunked spine") already sketches
  the idea.

**Move dynamic control into Scheme.** A big simplification for the VM:

- The VM provides only *raw* continuation capture and resume: `%call/cc`,
  which knows nothing about `dynamic-wind`.
- `dynamic-wind`, and the `call/cc` that users see, are written in
  `scheme/core.scm` on top of it. They keep a wind list and run the
  `before`/`after` thunks when a continuation crosses wind frames. This is
  the standard technique, and R7RS's reference implementation and SRFI
  sample implementations show it.
- `with-exception-handler`, `raise`, `raise-continuable`, `error` and
  `guard` are also written in Scheme, on a handler stack kept in a
  parameter-like variable. Error objects stay records with Rust accessors.
- `make-parameter` and `parameterize` are also Scheme, as in s1.

Why: every "call Scheme code, then continue" in these features is then
ordinary compiled Scheme, and the VM never has to run Scheme from inside
Rust (lesson 2 in 01 §6). If profiling later shows that `dynamic-wind` or
`guard` are hot, specific parts can move into Rust.

**Multiple values.** Start with s1's representation: `(values a b)`
returns one `Values` heap object (or the plain value, for exactly one
value), and `call-with-values` unpacks it. This allocates on every multiple
return. A faster scheme, with values left on the stack and a count register,
is a P10 item, done only if it is measured to matter.

---

## 11. Builtins and primitives (Decision D9)

**Builtin signature (kept from s1):**
`fn(&mut Heap, &[Value]) -> Result<Value, Error>`. A builtin never calls
Scheme code. The VM turns an `Err` into a call to the Scheme-level `raise`
with an error object, as s1's `post_error` does.

**Builtins that need the VM** (`apply`, `%call/cc`, resuming a
continuation, `eval`, `values`, `call-with-values`) are not ordinary
builtins. Options:

- (a) Make them instructions, emitted when the compiler sees a call to the
  known binding.
- (b) **A second kind of primitive, `VmPrimitive`, that returns an action
  instead of a value:**
  `enum Action { Return(Value), TailCall { proc, args }, Resume(cont, vals) }`.
  The dispatch loop carries out the action.

**Recommendation: (b).** These primitives are still first-class values (you
can `(map apply …)`), and there are only about six of them. `eval` compiles
its argument into a thunk template and returns `TailCall` to it, so it never
re-enters the VM.

**Higher-order library procedures stay in Scheme**, as in s1: `map`,
`for-each`, `vector-map`, `string-for-each`, `assoc` and `member` with a
compare argument, and so on. A builtin can then never need to call a Scheme
procedure. Once the VM is fast, these are fast too.

**Inlining primitives (P10).** Emitting `Car` instead of
`GlobalRef car; Call 1` is the largest single optimisation in a bytecode
Scheme. Compare the two listings in §6. It is only correct if the
reference is known to mean the standard `car`:

- **In libraries and programs:** R7RS §5.6 makes imported bindings
  immutable. It is an error to redefine or `set!` them. So if the compiler
  resolves `car` to the cell imported from `(scheme base)`, it may inline.
- **At the REPL:** the interaction environment allows redefinition. s1
  gives a redefined imported name a fresh cell. Options: inline only when
  the cell is imported and unchanged at compile time, accepting that code
  compiled before a redefinition keeps the old meaning (the behaviour of
  Chez and most other compilers at their default settings), or never inline
  at the REPL. **Decide in P10.**
- An inlined primitive handles the fast case (fixnums, flonums, pairs, vector indexes) inline. For
  anything else it falls back to calling the real builtin, so error
  behaviour is identical.

---

## 12. The macro expander (Decision D10)

The expander turns datums into the core AST. It needs a **compile-time
syntactic environment**, which s1 lacks (01 §5).

**Syntactic environment:** a chain of scopes, each mapping an identifier
to a binding:

```rust
enum Binding {
    Local(VarId),            // lambda/let parameter, internal define
    Global(GlobalId),        // top-level variable: a cell in some environment
    Macro(Rc<Transformer>),  // syntax-rules
    Core(CoreForm),          // lambda, if, define, quote, ... (expander cases)
}
```

**Hygiene by renaming, ported from s1** (`design/hygiene-design.md`,
`src/eval/identifiers.rs`):

- When a `syntax-rules` template is instantiated, every symbol it
  introduces is replaced by an **alias**: a fresh identifier recording
  `(original symbol, definition scope)`.
- To resolve an identifier, look it up in the current scope chain. If it is
  an alias that isn't bound there (it wasn't bound by a form in the macro's
  output), resolve its original in the macro's *definition* scope. This is
  s1's `resolve`, except that it runs once, during expansion, instead of on
  every evaluation.
- Binding forms bind aliases like any other identifier. So a macro's
  temporaries can't capture user variables, and user bindings can't capture
  a macro's free identifiers.
- `quote` strips aliases back to plain symbols (s1's `strip_datum`).
- Literal matching (`else`, `=>`) uses `free-identifier=?` against the
  scopes (s1's `free_identifier_eq`).
- s1's `syntax_rules.rs` matcher and instantiator carry over. They need
  only an interface for comparing identifiers and creating aliases, in
  place of `EnvRef`.

**Alternatives considered:**

- **`syntax-case` with marks or sets of scopes** (Racket, Chez): more
  powerful (procedural hygienic macros) and the modern standard, but R7RS-small
  doesn't need it, and it is a project in itself.
- **Explicit renaming as a user feature** (`er-macro-transformer`): would
  need Scheme code to run at compile time. Possible, because the compiler is
  in-process, but not needed (decision U3).

**Bodies.** R7RS body semantics need care:

- A body is expanded form by form until the first expression that isn't a
  definition. A macro use may expand into a definition, so the expander has
  to expand the head of each form to find out which it is.
- `begin` in a body splices in its forms.
- The collected definitions become a `Letrec` (with `letrec*` semantics).
- Internal `define-syntax` adds a `Macro` binding to the body's scope.

**The top level** is handled one form at a time:

- `define-syntax` at top level binds the macro in the top-level syntactic
  environment, so macros are available to later forms.
- A reference to a name not yet defined resolves to a fresh unbound cell
  (§8).
- Using a macro before it is defined is an error, as in other compiling
  Schemes. s1 expands lazily, so it accepts some programs that s2 will
  reject. Document this difference.

**No Scheme code runs at expansion time.** The only transformers are
`syntax-rules`, and expanding them is pure Rust. This is a big
simplification, and it follows from leaving out s1's procedural `macro`
(decision U3).

---

## 13. Libraries, environments, `eval` and the REPL

- **A top-level environment** maps names to bindings: value cells
  (`Global`) and macros (`Macro`). R7RS libraries can export both.
- **Standard libraries** are built at startup from tables forked from s1's
  `libraries.rs`: which names each `(scheme …)` library exports, pointing
  at the system environment's cells and macros.
- **`define-library`:** collect the declarations, resolve the imports into
  a fresh environment (forked import-set code: `only`, `except`, `prefix`,
  `rename`), expand and compile the body in that environment, run it, then
  record the exports. Libraries found on the search path (`.sld` files,
  forked) are loaded on first import.
- **`eval expr env`:** a `VmPrimitive` that expands and compiles `expr` in
  `env` and tail-calls the resulting thunk (§11).
- **`environment`, `interaction-environment`,
  `scheme-report-environment`:** return environment values.
- **The REPL:** read, compile, run, print, in the interaction environment.
- **`load`:** read, `eval`, repeat, written in Scheme as in s1.
- **Programs** (`s2 file.scm`) follow R7RS §5.1: `import` declarations
  first, then the body, compiled as a whole.

---

## 14. Errors and source locations

- Builtins report errors as `Err`, which the VM converts into a raised
  error object (§11), so `guard` and handlers see builtin errors in the
  normal way.
- An uncaught error prints the message, the irritants and a backtrace. The
  backtrace walks the frame stack and prints each procedure's name (stored
  in its template).
- **Source locations (optional, P9 or later):** the reader records the
  position of each pair it builds in a side table. The expander carries
  positions into the core AST, and codegen writes a line table into each
  template (`ip → source position`). A backtrace can then show
  `file:line`. Nothing else depends on this, so it can be added at any
  point.

---

## 15. Tools for learning and debugging

These are not "s1 extras". They are how the compiler is understood and
debugged, and they are cheap.

- **Disassembler:** prints a template's instructions and constants, as in
  §6. It exists from P1. CLI flags `--dis` (print the code for every
  top-level form) and `--expand` (print the core AST).
- **Instruction trace:** a Cargo feature (`--features trace`) that prints
  each instruction and the top of the stack as it runs. It is compiled out
  normally, so it costs nothing.
- **GC stress:** an environment variable or flag to collect at every
  safepoint, and poison-sweep mode (from s1).
- **Statistics** (later, for P10): instruction counts per opcode, to choose
  superinstructions.

---

## 16. Proposed module layout

One crate, modules only (D1):

```
s2/
  Cargo.toml
  src/
    main.rs             CLI and REPL
    lib.rs
    value.rs            Value, the only place that matches on its layout
    heap.rs             objects, allocation, mark-sweep, symbol table
    sstring.rs          (forked)
    io.rs               (forked)
    printer.rs          (forked, adapted)
    reader/             tokenizer, parser, number_syntax (forked)
    expand/             syntactic env, hygiene, syntax_rules (forked), core forms, bodies
    compile/            ast.rs, analyze.rs, codegen.rs, disasm.rs
    vm/                 op.rs, template.rs, machine.rs (dispatch loop), cont.rs
    builtins/           numbers, lists, strings, chars, vectors, bytevectors, records, ports, ...
    libraries.rs        standard library tables, define-library, import (forked, adapted)
  scheme/
    core.scm            map, for-each, derived syntax, dynamic-wind, exceptions, parameters, records
  tests/                Rust integration tests, Scheme test files, the r7rs suite
  bench/                benchmarks (from s1) and bench.sh
  plan/                 these documents
```
