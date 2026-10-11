;; Ported from s1's scheme/tail_tests.scm: s1's %kont-depth is s2's
;; %frame-depth, the number of frames waiting for a return.

;; tail_tests.scm: proper tail calls in every R7RS 3.5 tail context.
;;
;; Each loop returns (%frame-depth), the number of continuation frames
;; waiting at its base case. A loop whose self-call is a proper tail call
;; reaches the base case at the same depth after 5 iterations as after 500.

(display "          === Testing tail calls in every tail context ===")
(newline)

(define-syntax tail-id (syntax-rules () ((_ e) e)))

(define-syntax tail-test
  (syntax-rules ()
    ((_ name (loop n) step)
     (let ()
       (define (loop n) (if (= n 0) (%frame-depth) step))
       (test-equal (loop 5) (loop 500) name)))))

(tail-test "if" (loop n) (if #t (loop (- n 1)) 0))
(tail-test "cond" (loop n) (cond (#f 0) (else (loop (- n 1)))))
(tail-test "cond =>" (loop n) (cond ((- n 1) => loop)))
(tail-test "case" (loop n) (case 1 ((1) (loop (- n 1)))))
(tail-test "case else" (loop n) (case 1 ((2) 0) (else (loop (- n 1)))))
(tail-test "case =>" (loop n) (case 1 ((1) => (lambda (x) (loop (- n 1))))))
(tail-test "and, two operands" (loop n) (and #t (loop (- n 1))))
(tail-test "and, last of several" (loop n) (and #t 1 (loop (- n 1))))
(tail-test "or, two operands" (loop n) (or #f (loop (- n 1))))
(tail-test "or, last of several" (loop n) (or #f #f (loop (- n 1))))
(tail-test "when" (loop n) (when #t (loop (- n 1))))
(tail-test "unless" (loop n) (unless #f (loop (- n 1))))
(tail-test "let" (loop n) (let ((m (- n 1))) (loop m)))
(tail-test "let*" (loop n) (let* ((m (- n 1))) (loop m)))
(tail-test "letrec" (loop n) (letrec ((m (- n 1))) (loop m)))
(tail-test "letrec*" (loop n) (letrec* ((m (- n 1))) (loop m)))
(tail-test "named let" (loop n) (let l ((m (- n 1))) (loop m)))
(tail-test "let-values" (loop n) (let-values (((m) (- n 1))) (loop m)))
(tail-test "let*-values" (loop n) (let*-values (((m) (- n 1))) (loop m)))
(tail-test "let-syntax" (loop n) (let-syntax () (loop (- n 1))))
(tail-test "letrec-syntax" (loop n) (letrec-syntax () (loop (- n 1))))
(tail-test "begin" (loop n) (begin 1 (loop (- n 1))))
(tail-test "do result" (loop n) (do ((i 0 (+ i 1))) ((= i 1) (loop (- n 1)))))
(tail-test "lambda body" (loop n) ((lambda () (loop (- n 1)))))
(tail-test "internal definitions" (loop n) ((lambda () (define m (- n 1)) (loop m))))
(tail-test "case-lambda" (loop n) ((case-lambda ((x) (loop x))) (- n 1)))
(tail-test "macro use" (loop n) (tail-id (loop (- n 1))))
(tail-test "apply" (loop n) (apply loop (list (- n 1))))
(tail-test "apply, case-lambda" (loop n) (apply (case-lambda ((x) (loop x))) (list (- n 1))))
(tail-test "call/cc" (loop n) (call/cc (lambda (k) (loop (- n 1)))))
(tail-test "call-with-values consumer" (loop n) (call-with-values (lambda () (- n 1)) loop))

;; The check itself: a call that isn't in tail position does grow.
(define (tail-not n) (if (= n 0) (%frame-depth) (+ 0 (tail-not (- n 1)))))
(test-true (< (tail-not 5) (tail-not 500)) "a non-tail call grows the continuation")
(define (tail-or-not n) (if (= n 0) (%frame-depth) (or (tail-or-not (- n 1)) #f)))
(test-true (< (tail-or-not 5) (tail-or-not 500)) "an operand of or other than the last isn't a tail call")
