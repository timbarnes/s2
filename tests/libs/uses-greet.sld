;; Fixture for tests/scheme/library_tests.scm: a library file that imports another
;; library file, which is then loaded too.
(define-library (tests libs uses-greet)
  (export greet-twice)
  (import (scheme base) (tests libs greet))
  (begin (define (greet-twice name) (list (greet name) (greet name)))))
