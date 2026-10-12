# Exceptions

R7RS: [section 6.11, Exceptions](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.11).

Any object can be raised. Errors are represented by **error objects**, which `error` creates and which built-in procedures raise when they fail.

## Handlers

### `with-exception-handler`

`(with-exception-handler handler thunk)`

Calls `thunk` with `handler` installed as the current exception handler. When something is raised during the call, `handler` is called with the raised object, with the handlers that were current outside this one installed. So an error inside a handler goes to the next handler out.

The handler is uninstalled when `thunk` returns, and also when control escapes out of `thunk` through a continuation. Re-entering through a continuation reinstalls it.

### `raise`

`(raise obj)`

Raises `obj`, calling the current handler. If the handler returns, a secondary error is raised in the handler's dynamic environment, with the message "exception handler returned from a non-continuable raise of" and `obj` as its irritant. Escape from a handler with a continuation or `guard` instead.

### `raise-continuable`

`(raise-continuable obj)`

Raises `obj`. If the handler returns, its value becomes the value of `raise-continuable`:

```scheme
(with-exception-handler
  (lambda (c) 42)
  (lambda () (+ (raise-continuable 'oops) 23)))   ; => 65
```

### `guard`

`(guard (var clause ...) body ...)`

Evaluates `body`. If it raises, control returns to the `guard`, running the `after` thunks of any `dynamic-wind` extents being left, and the raised object is bound to `var` while the clauses are evaluated as in `cond`, including `else` and `=>`. If no clause matches, the object is raised again with `raise-continuable`, in the dynamic environment of the original raise.

```scheme
(guard (e ((string? e) (string-append "caught " e))
          ((error-object? e) (error-object-message e)))
  (car 1))                                       ; => "car: expected a pair, got 1"
```

`guard` is a hygienic `syntax-rules` macro in `scheme/core.scm`, so its expansion can't be affected by the user's variables, including local bindings named `lambda`, `let` or `cond`.

`guard` catches what is raised while its body *runs*. Errors found while a top-level form is compiled, such as a macro use that matches no rule or a malformed `define`, end the whole form before anything in it runs, so no `guard` in that form can catch them.

## Error objects

### `error`

`(error message irritant ...)`

Raises a new error object with the given message (normally a string) and list of irritants.

### Predicates and accessors

* `(error-object? obj)`: `#t` for error objects.
* `(error-object-message e)`, `(error-object-irritants e)`: the message and the list of irritants.
* `(file-error? obj)`: `#t` for the error raised when `open-input-file` or `open-output-file` (and so `load`) can't open a file. Its irritant is the file name.
* `(read-error? obj)`: `#t` for the error raised when `read` meets malformed input.

When a built-in procedure fails, for example `(car 1)`, applying a non-procedure, the wrong number of arguments, or an unbound variable, it raises an error object whose message is the error text (`"car: expected a pair, got 1"`) and whose irritant list is empty.

An error object prints as `#<error "message" irritant ...>`.

## Uncaught exceptions

When nothing handles an exception, s2 runs the `after` thunks of any `dynamic-wind` extents that were active, innermost first, and prints it on standard error as `error: message irritant ...` (or `error: uncaught exception: obj` for a raised object that isn't an error object), followed by a backtrace: the names of the procedures that were waiting for a value, innermost first, up to ten.

```
error: unbound variable: h
  in g
```

A program then ends, with status 70. The REPL, `-e` and `-f` carry on with the next form.
