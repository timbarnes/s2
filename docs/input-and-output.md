# Input and Output

R7RS: [section 6.13, Input and output](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.13).

A port is an object that characters or bytes are read from or written to. Ports are shared: every reference to a port sees the same position and contents, and closing a port closes it everywhere. All of these procedures are built in except where noted.

## Kinds of port

* **Textual ports** read and write characters: standard input, output and error, string ports, and files opened with `open-input-file` / `open-output-file`.
* **Binary ports** read and write bytes: bytevector ports and files opened with `open-binary-input-file` / `open-binary-output-file`.

Character procedures on a binary port, or byte procedures on a textual one, raise an error.

* `(port? obj)`, `(input-port? obj)`, `(output-port? obj)`, `(textual-port? obj)`, `(binary-port? obj)`.
* `(input-port-open? port)`, `(output-port-open? port)`.

## Current ports

`(current-input-port)`, `(current-output-port)`, `(current-error-port)` return the ports that procedures use when no port is given. They start as standard input, output and error, and can be changed for a dynamic extent with `parameterize`:

```scheme
(let ((out (open-output-string)))
  (parameterize ((current-output-port out))
    (display "captured"))
  (get-output-string out))         ; => "captured"
```

The current input port is standard input even while a file is being loaded, so `(read)` in a loaded file reads from standard input, not from the file.

## Opening and closing

* `(open-input-file name)`, `(open-binary-input-file name)`: a file that can't be opened raises a `file-error?` error naming it.
* `(open-output-file name)`, `(open-binary-output-file name)`: create or truncate the file. Output to a file is buffered; the buffer is written when the port is flushed or closed, and every open file port is flushed when s2 exits, however it exits.
* `(open-input-string string)`, `(open-output-string)`, `(get-output-string port)`: string ports.
* `(open-input-bytevector bv)`, `(open-output-bytevector)`, `(get-output-bytevector port)`: bytevector ports.
* `(close-port port)`, `(close-input-port port)`, `(close-output-port port)`: a closed port can't be read or written, but still answers the port predicates.
* `(call-with-port port proc)`: calls `proc` with `port`, then closes it. `(call-with-input-file name proc)` and `(call-with-output-file name proc)` open a file and do the same. In `core.scm`.
* `(with-input-from-file name thunk)`, `(with-output-to-file name thunk)`: call `thunk` with the current input or output port set to the file. In `core.scm`.

## Input

Each takes an optional port, defaulting to the current input port (the byte procedures require a binary port). At end of input they return the eof object, which `(eof-object? obj)` tests for and `(eof-object)` returns.

* `(read [port])`: the next datum. Malformed input raises a `read-error?` error.
* `(read-char [port])`, `(peek-char [port])`: the next character, consumed or not.
* `(read-line [port])`: the characters up to the next line ending (`\n`, `\r` or `\r\n`), which is consumed but not returned.
* `(read-string k [port])`: up to `k` characters.
* `(char-ready? [port])`: `#t` if a character can be read without waiting (always, for string ports).
* `(read-u8 port)`, `(peek-u8 port)`, `(u8-ready? port)`, `(read-bytevector k port)`, `(read-bytevector! bv port [start [end]])`: bytes.

## Output

Each takes an optional port, defaulting to the current output port.

* `(write obj [port])`: the external representation, readable by `read`. Cyclic data is written with datum labels, so `write` always terminates: a list whose tail is itself prints as `#0=(1 . #0#)`.
* `(write-shared obj [port])`: datum labels for every pair or vector that appears more than once.
* `(write-simple obj [port])`: no datum labels (it loops forever on cyclic data).
* `(display obj [port])`: strings and characters appear as their raw text. Cycles are labelled as for `write`.
* `(newline [port])`, `(write-char char [port])`, `(write-string string [port [start [end]]])`.
* `(write-u8 byte [port])`, `(write-bytevector bv [port [start [end]]])`.
* `(flush-output-port [port])`: write out anything buffered.

### How objects print

`write` and `display` print data in the syntax `read` accepts (see [Lexical Syntax](./lexical-syntax.md)). Objects that R7RS gives no external representation print as opaque `#<...>` forms, which can't be read back:

* `#<procedure car>` for a built-in procedure.
* `#<procedure f>` for a closure whose `lambda` is the value of a `define`, `letrec` or internal definition, or the procedure of a named `let`: `(define (f) ...)` and `(define f (lambda () ...))` both print as `#<procedure f>`. The name is given when the procedure is compiled, so after `(define (adder n) (lambda (x) (+ x n)))` and `(define add1 (adder 1))`, `add1` prints as `#<procedure>`, as do `case-lambda` procedures, parameter objects and continuations.
* Syntax keywords, such as `if` or a macro, aren't values: using one as an expression is an error (`if: syntax used as a variable`).
* Ports print by kind: `#<input-port string>`, `#<output-port stdout>`, `#<output-port "out.txt">`, `#<binary-input-port bytevector>`, `#<closed-port>`.
* A record prints as its type name followed by its field values, `#<<point> 1 2>`; an error object as `#<error "message" irritant ...>`; an environment as `#<environment>`.
* An unspecified value, such as the result of `(if #f #f)`, prints as nothing, so the REPL shows nothing for `set!` or `vector-fill!`. Inside a list it leaves a gap: `(list 1 (if #f #f))` prints as `(1 )`. The end-of-file object prints as `#<eof>`.

## Files

These are in `(scheme file)`, with the procedures above that open files.

* `(file-exists? name)`.
* `(delete-file name)`: a file that can't be deleted raises a `file-error?` error.

## Loading

`(load filename [environment])` reads and evaluates the file's forms in turn; see [System Interface](./system-interface.md#load).
