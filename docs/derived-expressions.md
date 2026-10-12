# Derived Expression Types

R7RS: [section 4.2, Derived expression types](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-6.html#TAG:__tex2page_sec_4.2). The report calls these forms "derived" because they can be defined in terms of the [primitive expressions](./primitive-expressions.md), and gives reference definitions in [section 7.3](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-9.html#TAG:__tex2page_sec_7.3).

In s2, `let`, named `let`, `letrec`, `letrec*`, `begin` and `quasiquote` are core forms of the compiler. The others (`cond`, `case`, `and`, `or`, `when`, `unless`, `let*`, `do`, `let-values`, `let*-values`, `define-values`, `parameterize`, `delay`, `delay-force`, `case-lambda`, `guard`) are hygienic `syntax-rules` macros in `scheme/core.scm`. Either way they are hygienic: rebinding `if`, `let` or `lambda` locally can't change what they do.

`guard` is described in [Exceptions](./exceptions.md#guard), and `cond-expand` in [Libraries](./libraries.md#cond-expand-and-features).

## Conditionals

### `cond`

`(cond clause ...)`

Each clause is `(test expr ...)`. The tests are evaluated in order; the first that is true has its expressions evaluated, and the last one's value is the result. A clause with no expressions returns the value of its test. The last clause may be `(else expr ...)`. A clause `(test => proc)` calls `proc` with the test's value:

```scheme
(cond ((assv 'b '((a 1) (b 2))) => cadr)
      (else #f))                          ; => 2
```

If no clause is true and there is no `else`, the result is unspecified.

### `case`

`(case key clause ...)`

Evaluates `key` once and compares it with `eqv?` against the data in each clause `((datum ...) expr ...)`. The first matching clause's expressions are evaluated. An `else` clause matches anything. A clause of the form `((datum ...) => proc)` or `(else => proc)` calls `proc` with the key.

```scheme
(case (car '(c d))
  ((a e i o u) 'vowel)
  ((w y) 'semivowel)
  (else => (lambda (x) x)))          ; => c
```

### `and`, `or`

`(and test ...)` evaluates the tests left to right and stops at the first false one, returning `#f`; otherwise it returns the last test's value, or `#t` if there are none. `(or test ...)` stops at the first true value and returns it, or returns `#f`. The last test is in tail position.

### `when`, `unless`

`(when test expr1 expr2 ...)` evaluates the expressions in order if `test` is true; `(unless test expr1 expr2 ...)` if it is false. The result is that of the last expression, or unspecified if they don't run.

## Binding constructs

Each of these binds variables in a new region, then evaluates a body. A body may start with [internal definitions](./program-structure.md#internal-definitions), and its last expression is in tail position.

### `let`, `let*`

`(let ((var init) ...) body ...)` evaluates all the inits, then binds them to the variables. The inits can't see the new variables. `(let* ((var init) ...) body ...)` binds them one at a time, so each init sees the variables before it:

```scheme
(let ((x 2) (y 3))
  (let* ((x 7) (z (+ x y)))
    (* z x)))                ; => 70
```

### `letrec`, `letrec*`

`(letrec ((var init) ...) body ...)` binds the variables first and then evaluates the inits in their scope, so the inits can be mutually recursive procedures:

```scheme
(letrec ((even? (lambda (n) (if (= n 0) #t (odd? (- n 1)))))
         (odd?  (lambda (n) (if (= n 0) #f (even? (- n 1))))))
  (even? 100))               ; => #t
```

`letrec*` evaluates the inits left to right, each able to use the values of the variables bound before it. s2's `letrec` already behaves this way. In both, an init that uses the value of a variable not yet initialized is an error.

### Named `let`

`(let name ((var init) ...) body ...)`

Like `let`, but also binds `name` within the body to a procedure whose parameters are the variables and whose body is `body`. Calling `name` in tail position loops:

```scheme
(let loop ((i 0) (acc '()))
  (if (= i 3)
      (reverse acc)
      (loop (+ i 1) (cons i acc))))     ; => (0 1 2)
```

### `let-values`, `let*-values`

`(let-values ((formals init) ...) body ...)`

Each `init` returns as many values as `formals` describes, and they are bound to its variables. `formals` is a list `(a b)`, a dotted list `(a . rest)` or a single variable that receives all values as a list. `let-values` evaluates all the inits before binding anything; `let*-values` binds each in turn, so later inits see earlier variables.

```scheme
(let*-values (((root rem) (exact-integer-sqrt 32))) (* root rem))   ; => 35
```

### `define-values`

`(define-values formals expr)`

Defines the variables in `formals` (as for `let-values`) from the values of `expr`, at top level or in a body. Other definitions may follow it in the same body.

## Sequencing

### `begin`

`(begin expr1 expr2 ...)` evaluates the expressions in order and returns the last one's value. At top level, or at the start of a body, `(begin definition ...)` splices its definitions into the surrounding scope, which is how a macro can expand into several definitions.

## Iteration

### `do`

`(do ((var init step) ...) (test result ...) command ...)`

Binds each `var` to its `init`, then repeats: if `test` is true, evaluate the `result` expressions and return the last one's value (unspecified if there are none); otherwise evaluate the `command`s for effect, then rebind each `var` to the value of its `step`. A variable without a `step` keeps its value.

```scheme
(do ((vec (make-vector 5))
     (i 0 (+ i 1)))
    ((= i 5) vec)
  (vector-set! vec i i))     ; => #(0 1 2 3 4)
```

Named `let` is the other way to loop. Both run in constant space however many times they repeat.

## Delayed evaluation

### `delay`, `delay-force`, `force`

`(delay expr)` returns a promise to evaluate `expr`. `(force promise)` evaluates it the first time and returns the same value every time after. Forcing a non-promise returns it unchanged.

`(delay-force expr)`, where `expr` evaluates to a promise, is for iterative lazy algorithms: forcing a long chain of `delay-force` promises runs in constant space.

### `make-promise`, `promise?`

`(make-promise obj)` returns `obj` if it is a promise, otherwise a promise already forced to `obj`. `(promise? obj)` tests for promises. A promise is a [record](./records.md) of a type private to `core.scm`.

## Dynamic bindings

### `make-parameter`

`(make-parameter value [converter])`

Returns a parameter object: a procedure that, called with no arguments, returns its current value. With a `converter`, the initial value and every value given by `parameterize` are passed through it first (it can check or normalise them).

### `parameterize`

`(parameterize ((param value) ...) body ...)`

Evaluates the body with each parameter set to its (converted) value, restoring the previous values when control leaves the body by any means: a normal return, an error, or a continuation. Re-entering through a continuation reinstates the parameterized values.

```scheme
(define radix (make-parameter 10))
(define (f n) (number->string n (radix)))
(f 12)                            ; => "12"
(parameterize ((radix 2)) (f 12)) ; => "1100"
```

The current ports are parameters, so `parameterize` can redirect output (see [Input and Output](./input-and-output.md#current-ports)).

## Quasiquote

`` (quasiquote template) ``, or `` `template ``

Builds list and vector structure from a template, like `quote`, except that parts marked with `,` (`unquote`) are evaluated and inserted, and parts marked with `,@` (`unquote-splicing`) are evaluated and their elements spliced in:

```scheme
`(1 ,(+ 1 1) ,@(list 3 4))          ; => (1 2 3 4)
`#(a ,(* 2 3))                       ; => #(a 6)
```

Quasiquotes nest: an inner `` ` `` raises the level, and only unquotes at the outermost level are evaluated. `write` prints the nested forms in their long form, as `(quasiquote ...)` and `(unquote ...)`.

## Procedures

### `case-lambda`

`(case-lambda (formals body ...) ...)`

A procedure that, when called, runs the first clause whose `formals` accept that many arguments. Calling it with a number of arguments no clause accepts is an error. This is R7RS's way to write procedures with optional arguments.

```scheme
(define range
  (case-lambda
    ((e) (range 0 e))
    ((b e) (do ((r '() (cons e r)) (e (- e 1) (- e 1))) ((< e b) r)))))
(range 3)      ; => (0 1 2)
(range 3 5)    ; => (3 4)
```
