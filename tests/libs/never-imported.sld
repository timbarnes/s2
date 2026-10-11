;; Fixture for tests/scheme/library_tests.scm: a library file that exists, for
;; cond-expand's (library ...) requirement, but is never imported.
(define-library (tests libs never-imported) (export) (import (scheme base)))
