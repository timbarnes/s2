# Lexical Syntax

How s2 reads source text. R7RS: [chapter 2, Lexical conventions](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-4.html#TAG:__tex2page_chap_2), and the formal grammar in [section 7.1.1](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-9.html#TAG:__tex2page_sec_7.1.1).

## Identifiers

Identifiers are case sensitive: `Hello` and `hello` are different. Besides letters and digits, they can contain `! $ % & * / : < = > ? ^ _ ~ + - . @`, so `list->vector`, `<=?` and `set-car!` are ordinary identifiers, as are `+`, `-` and `...`. Any other characters can be included by writing the identifier between vertical bars: `|two words|`, `|foo\x41;|` (which is `fooA`).

`write` puts bars around a symbol that wouldn't read back as the same symbol, so `(string->symbol "two words")` prints as `|two words|`. See [Symbols](./symbols.md).

## Case folding

`#!fold-case` in the source makes the reader fold identifiers and character names to lower case from then on, as R5RS and earlier Schemes did; `#!no-fold-case` turns it off again. String contents are never folded.

```scheme
#!fold-case
(list 'ABC #\SPACE "ABC")      ; => (abc #\space "ABC")
#!no-fold-case
```

`include-ci` reads a file as if it began with `#!fold-case`.

## Whitespace and comments

* `;` comments out the rest of the line.
* `#| ... |#` is a block comment. Block comments nest.
* `#;` comments out the next datum, however many lines it spans: `(a #;(b c) d)` reads as `(a d)`.
* A line beginning `#!/` or `#! ` is a comment, so a script can start with `#!/usr/bin/env s2` (see [System Interface](./system-interface.md#running-s2)).

## Literals

Each kind of literal is described on the page for its type:

| Syntax | Example | Page |
| --- | --- | --- |
| Booleans | `#t`, `#true`, `#f`, `#false` | [Booleans](./booleans.md) |
| Numbers | `42`, `-7/3`, `1.5e-3`, `#xff`, `#e1.5`, `+inf.0` | [Numbers](./numbers.md#number-syntax) |
| Characters | `#\a`, `#\space`, `#\x3BB` | [Characters](./characters.md) |
| Strings | `"line\n"`, `"\x3BB;"` | [Strings](./strings.md) |
| Vectors | `#(1 2 3)` | [Vectors](./vectors.md) |
| Bytevectors | `#u8(1 2 3)` | [Bytevectors](./bytevectors.md) |
| Lists and pairs | `(1 2 3)`, `(a . b)` | [Pairs and Lists](./pairs-and-lists.md) |

Numbers, strings, characters, booleans, vectors and bytevectors evaluate to themselves. Lists and symbols must be quoted to be used as data.

`'datum`, `` `datum ``, `,datum` and `,@datum` are abbreviations for `(quote datum)`, `(quasiquote datum)`, `(unquote datum)` and `(unquote-splicing datum)`. See [Primitive Expressions](./primitive-expressions.md#quote) and [Derived Expressions](./derived-expressions.md#quasiquote).

## Datum labels

`#n=` labels the datum that follows it, and `#n#` refers back to it, so literal data can share structure or be circular:

```scheme
(define x '#0=(a b . #0#))   ; a circular list: a b a b ...
(car (cddr x))               ; => a
```

`write` uses the same notation when it prints cyclic data, so it always terminates. See [Input and Output](./input-and-output.md#output).

## s2 differences

* Square brackets are read as a vector: `[a b c]` is the same as `#(a b c)`, as in s1. In most Schemes, square brackets are an alternative to parentheses; in s2 they are not.
* Complex numbers such as `1+2i` are rejected by the reader (see [Numbers](./numbers.md)).
