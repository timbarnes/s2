;; Fixture for tests/scheme/library_tests.scm: a library file that defines a library
;; other than the one its name implies, and counts how often it is loaded.
(define-library (tests libs not-wrong-name) (export) (import (scheme base)))
(set! sld-load-count (+ sld-load-count 1))
