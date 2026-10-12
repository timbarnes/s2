# Records

R7RS: [section 5.5, Record-type definitions](https://standards.scheme.org/corrected-r7rs/r7rs-Z-H-7.html#TAG:__tex2page_sec_5.5).

`(define-record-type type (constructor field ...) predicate (field accessor [modifier]) ...)`

Defines a new record type. `type` is bound to the record type, `constructor` to a procedure that makes a record from the named fields (others start unspecified), `predicate` to a test for records of this type, and each `accessor` and `modifier` to procedures that read and set a field. Accessors and modifiers raise an error when given anything but a record of their type.

```scheme
(define-record-type <pare>
  (kons x y)
  pare?
  (x kar set-kar!)
  (y kdr))

(kar (kons 1 2))          ; => 1
(pare? (cons 1 2))        ; => #f
```

`define-record-type` works at top level and in bodies. Each use creates a distinct type, even with the same name. A record prints as `#<<pare> 1 2>`, its type name followed by its field values.

`define-record-type` is a `syntax-rules` macro in `scheme/core.scm` over internal primitives (`%make-record-type`, `%record-make`, `%record?`, `%record-ref`, `%record-set-index!`). Each accessor finds its field's index once, when the type is defined. Promises are implemented as a record type.
