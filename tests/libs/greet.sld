;; Fixture for tests/scheme/library_tests.scm: a library found on the search path,
;; whose body comes from a file included relative to this one.
(define-library (tests libs greet)
  (export greet)
  (import (scheme base))
  (include "greet-body.scm"))
