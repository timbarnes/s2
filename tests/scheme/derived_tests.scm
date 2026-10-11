;; Ported from s1's scheme/derived_tests.scm, with s2's wording of arity
;; errors, which name the procedure and the expected count.

;; derived_tests.scm: R7RS derived expression types (4.2), case-lambda,
;; parameters and promises, plus the evaluator's tail-position handling.
;; Also re-run under gc-threshold 1 by gc_stress_tests.scm.

(display "          === Testing derived expression types ===")
(newline)

;; gc_stress_tests.scm defines **derived-loop-n** before reloading this file,
;; since a collection per allocation makes long loops far too slow.
(define derived-loop-n (guard (e (#t 10000)) **derived-loop-n**))

;; --- when / unless
(test-equal 2 (when #t 1 2) "when true")
(test-equal 4 (unless #f 3 4) "unless false")
(test-equal 'ok (let ((if #f) (not #f)) (unless (= 1 2) 'ok)) "unless is hygienic")

;; --- case
(test-equal 'composite (case (* 2 3) ((2 3 5 7) 'prime) ((1 4 6 8 9) 'composite)) "case")
(test-equal 'c (case (car '(c d)) ((a e i o u) 'vowel) ((w y) 'semivowel) (else => (lambda (x) x)))
    "case else =>")
(test-equal '((other . z) (semivowel . y) (vowel . u))
    (map (lambda (x)
           (case x
             ((a e i o u) => (lambda (w) (cons 'vowel w)))
             ((w y) (cons 'semivowel x))
             (else => (lambda (w) (cons 'other w)))))
         '(z y u))
    "case with => clauses")
(test-equal 'none (case 99 ((1) 'one) (else 'none)) "case else")
(test-equal 'two (let ((n 0)) (case (begin (set! n (+ n 1)) n) ((1) 'two) (else 'no))) "case evaluates its key once")

;; --- letrec*
(test-equal 5
    (letrec* ((p (lambda (x) (+ 1 (q (- x 1)))))
              (q (lambda (y) (if (zero? y) 0 (+ 1 (p (- y 1))))))
              (x (p 5))
              (y x))
      y)
    "letrec* sees earlier bindings")

;; --- let-values family
(test-equal 35 (let*-values (((root rem) (exact-integer-sqrt 32))) (* root rem)) "let*-values")
(test-equal '(x y x y)
    (let ((a 'a) (b 'b) (x 'x) (y 'y))
      (let*-values (((a b) (values x y)) ((x y) (values a b))) (list a b x y)))
    "let*-values binds in sequence")
(test-equal '(x y a b)
    (let ((a 'a) (b 'b) (x 'x) (y 'y))
      (let-values (((a b) (values x y)) ((x y) (values a b))) (list a b x y)))
    "let-values evaluates all inits first")
(test-equal '(1 (2 3) (4 5)) (let-values (((a . rest) (values 1 2 3)) (all (values 4 5))) (list a rest all))
    "let-values with dotted and variadic formals")
(test-equal 'ok (let-values () 'ok) "empty let-values")
(test-equal 1 (let ((x 1)) (let*-values () (define x 2) #f) x) "let*-values body is a new scope")

;; --- define-values
(test-equal 10 (let () (define-values (x y . z) (values 1 2 3 4)) (+ x y (car z) (cadr z))) "define-values dotted")
(test-equal 3 (let () (define-values x (values 1 2)) (apply + x)) "define-values variadic")
(test-equal 'ok (let () (define-values () (values)) 'ok) "define-values with no variables")
(test-equal 6 (let () (define-values (a b) (values 1 2)) (define c 3) (+ a b c))
    "a definition may follow define-values")
(define-values (dv-q dv-r) (floor/ 17 5))
(test-equal '(3 2) (list dv-q dv-r) "top-level define-values")

;; --- parameters
(define radix
  (make-parameter 10 (lambda (x) (if (and (exact-integer? x) (<= 2 x 16)) x (error "invalid radix")))))
(define (show-in-radix n) (number->string n (radix)))
(test-equal '("12" "1100" "12")
    (list (show-in-radix 12) (parameterize ((radix 2)) (show-in-radix 12)) (show-in-radix 12))
    "parameterize binds for its extent")
(test-equal 'rejected (guard (e (#t 'rejected)) (parameterize ((radix 0)) (show-in-radix 12)))
    "the converter checks parameterized values")
(test-equal 10 (radix) "an error inside parameterize restores the value")
(define p1 (make-parameter 1))
(define p2 (make-parameter 2))
(test-equal '(10 20) (parameterize ((p1 10) (p2 20)) (list (p1) (p2))) "several parameters")
(test-equal 1
    (begin (call/cc (lambda (k) (parameterize ((p1 99)) (k 'out)))) (p1))
    "escaping with a continuation restores the value")

;; --- promises
(test-equal 3 (force (delay (+ 1 2))) "force delay")
(test-equal '(3 3) (let ((p (delay (+ 1 2)))) (list (force p) (force p))) "a promise is forced once")
(define stream-integers
  (letrec ((next (lambda (n) (delay (cons n (next (+ n 1))))))) (next 0)))
(define (stream-filter p? s)
  (delay-force
   (if (null? (force s))
       (delay '())
       (let ((h (car (force s))) (t (cdr (force s))))
         (if (p? h) (delay (cons h (stream-filter p? t))) (stream-filter p? t))))))
(test-equal 5 (car (force (cdr (force (cdr (force (stream-filter odd? stream-integers)))))))
    "delay-force streams")
(define (loop-promise n) (delay-force (if (= n 0) (delay 'done) (loop-promise (- n 1)))))
(test-equal 'done (force (loop-promise derived-loop-n)) "a long delay-force chain")
(define promise-count 0)
(define reentrant
  (delay (begin (set! promise-count (+ promise-count 1))
                (if (> promise-count promise-x) promise-count (force reentrant)))))
(define promise-x 5)
(test-equal '(6 6) (list (force reentrant) (begin (set! promise-x 10) (force reentrant)))
    "a promise forced re-entrantly keeps its first value")
(test-equal '(#t #t #f) (list (promise? (delay 1)) (promise? (make-promise 1)) (promise? 5)) "promise?")
(test-equal 4 (force (make-promise (make-promise 4))) "make-promise of a promise")
(test-equal 5 (force 5) "force of a non-promise")

;; --- case-lambda
(define range
  (case-lambda
    ((e) (range 0 e))
    ((b e) (do ((r '() (cons e r)) (e (- e 1) (- e 1))) ((< e b) r)))))
(test-equal '((0 1 2) (3 4)) (list (range 3) (range 3 5)) "case-lambda by argument count")
(define any-arity
  (case-lambda (() 'zero) ((x) x) ((x y) (cons x y)) ((x y z) (list x y z)) (args (cons 'many args))))
(test-equal '(zero 1 (1 . 2) (1 2 3) (many 1 2 3 4))
    (map (lambda (args) (apply any-arity args)) '(() (1) (1 2) (1 2 3) (1 2 3 4)))
    "case-lambda through apply, with a variadic clause")
(define rest-arity
  (case-lambda (() '(zero)) ((x) (list 'one x)) ((x y) (list 'two x y)) ((x y . z) (list 'more x y z))))
(test-equal '((zero) (one 1) (two 1 2) (more 1 2 (3)))
    (list (rest-arity) (rest-arity 1) (rest-arity 1 2) (rest-arity 1 2 3))
    "case-lambda with a dotted clause")
(test-equal "case-lambda: no clause accepts 2 arguments"
    (guard (e (#t (error-object-message e))) ((case-lambda ((x) x)) 1 2))
    "no matching clause is an error")
(test-equal #t (procedure? any-arity) "a case-lambda is a procedure")

;; --- arity
(test-equal "#<procedure>: wrong number of arguments: 2 (expected 1)" (guard (e (#t (error-object-message e))) ((lambda (x) x) 1 2))
    "extra arguments to a fixed-arity procedure are an error")
(test-equal "#<procedure>: wrong number of arguments: 1 (expected 2)" (guard (e (#t (error-object-message e))) ((lambda (x y) x) 1))
    "missing arguments are an error")

;; --- tail position: a call under a non-tail if/begin/cond/and must return
;; to its caller's environment
(define (identity x) x)
(define (after-if a) (if #t (identity 0)) a)
(test-equal 'ok (after-if 'ok) "code after a non-tail if sees the caller's variables")
(define (in-if-test a) (if (if #t (identity #t) #f) a 'wrong))
(test-equal 'ok (in-if-test 'ok) "an if used as an if's test")
(define (in-and a) (and (if #t (identity #t) #f) a))
(test-equal 'ok (in-and 'ok) "an if inside and")
(define (in-cond a) (cond ((if #t (identity #f) #t) 'wrong) (else a)))
(test-equal 'ok (in-cond 'ok) "an if as a cond test")
(define (after-begin a) (begin (identity 0)) a)
(test-equal 'ok (after-begin 'ok) "code after a nested begin")
(define (after-when a) (when #t (identity 0)) a)
(test-equal 'ok (after-when 'ok) "code after a non-tail when")
(define (loop-in-let n) (let ((m (- n 1))) (if (= m 0) 'done (loop-in-let m))))
(test-equal 'done (loop-in-let derived-loop-n) "a tail call inside a let body")
(define (loop-in-cond n) (cond ((= n 0) 'done) (else (loop-in-cond (- n 1)))))
(test-equal 'done (loop-in-cond derived-loop-n) "a tail call in a cond clause")
