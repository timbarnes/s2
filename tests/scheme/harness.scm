;; harness.scm: test-equal for the ported s1 test files (tests/scheme.rs
;; runs each one after this). A failing test prints a FAIL line; an error
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

(define (test-summary)
  (display %tests-run)
  (display " tests, ")
  (display %tests-failed)
  (display " failed")
  (newline))
