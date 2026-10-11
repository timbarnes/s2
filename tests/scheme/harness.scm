;; harness.scm: s1's test procedures (scheme/test-harness.scm: test-equal,
;; test-true and the rest), for the ported s1 test files, which
;; tests/scheme.rs runs each one after this. Only the reporting differs. A failing test prints a FAIL line; an error
;; in a test aborts its top-level form, which s2 reports on stderr.

;; The size of a test's long loop or deep recursion. tests/scheme.rs caps
;; it under GC stress, where every call collects and big sizes take hours.
(define (test-size n) n)

(define %tests-run 0)
(define %tests-failed 0)

(define (test-equal expected actual message)
  (set! %tests-run (+ %tests-run 1))
  (unless (equal? expected actual)
    (set! %tests-failed (+ %tests-failed 1))
    (display "FAIL: ")
    (display message)
    (display ": expected ")
    (write expected)
    (display ", got ")
    (write actual)
    (newline)))

(define (test-true value message) (test-equal #t value message))
(define (test-false value message) (test-equal #f value message))
(define (test-nil value message) (test-equal '() value message))

;; s1's harness has these too: a pass needs a value of the type.
(define (test-number value message) (test-equal #t (number? value) message))
(define (test-symbol value message) (test-equal #t (symbol? value) message))

(define (test-summary)
  (display %tests-run)
  (display " tests, ")
  (display %tests-failed)
  (display " failed")
  (newline))
