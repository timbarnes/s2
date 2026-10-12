# s2 Extensions

What s2 provides beyond R7RS-small. It is deliberately little: s2 is a small, standard core. The library `(s2)` exports these, so a library can `(import (s2))` to use them. A portable program should avoid them, or test for s2 with [`cond-expand`](./libraries.md#cond-expand-and-features) (`s2` is one of its features).

## Language

* **Square brackets** read as a vector: `[1 2 3]` is `#(1 2 3)`. See [Lexical Syntax](./lexical-syntax.md#s2-differences).
* **Programs needn't start with `import`.** See [Program Structure](./program-structure.md#programs).

## Procedures

* `(port? obj)`: `#t` for any port.
* `(environment? obj)`: `#t` for an environment.
* `(library-names)`, `(library-exports '(name ...))`: the registered libraries, and the names one exports. See [Libraries](./libraries.md#standard-libraries).
* `(dis 'datum)`: prints the bytecode `datum` compiles to, without running it. See [Macros](./macros.md#inspecting-expansions).

## Command line

* `--expand` and `--dis` print each form's expansion and bytecode before running it.
* `--compile` compiles a program to a file, and library files are compiled and cached automatically. See [System Interface](./system-interface.md#compiled-programs).

## Debugging

When an error isn't handled, s2 prints the names of the procedures that were waiting for a value, innermost first:

```
error: unbound variable: h
  in g
  in f
```

There is no debugger, tracer or source location in errors yet. For a Scheme with those, see [s1](https://github.com/timbarnes/s1), whose debugger and tracer s2 doesn't share.

## Environment variables

* `S2_LIBRARY_PATH`: directories to search for library files. See [Libraries](./libraries.md#library-files).
* `S2_GC_STRESS=1` collects garbage at every safepoint, and `S2_GC_POISON=1` overwrites freed objects. Both are for testing s2 itself, and make it much slower.

## Internal procedures

Names beginning with `%` (`%make-record-type`, `%frame-depth`, `%library-unimplemented`, ...) are the internal primitives that `scheme/core.scm`'s macros and procedures are built on. They are exported by `(s2)` but may change without notice.
