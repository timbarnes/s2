;; control_tests.scm: dynamic-wind with escapes and re-entry. From s1's
;; scheme/advanced_tests.scm (the dynamic-wind sections), unchanged except
;; for leaving out a test that uses s1's displayln.

;; Escapes must keep the frames pending around the call/cc and drop the
;; arguments already evaluated by the frames they abandon.
(test-equal 6 (+ 1 (call/cc (lambda (k) (k 5)))) "call/cc escape from argument position keeps pending +")
(define (escape-mid-args) (call/cc (lambda (k) (+ 10 (k 2)))))
(test-equal '(1 2 3) (list 1 (escape-mid-args) 3) "call/cc escape discards abandoned arguments")

;; before runs outside the extent: re-entering a continuation captured in
;; before resumes it without winding in (no second before).
(test-equal '(b t a t a)
  (let ((k #f) (n 0) (trace '()))
    (dynamic-wind
      (lambda () (set! trace (cons 'b trace)) (call/cc (lambda (c) (set! k c))))
      (lambda () (set! trace (cons 't trace)))
      (lambda () (set! trace (cons 'a trace))))
    (set! n (+ n 1))
    (if (< n 2) (k #f))
    (reverse trace))
  "dynamic-wind: re-entry captured in before does not re-run before")

(define x '())
(define result
  (call/cc
   (lambda (k)
     (dynamic-wind
      (lambda () (set! x (cons 'before x)))
      (lambda () (k 'escaped))
      (lambda () (set! x (cons 'after x)))))))
(test-equal 'escaped result "dynamic-wind: non-local exit returns correct value")
(test-equal '(after before) x "dynamic-wind: non-local exit runs 'after' thunk")

(define nested-dw-log '())
(define (dw-log . items)
  (set! nested-dw-log (append nested-dw-log items)))

(define nested-dw-result
  (call/cc
   (lambda (exit)
     (dynamic-wind
       (lambda () (dw-log 'outer-before))
       (lambda ()
         (dynamic-wind
           (lambda () (dw-log 'inner-before))
           (lambda () (exit 'escaped))
           (lambda () (dw-log 'inner-after))))
       (lambda () (dw-log 'outer-after))))))

(test-equal 'escaped nested-dw-result "nested dynamic-wind: correct return value")
(test-equal '(outer-before inner-before inner-after outer-after) nested-dw-log "nested dynamic-wind: correct thunk order")

(define re-entrant-log '())
(define (log-re . items)
  (set! re-entrant-log (append re-entrant-log items)))

(define k #f)
(define re-entrant-result
  (dynamic-wind
    (lambda () (log-re 'before-1))
    (lambda ()
      (call/cc
       (lambda (exit)
         (set! k exit)
         'initial-run)))
    (lambda () (log-re 'after-1))))

(test-equal 'initial-run re-entrant-result "re-entrant: initial result")
(test-equal '(before-1 after-1) re-entrant-log "re-entrant: initial run order")

;; Re-entry: (k 're-entry-value) goes back into the earlier top-level form,
;; which finishes by defining re-entrant-result; then the next form runs.
(set! re-entrant-log '())
(set! re-entrant-result #f)
(call/cc
 (lambda (top-exit)
   (dynamic-wind
     (lambda () (log-re 'before-2))
     (lambda () (top-exit (k 're-entry-value)))
     (lambda () (log-re 'after-2)))))
(test-equal 're-entry-value re-entrant-result "re-entrant: re-entry result")
(test-equal '(before-2 after-2 before-1 after-1) re-entrant-log "re-entrant: re-entry run order")
