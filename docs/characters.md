# Characters

R7RS: [section 6.6, Characters](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.6).

Characters are Unicode scalar values. Literals: `#\a`, `#\λ`, `#\x3BB`, and the names `#\alarm`, `#\backspace`, `#\delete`, `#\escape`, `#\newline`, `#\null`, `#\return`, `#\space`, `#\tab`. All of these procedures are built in.

## Comparison

`(char=? c1 c2 c3 ...)`, `(char<? ...)`, `(char>? ...)`, `(char<=? ...)`, `(char>=? ...)` compare by Unicode scalar value and take two or more arguments.

`(char-ci=? ...)`, `(char-ci<? ...)`, `(char-ci>? ...)`, `(char-ci<=? ...)`, `(char-ci>=? ...)` compare after `char-foldcase`.

## Classification

* `(char-alphabetic? c)`, `(char-upper-case? c)`, `(char-lower-case? c)`, `(char-whitespace? c)`: Unicode's Alphabetic, Uppercase, Lowercase and White_Space properties, so `(char-alphabetic? #\λ)` is `#t`.
* `(char-numeric? c)`: `#t` for a decimal digit in any script (Unicode category Nd), such as `#\x0E50` (Thai zero).
* `(digit-value c)`: the value 0 to 9 of such a digit, or `#f`: `(digit-value #\x0664)` is `4`.

## Conversion

* `(char->integer c)`, `(integer->char n)`: between characters and Unicode scalar values.
* `(char-upcase c)`, `(char-downcase c)`: Unicode case mapping. A character whose mapping is more than one character (`ß` upcases to `SS`) is returned unchanged.
* `(char-foldcase c)`: simple Unicode case folding, as used by the `-ci` comparisons.
