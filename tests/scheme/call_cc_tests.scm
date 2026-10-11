;;; call_cc_tests.scm: re-entrant and multi-shot continuations, R7RS-small
;;; core only. Copied from s1 (scheme/call_cc_tests.scm), unchanged apart
;;; from this header and test-size in the two long tests.

(define (check name expected thunk)
  (test-equal expected (thunk) name))

;; 1. Same continuation invoked repeatedly after its extent has exited.
(check "multi-shot re-entry"
  '(3 2 1 0)
  (lambda ()
    (let ((k #f) (log '()))
      (let ((v (call/cc (lambda (c) (set! k c) 0))))
        (set! log (cons v log))
        (if (< v 3) (k (+ v 1)))
        log))))

;; 2. Re-entry in the middle of argument evaluation; each pass must
;;    produce a fresh list with the already-evaluated args intact.
(check "re-entry mid-argument-evaluation"
  '((1 12 3) (1 11 3) (1 2 3))
  (lambda ()
    (let ((k #f) (results '()))
      (let ((v (list 1 (call/cc (lambda (c) (set! k c) 2)) 3)))
        (set! results (cons v results))
        (if (< (length results) 3) (k (+ 10 (length results))))
        results))))

;; 3. Catches frames that accumulate args in shared mutable storage
;;    (a common CEK bug): mutating one pass's result must not leak into
;;    the next pass.
(check "no aliasing between re-entries"
  '((a b) (a b))
  (lambda ()
    (let ((k #f) (seen '()))
      (let ((v (list 'a (call/cc (lambda (c) (set! k c) 0)) 'b)))
        (set! seen (cons (list (car v) (car (cddr v))) seen))
        (set-car! v 'mutated)
        (set-car! (cddr v) 'mutated)
        (if (< (length seen) 2) (k 1))
        seen))))

;; 4. Capture under 1000 pending non-tail frames, re-enter several times.
(define (deep n box)
  (if (= n 0)
      (call/cc (lambda (c) (set-car! box c) 0))
      (+ 1 (deep (- n 1) box))))

(check "re-entry through deep non-tail recursion"
  (list (test-size 1000) (+ (test-size 1000) 1) (+ (test-size 1000) 2))
  (lambda ()
    (let ((box (list #f)) (results '()))
      (let ((v (deep (test-size 1000) box)))
        (set! results (cons v results))
        (if (< (length results) 3) ((car box) (length results)))
        (reverse results)))))

;; 5. 100k re-entries must not grow the stack/heap without bound.
(check "re-entry loop in bounded space"
  (test-size 100000)
  (lambda ()
    (let ((k #f) (n 0))
      (call/cc (lambda (c) (set! k c)))
      (set! n (+ n 1))
      (if (< n (test-size 100000)) (k #f))
      n)))

;; 6. Two interleaved generators: resumes continuations captured inside
;;    another generator's exited extent.
(define (make-gen lst)
  (define return #f)
  (define resume #f)
  (lambda ()
    (call/cc
     (lambda (r)
       (set! return r)
       (if resume
           (resume #f)
           (begin
             (let loop ((l lst))
               (if (pair? l)
                   (begin
                     (call/cc (lambda (c) (set! resume c) (return (car l))))
                     (loop (cdr l)))))
             (return 'done)))))))

(check "interleaved generators"
  '(1 a 2 b 3 c done done)
  (lambda ()
    (let ((g1 (make-gen '(1 2 3))) (g2 (make-gen '(a b c))))
      (let* ((x1 (g1)) (y1 (g2)) (x2 (g1)) (y2 (g2))
             (x3 (g1)) (y3 (g2)) (x4 (g1)) (y4 (g2)))
        (list x1 y1 x2 y2 x3 y3 x4 y4)))))

;; 7. amb backtracking: each amb's return continuation is invoked once
;;    per choice -- heavy multi-shot use.
(define fail-stack '())

(define (amb-fail)
  (let ((back (car fail-stack)))
    (set! fail-stack (cdr fail-stack))
    (back #f)))

(define (amb choices)
  (call/cc
   (lambda (k)
     (let try ((cs choices))
       (if (null? cs)
           (amb-fail)
           (begin
             (call/cc (lambda (next)
                        (set! fail-stack (cons next fail-stack))
                        (k (car cs))))
             (try (cdr cs))))))))

(define (require p) (if (not p) (amb-fail)))

(define (range lo hi)
  (if (> lo hi) '() (cons lo (range (+ lo 1) hi))))

(check "amb: all pythagorean triples to 20"
  '((3 4 5) (5 12 13) (6 8 10) (8 15 17) (9 12 15) (12 16 20))
  (lambda ()
    (let ((sols '()) (r (range 1 20)))
      (call/cc
       (lambda (done)
         (set! fail-stack (list (lambda (_) (done #f))))
         (let* ((a (amb r)) (b (amb r)) (c (amb r)))
           (require (< a b))
           (require (= (+ (* a a) (* b b)) (* c c)))
           (set! sols (cons (list a b c) sols))
           (amb-fail))))
      (reverse sols))))

;; 8. Re-entry through a builtin higher-order procedure. Fails if map is
;;    a host-language loop that calls back into the evaluator.
;;    R7RS 6.10: earlier returns from map must not be mutated.
(check "re-entry through builtin map"
  '((1 2 3) (1 10 3) (1 20 3))
  (lambda ()
    (let ((k #f) (count 0) (results '()))
      (let ((r (map (lambda (x)
                      (call/cc (lambda (c) (if (= x 2) (set! k c)) x)))
                    '(1 2 3))))
        (set! results (cons r results))
        (set! count (+ count 1))
        (if (< count 3) (k (* 10 count)))
        (reverse results)))))

;; 9. dynamic-wind: re-entry must re-run the before thunk.
;;    Delete this test if dynamic-wind isn't implemented yet.
(check "dynamic-wind on re-entry"
  '(in out in out in out)
  (lambda ()
    (let ((trace '()) (k #f) (n 0))
      (dynamic-wind
       (lambda () (set! trace (cons 'in trace)))
       (lambda () (call/cc (lambda (c) (set! k c))))
       (lambda () (set! trace (cons 'out trace))))
      (set! n (+ n 1))
      (if (< n 3) (k #f))
      (reverse trace))))
