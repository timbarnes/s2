# Macros

R7RS: [section 4.3, Macros](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-6.html#TAG:__tex2page_sec_4.3). Syntax definitions are in [section 5.4](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-7.html#TAG:__tex2page_sec_5.4).

s2 has one macro system, R7RS's `syntax-rules`: pattern-based and **hygienic**. Hygiene is implemented by renaming: identifiers a template introduces become *aliases* that remember where the macro was defined.

Macros are expanded when a top-level form is compiled, before any of it runs. So a macro must be defined by an earlier top-level form than its uses (or earlier in the same body), and every use in a form is expanded, whether that code ever runs or not. Macros aren't values: using a macro's name as an expression is an error.

## Defining macros

### `define-syntax`

`(define-syntax keyword transformer)`

Binds `keyword` to a transformer, normally a `syntax-rules` form. Works at top level and inside bodies (an internal `define-syntax` is part of the body's scope, like an internal `define`).

```scheme
(define-syntax swap!
  (syntax-rules ()
    ((_ a b) (let ((tmp a)) (set! a b) (set! b tmp)))))
```

### `let-syntax` and `letrec-syntax`

`(let-syntax ((keyword transformer) ...) body ...)`
`(letrec-syntax ((keyword transformer) ...) body ...)`

Bind macros for the extent of `body`, which runs in a new scope: definitions inside it are local. With `letrec-syntax` the transformers can refer to each other and to themselves.

## `syntax-rules`

`(syntax-rules (literal ...) (pattern template) ...)`
`(syntax-rules ellipsis (literal ...) (pattern template) ...)`

A use of the macro is matched against each pattern in turn. The first that matches is instantiated with its template. A use that matches no rule is an error (`no syntax rule matches ...`), reported when the form containing it is compiled.

### Patterns

* The first element of a pattern stands for the keyword and is ignored.
* An identifier is a **pattern variable** and matches anything, unless it is `_` or a literal.
* `_` matches anything without binding it.
* A **literal** (an identifier listed in the literals) matches an identifier only if both refer to the same binding, or both are unbound and have the same name. So `else` matches `else`, but not an `else` that the use site has bound as a variable.
* `p ...` matches zero or more forms matching `p`. The ellipsis can appear anywhere in a list, followed by more patterns, and before a dotted tail: `(a b ... c d . rest)`.
* Vector patterns `#(p ...)` work like list patterns.
* Other data (numbers, strings, characters, booleans) match by `equal?`.

### Templates

* Pattern variables are replaced by what they matched. A variable matched under an ellipsis must be used under at least as many: `(list x ...)`. Several ellipses flatten: `(b ... ...)`.
* `(... template)` escapes the ellipsis: `(... ...)` is a literal `...`, and `'(... (x ...))` quotes the list `(x ...)` with `x` substituted. This is how a macro writes a macro that uses `...`.
* A custom ellipsis identifier, given before the literals, replaces `...` within that `syntax-rules` form: `(syntax-rules dots () ((_ x dots) (list x dots)))`. A literal takes priority over the ellipsis.

Malformed rules (an ellipsis with nothing before it, two ellipses in one list, a pattern variable used twice or under too few ellipses) are reported when the `define-syntax` is compiled. A template that repeats something with no pattern variable under the ellipsis is reported at its first use.

### Hygiene

Identifiers that a template introduces (not pattern variables) behave as if renamed:

* **They mean what they meant where the macro was defined.** A template's `if`, `list` or `counter` refers to the definition-site binding even if the use site binds its own. `set!` on such an identifier assigns the definition-site variable.
* **They can't capture the user's identifiers.** `swap!`'s `tmp` is a different variable from a `tmp` passed in by the user: `(let ((tmp 1) (y 2)) (swap! tmp y) (list tmp y))` gives `(2 1)`.
* **Definitions they introduce are hidden.** If a template expands to `(define helper ...)`, `helper` is visible to the code the same expansion produced, including macros it defines, but not to user code, even at top level. Names the user passes in (`(define-getter name ...)`) are defined normally.
* **Quoted data is plain.** Symbols inside a quoted part of a template, a quasiquoted part, or a vector literal are ordinary symbols: `'(a b)` in a template yields a list whose `a` is `eq?` to the user's `'a`.

The derived forms in `scheme/core.scm` (`cond`, `case`, `do`, `guard`, `parameterize`, ...) are `syntax-rules` macros, and so are hygienic in the same way: binding `lambda`, `let`, `if` or `loop` locally can't affect them.

### Errors appear when a form is compiled

Every macro use in a top-level form is expanded when the form is compiled, so a malformed use is reported even in code that never runs, and the whole form fails: in `(define (f) (bad-use))`, `f` isn't defined.

`(syntax-error message arg ...)` in a template reports a malformed use: it is an error whose message is the string `message` followed by the `arg`s, unevaluated.

```scheme
(define-syntax must-be-pair
  (syntax-rules ()
    ((_ (a . b)) 'pair)
    ((_ x) (syntax-error "must-be-pair: not a pair" x))))
(must-be-pair oops)   ; error: must-be-pair: not a pair oops
```

### Auxiliary syntax

`else`, `=>`, `_` and `...` are bound in `(scheme base)`, so they can be exported, imported and renamed like other syntax: after `(import (rename (only (scheme base) else) (else otherwise)))`, `otherwise` works as `else` in `cond` and `case`. Using one as an expression, as in `(else 1)`, is an error. A local variable named `else` or `=>` is an ordinary variable, and `cond`, `case` and `guard` don't treat it as a keyword.

### Performance

A macro costs nothing at run time: each use is expanded once, when its form is compiled, and the compiled code is what runs.

## Inspecting expansions

`s2 --expand` prints each top-level form after full expansion, as the core forms the compiler sees, before running it. Renamed identifiers print with a number after their name, so two different `tmp`s can be told apart:

```
$ s2 --expand -e '(let loop ((i 0)) (if (< i 3) (loop (+ i 1))))'
((letrec* ((loop.1 (named-lambda (loop i.2) (if (< i.2 3) (loop.1 (+ i.2 1)) #<unspecified>)))) loop.1) 0)
```

`--dis` prints the bytecode each form compiles to, and `(dis 'datum)` (an s2 extension) prints what `datum` would compile to, without running it: `(dis '(lambda (x) (+ x 1)))`.
