# Bytevectors

R7RS: [section 6.9, Bytevectors](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-8.html#TAG:__tex2page_sec_6.9).

Bytevectors hold bytes (exact integers 0 to 255) and are written `#u8(1 2 3)`; they evaluate to themselves. `equal?` compares their contents. Procedures with optional `start` and `end` work on the bytes from index `start` (default 0) up to, not including, `end` (default the length).

* `(bytevector? obj)`, `(make-bytevector k [byte])`, `(bytevector byte ...)`, `(bytevector-length bv)`.
* `(bytevector-u8-ref bv k)`, `(bytevector-u8-set! bv k byte)`.
* `(bytevector-copy bv [start [end]])`, `(bytevector-append bv ...)`.
* `(bytevector-copy! to at from [start [end]])`: correct even when `to` and `from` are the same bytevector.
* `(utf8->string bv [start [end]])`: decodes UTF-8 (an error if the bytes aren't valid UTF-8).
* `(string->utf8 s [start [end]])`: encodes characters `start` to `end` of `s`.
