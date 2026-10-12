# Libraries

R7RS: [section 5.2, Import declarations](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-7.html#TAG:__tex2page_sec_5.2), [section 5.6, Libraries](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-7.html#TAG:__tex2page_sec_5.6), `cond-expand` in [section 4.2.1](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-6.html#TAG:__tex2page_sec_4.2.1), and the lists of [standard libraries](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-10.html#TAG:__tex2page_chap_A) and [feature identifiers](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-11.html#TAG:__tex2page_chap_B) in appendices A and B.

Programs run in the interaction environment, which already has every standard binding (see [Environments and Evaluation](./environments.md)), so a program needs `import` only for its own libraries or to rename. A program that starts with the usual R7RS `(import (scheme base) ...)` runs unchanged. The design is in [P8](https://github.com/timbarnes/s2/blob/main/plan/phases/P8.md) and [P11](https://github.com/timbarnes/s2/blob/main/plan/phases/P11.md).

## Standard libraries

The R7RS standard libraries exist as export lists over the system environment: `(scheme base)`, `(scheme case-lambda)`, `(scheme char)`, `(scheme complex)`, `(scheme cxr)`, `(scheme eval)`, `(scheme file)`, `(scheme inexact)`, `(scheme lazy)`, `(scheme load)`, `(scheme process-context)`, `(scheme read)`, `(scheme repl)`, `(scheme time)`, `(scheme write)` and `(scheme r5rs)`. Names a library should export but s2 doesn't define are left out: only the complex-number procedures of `(scheme complex)` and `(scheme r5rs)`. `(%library-unimplemented library-name)` lists them. `(s2)` exports everything else in the system environment: s2's extensions and its internal procedures.

* `(library-names)`: the registered libraries' names, such as `(scheme base)`. An s2 extension.
* `(library-exports library-name)`: the names a library exports, as a list of symbols. An s2 extension.

## `import`

`(import import-set ...)`

Binds the names the import sets denote in the current top-level environment. An import set is:

* a library name, such as `(scheme base)`: all its exports;
* `(only set identifier ...)`: just those names;
* `(except set identifier ...)`: all but those names;
* `(prefix set prefix)`: every name with `prefix` in front;
* `(rename set (from to) ...)`: those names renamed.

Naming an identifier the set doesn't contain is an error. Every set is checked before anything is bound, so an import that fails binds nothing.

```scheme
(import (prefix (scheme char) c:))
(c:char-upcase #\a)                                    ; => #\A
(import (rename (only (scheme base) car) (car first)))
(first '(1 2))                                         ; => 1
```

An imported name shares the library's variable, and can't be `set!`. `import` is allowed only at the top level of an environment (the REPL, a loaded file, or `eval` in a mutable environment), not inside a body. At the REPL, importing a name replaces any binding it had, and a later `define` of it replaces the import.

## `define-library`

`(define-library library-name declaration ...)`

Defines a library. Its body runs in a new environment that contains only what the library imports, and the library is registered under `library-name` once the body has run. The declarations are:

* `(export spec ...)`: each `spec` is a name the library defines (or imports), or `(rename internal external)` to export it under another name.
* `(import import-set ...)`: as for `import`. Importing one name with two different bindings is an error.
* `(begin form ...)`: body forms.
* `(include file ...)`, `(include-ci file ...)`: body forms read from files (`include-ci` folds case).
* `(include-library-declarations file ...)`: further declarations read from files.

```scheme
(define-library (example counter)
  (export count inc!)
  (import (scheme base))
  (begin
    (define count 0)
    (define (inc!) (set! count (+ count 1)))))

(import (example counter))
(inc!)
count                 ; => 1: importers see the library's assignments
(set! count 5)        ; error: count is imported
```

* A library sees only its imports: a library that calls `char-upcase` must import `(scheme char)`.
* An exported `syntax-rules` macro may use the library's unexported definitions, and may assign the library's variables.
* Exporting a name the body never defines is an error, and a library whose body raises an error isn't registered, so it can be corrected and evaluated again.
* `define-library` is allowed only at top level. Evaluating it again replaces the library for later imports; existing importers keep the variables they imported.
* Files named in `include` declarations are relative to the library file's directory when the library was loaded from a file (below), and otherwise to the current directory.

* `(cond-expand clause ...)` declarations choose further declarations, as below.

## Library files

When `import` (or a library's `import` declaration) names a library that isn't registered, s2 looks for a file named after it: the name's parts joined by `/`, with `.sld` added, so `(foo bar)` is `foo/bar.sld`. It searches, in order:

1. the directories in the environment variable `S2_LIBRARY_PATH`, separated as in `PATH` (by colons on Linux and macOS, semicolons on Windows);
2. the current directory;
3. the installation's library directory: `share/s2/lib` under the prefix s2 is installed in (`/usr/local/share/s2/lib` for `/usr/local/bin/s2`).

The first file found is loaded in the interaction environment, normally defining the library with `define-library`, and the import goes ahead. A library file may itself import libraries from files.

**Compiled library files.** Loading `foo/bar.sld` compiles it and writes the result beside it, as `foo/bar.s2o`. Later runs load that file instead of compiling again, as long as it is *fresh*: `foo/bar.sld`, and every file it `include`s, still has the modification time it had when it was compiled, and the running s2 was built before the file was written. Otherwise the library is compiled again. Caching is best effort: if the `.s2o` file can't be written, s2 compiles the library on every run. A library file that can't be compiled ahead (one whose second `define-library` imports its first) is evaluated form by form instead.

Each file is loaded at most once per run. If it doesn't define the library its name implies, the import is an error (`import: ./foo/bar.sld does not define (foo bar)`), and importing again gives the same error without reloading. After fixing such a file, `(load "foo/bar.sld")` loads it again. `environment` uses only registered libraries; it doesn't search for files.

## `include` and `include-ci`

`(include file ...)` reads the forms in the files and evaluates them in place of the `include`, as a `begin`. `(include-ci file ...)` does the same, reading with case folding. Both work anywhere an expression or definition can appear.

## `cond-expand` and `features`

`(cond-expand (requirement form ...) ... [(else form ...)])`

Evaluates the forms of the first clause whose requirement holds, in place of the `cond-expand`, as a `begin`. If no clause holds and there is no `else`, the result is unspecified. In `define-library`, the forms are declarations instead. A requirement is:

* a feature identifier, true if it is in `(features)`;
* `(library library-name)`: true if the library is registered or its file is on the search path (it isn't loaded);
* `(and requirement ...)`, `(or requirement ...)`, `(not requirement)`.

```scheme
(cond-expand
  ((and s2 (library (scheme char))) (define upcase char-upcase))
  (else (define (upcase c) c)))
```

`(features)` returns s2's feature identifiers: `r7rs`, `exact-closed`, `ieee-float`, `full-unicode`, `ratios`, `s2`, and the operating system, its family, the architecture and the byte order, for example `macos unix posix aarch64 little-endian`. `full-unicode` is present: characters are Unicode scalar values, strings hold any of them, and the case, character-class and digit procedures follow Unicode. `exact-complex` is absent, since s2 has no complex numbers.
