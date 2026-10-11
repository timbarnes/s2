;; exception_tests.scm: with-exception-handler, raise, error objects and
;; guard (R7RS 6.11, 4.2.7). Ported from s1's scheme/exception_tests.scm,
;; with s2's error messages; guards nest 8,000 deep (the roadmap's figure).
;; The test of an uncaught error is a unit test in src/main.rs, since this
;; runner treats any uncaught error as a failure.

(display "          === Testing exceptions ===")
(newline)

;; --- raise-continuable and with-exception-handler
(test-equal 65
    (with-exception-handler (lambda (c) 42) (lambda () (+ (raise-continuable 'oops) 23)))
    "raise-continuable returns the handler's value")
(test-equal '(caught oops)
    (call/cc (lambda (k)
      (with-exception-handler (lambda (c) (k (list 'caught c))) (lambda () (raise 'oops)))))
    "handler escapes with a continuation")
(test-equal 'outer
    (call/cc (lambda (k)
      (with-exception-handler
        (lambda (c) (k 'outer))
        (lambda ()
          (with-exception-handler
            (lambda (c) (raise 'from-inner-handler))
            (lambda () (raise 'x)))))))
    "an error inside a handler goes to the outer handler")
(test-equal 'not-called
    (call/cc (lambda (k)
      (with-exception-handler
        (lambda (c) (k 'not-called))
        (lambda ()
          (with-exception-handler (lambda (c) (k 'stale-handler)) (lambda () 'returned))
          (raise 'later)))))
    "a handler is uninstalled when its thunk returns")
(test-equal 'outer-only
    (call/cc (lambda (k)
      (with-exception-handler
        (lambda (c) (k 'outer-only))
        (lambda ()
          (call/cc (lambda (escape)
            (with-exception-handler (lambda (c) (k 'stale)) (lambda () (escape 'out)))))
          (raise 'after-escape)))))
    "escaping out of a handler's extent uninstalls it")

;; --- built-in failures are error objects
(test-equal "car: expected a pair, got 1"
    (guard (e ((error-object? e) (error-object-message e))) (car 1))
    "built-in error is an error object with a message")
(test-equal '() (guard (e (#t (error-object-irritants e))) (car 1)) "built-in error has no irritants")
(test-equal 'arity (guard (e (#t 'arity)) ((lambda (x) x))) "wrong argument count is catchable")
(test-equal 'non-procedure (guard (e (#t 'non-procedure)) (1 2)) "applying a non-procedure is catchable")
(test-equal 'unbound (guard (e (#t 'unbound)) undefined-variable-xyz) "unbound variable is catchable")

;; --- error
(test-equal "boom" (guard (e (#t (error-object-message e))) (error "boom" 1 2)) "error message")
(test-equal '(1 2) (guard (e (#t (error-object-irritants e))) (error "boom" 1 2)) "error irritants")
(test-equal '(#t #f #f) (guard (e (#t (list (error-object? e) (file-error? e) (read-error? e)))) (error "x"))
    "error makes a plain error object")
(test-equal #f (error-object? 'sym) "error-object? of a non-error")

;; --- file and read errors
(test-equal '(#t ("/no/such/file"))
    (guard (e ((file-error? e) (list #t (error-object-irritants e)))) (open-input-file "/no/such/file"))
    "open-input-file failure is a file error naming the file")
(define read-error-file "target/s2-read-error-test.tmp")
(let ((out (open-output-file read-error-file)))
  (display ")" out)
  (close-output-port out))
(test-equal #t
    (guard (e ((read-error? e) #t)) (read (open-input-file read-error-file)))
    "read of malformed input is a read error")

;; --- non-continuable raise
(test-equal "exception handler returned from a non-continuable raise of"
    (guard (e ((error-object? e) (error-object-message e)))
      (with-exception-handler (lambda (c) 'returned) (lambda () (raise 'x))))
    "returning from a raise handler is a secondary error")

;; --- guard
(test-equal '(caught boom) (guard (e (#t (list 'caught e))) (raise 'boom)) "guard catches")
(test-equal 'str (guard (e ((symbol? e) 'sym) ((string? e) 'str)) (raise "x")) "guard picks the matching clause")
(test-equal 42 (guard (e ((assq 'a e) => cdr) ((assq 'b e))) (raise (list (cons 'a 42)))) "guard => clause")
(test-equal '(b . 23) (guard (e ((assq 'a e) => cdr) ((assq 'b e))) (raise (list (cons 'b 23)))) "guard test-only clause")
(test-equal 'else (guard (e ((string? e) 'str) (else 'else)) (raise 1)) "guard else clause")
(test-equal 'outer (guard (e (#t 'outer)) (guard (e ((string? e) 'inner)) (raise 'not-a-string)))
    "unmatched guard re-raises to the outer guard")
(test-equal 11
    (with-exception-handler (lambda (c) 10)
      (lambda () (+ 1 (guard (e ((string? e) 'no)) (raise-continuable 5)))))
    "unmatched guard re-raises continuably in the original context")
(test-equal '(1 2) (call-with-values (lambda () (guard (e (#t 0)) (values 1 2))) list) "guard body returns multiple values")
(test-equal 7 (guard (e (#t 'no)) (+ 3 4)) "guard with no raise returns the body's value")
(test-equal '(mine mine2 r)
    (let ((condition 'mine) (args 'mine2)) (guard (e (#t (list condition args e))) (raise 'r)))
    "guard's temporaries don't capture user variables")
(define guard-log '())
(test-equal '(g (before after handled))
    (let ((v (guard (e (#t (set! guard-log (cons 'handled guard-log)) 'g))
               (dynamic-wind (lambda () (set! guard-log (cons 'before guard-log)))
                             (lambda () (raise 'x))
                             (lambda () (set! guard-log (cons 'after guard-log)))))))
      (list v (reverse guard-log)))
    "guard unwinds dynamic-wind before running its clauses")
(define (reraise-test v)
  (call/cc (lambda (k)
    (with-exception-handler
      (lambda (x) (k (list 'reraised x)))
      (lambda ()
        (guard (condition ((positive? condition) 'positive) ((negative? condition) 'negative))
          (raise v)))))))
(test-equal '(positive negative (reraised 0)) (map reraise-test '(1 -1 0)) "SRFI-34 re-raise examples")

;; --- cond => passes the test value, not a re-evaluation of it
(test-equal 2 (cond ((assq 'b '((a 1) (b 2))) => cadr) (else 'no)) "cond => with a list value")

(display "          === Testing nested guards and escape-only continuations ===")
(newline)

;; Nested guards used to be cubic: every guard captured a full continuation
;; (copying the argument stack), and every collection re-marked each one's
;; whole frame chain (in s1).
(define guard-depth-n (test-size 8000))
(define (nested-guards n)
  (if (= n 0) 0 (+ 1 (guard (e (#t 'caught)) (nested-guards (- n 1))))))
(test-equal guard-depth-n (nested-guards guard-depth-n) "deeply nested guards return normally")
(define (nested-reraise n)
  (if (= n 0) (raise 'deep) (guard (e ((eq? e 'never) 0)) (nested-reraise (- n 1)))))
(test-equal '(outer deep) (guard (e (#t (list 'outer e))) (nested-reraise guard-depth-n))
    "a raise passes through deeply nested guards whose clauses don't match")

;; guard jumps out with an escape-only continuation (%call/ec). One still
;; works when a full continuation re-enters the guard's body after the
;; guard has returned: the guard's frames are part of the re-entered chain.
(define guard-re-k #f)
(define guard-re-count 0)
(define guard-re-result
  (guard (e (#t 'caught))
    (+ 1 (call/cc (lambda (k) (set! guard-re-k k) 1)))))
(if (< guard-re-count 1)
    (begin (set! guard-re-count (+ guard-re-count 1)) (guard-re-k 10)))
(test-equal 11 guard-re-result "guard returns normally after its body is re-entered")
(define guard-re-raise
  (guard (e (#t (list 'caught e)))
    ((call/cc (lambda (k) (set! guard-re-k k) (lambda () 1))))))
(if (equal? guard-re-raise 1) (guard-re-k (lambda () (raise 'later))))
(test-equal '(caught later) guard-re-raise "guard catches a raise after its body is re-entered")

(define saved-ec #f)
(define (save-ec) (list (%call/ec (lambda (k) (set! saved-ec k) 1))))
(test-equal '(1) (save-ec) "%call/ec returns normally")
(test-equal '(2) (list (%call/ec (lambda (k) (+ 1 (k 2))))) "%call/ec escapes")
(test-equal "escape continuation invoked after its extent has ended"
    (guard (e (#t (error-object-message e))) (saved-ec 3))
    "an escape-only continuation can't be used once its call has returned")
