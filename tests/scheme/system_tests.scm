;; Ported from s1's scheme/system_tests.scm, with s2's wording of a type
;; error.

;; system_tests.scm: R7RS 6.14, (scheme process-context) and (scheme time).
;; exit, emergency-exit and command-line end or depend on the process, so
;; they are tested from Rust instead: tests/process.rs.

(display "          === Testing environment variables ===")
(newline)

(test-true (string? (get-environment-variable "PATH")) "get-environment-variable: PATH is set")
(test-equal #f (get-environment-variable "S1_SURELY_NOT_SET_9F3A") "get-environment-variable: an unset variable is #f")
(test-equal #f (get-environment-variable "") "get-environment-variable: the empty name is never set")
(test-equal #f (get-environment-variable "A=B") "get-environment-variable: a name with = is never set")
(test-equal "get-environment-variable: expected a string, got PATH"
    (guard (e (#t (error-object-message e))) (get-environment-variable 'PATH))
    "get-environment-variable rejects a non-string")

(define sys-env-vars (get-environment-variables))
(test-true (and (list? sys-env-vars)
                (let loop ((l sys-env-vars))
                  (or (null? l)
                      (and (pair? (car l)) (string? (caar l)) (string? (cdar l))
                           (loop (cdr l))))))
    "get-environment-variables: a list of string pairs")
(test-equal (get-environment-variable "PATH") (cdr (assoc "PATH" sys-env-vars))
    "get-environment-variables agrees with get-environment-variable")

(display "          === Testing time ===")
(newline)

(test-true (and (real? (current-second)) (inexact? (current-second))) "current-second is an inexact real")
(test-true (> (current-second) 1.7e9) "current-second counts from the Unix epoch")
(test-equal 1000000000 (jiffies-per-second) "jiffies are nanoseconds")
(test-true (and (exact-integer? (current-jiffy)) (>= (current-jiffy) 0)) "current-jiffy is a non-negative exact integer")
(define sys-j0 (current-jiffy))
(define (sys-spin n) (if (> n 0) (sys-spin (- n 1))))
(sys-spin 1000)
(define sys-j1 (current-jiffy))
(test-true (> sys-j1 sys-j0) "current-jiffy advances")
(test-true (< (/ (- sys-j1 sys-j0) (jiffies-per-second)) 10) "an elapsed time in seconds, from jiffies")
(test-equal 1024 (eval '(begin (current-jiffy) (expt 2 10)) (environment '(scheme base) '(scheme time)))
    "(scheme time) exports current-jiffy")
(test-true (string? (eval '(get-environment-variable "PATH") (environment '(scheme process-context))))
    "(scheme process-context) exports get-environment-variable")

(display "          === Testing command-line ===")
(newline)

(test-true (let ((cl (command-line)))
             (and (pair? cl) (let loop ((l cl)) (or (null? l) (and (string? (car l)) (loop (cdr l)))))))
    "command-line is a non-empty list of strings")
