# Strings

R7RS: [section 6.7, Strings](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.7).

Strings hold Unicode characters; indexes count characters, not bytes. String literals support the escapes `\a \b \t \n \r \" \\ \|`, `\x3BB;` and a backslash at the end of a line (the line break and surrounding spaces are skipped). Procedures that take an optional `start` and `end` work on the characters from index `start` (default 0) up to, not including, `end` (default the length). All are built in.

## Construction and access

* `(string? obj)`, `(make-string k [char])`, `(string char ...)`, `(string-length s)`.
* `(string-ref s k)`, `(string-set! s k char)`.
* `(string-append s ...)`, `(substring s start end)`, `(string-copy s [start [end]])`.
* `(string-copy! to at from [start [end]])`: copies characters into `to` starting at index `at`; correct even when `to` and `from` are the same string.
* `(string-fill! s char [start [end]])`.

## Comparison

`(string=? s1 s2 s3 ...)`, `(string<? ...)`, `(string>? ...)`, `(string<=? ...)`, `(string>=? ...)` compare lexicographically by Unicode scalar value and take two or more arguments. `(string-ci=? ...)` and the other `-ci` forms compare after `string-foldcase`.

## Case

`(string-upcase s)`, `(string-downcase s)` use full Unicode case mapping (`(string-upcase "ßa")` is `"SSA"`, and a final capital sigma downcases to `ς`). `(string-foldcase s)` applies full case folding (`(string-foldcase "Maß")` is `"mass"`).

## Conversion

* `(string->list s [start [end]])`, `(list->string list)`.
* `(string->vector s [start [end]])`, `(vector->string vector [start [end]])`.
* `(string->symbol s)`, `(symbol->string sym)`; `(string->number s [radix])`, `(number->string z [radix])`.
* `(string->utf8 s [start [end]])`, `(utf8->string bytevector [start [end]])`: see [Bytevectors](./bytevectors.md).

## Iteration

`(string-map proc s1 s2 ...)` returns a string of `proc`'s results on corresponding characters; `(string-for-each proc s1 s2 ...)` calls `proc` for effect. Both stop at the shortest string.
