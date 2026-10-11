;; shim.scm: a minimal stand-in for chibi's (chibi test) library, so that the
;; vendored R7RS conformance suite (r7rs-tests.scm) can run on s2. Forked
;; from s1's shim, rewritten with syntax-rules.
;;
;; ponytail: P6's temporary shim. s2 has no guard until P7, so a test whose
;; expression raises aborts its top-level form (s2 prints the error and
;; goes on to the next form). The test stays "pending" and is counted as an
;; error when the next test starts. test-error tests therefore count as
;; errors until P7, when this shim gets s1's guard-based version.
;;
;; Counting: every test bumps %r7rs-attempted before its expression is
;; evaluated. Tests in a form that failed before reaching them are never
;; attempted at all; run.sh reports those as "not reached".

(define %r7rs-pass 0)
(define %r7rs-fail 0)
(define %r7rs-error 0)
(define %r7rs-attempted 0)
(define %r7rs-pending #f)           ; quoted expr of the test now running
(define %r7rs-sections '())         ; stack of (name pass fail error)
(define %r7rs-epsilon 1e-5)

;; Close out a test that started but never reported back.
(define (%r7rs-settle-pending)
  (if %r7rs-pending
      (begin
        (set! %r7rs-error (+ %r7rs-error 1))
        (display "ERROR: ")
        (write %r7rs-pending)
        (newline)
        (set! %r7rs-pending #f))))

(define (%r7rs-section-name)
  (if (null? %r7rs-sections) "" (car (car %r7rs-sections))))

;; equal?, but inexact numbers only need to agree to a relative epsilon, as in
;; (chibi test).
(define (%r7rs-equal? expected actual)
  (or (equal? expected actual)
      (and (number? expected) (number? actual)
           (or (inexact? expected) (inexact? actual))
           (< (abs (- expected actual))
              (* %r7rs-epsilon (max 1 (abs expected)))))))

(define (%r7rs-start expr)
  (%r7rs-settle-pending)
  (set! %r7rs-attempted (+ %r7rs-attempted 1))
  (set! %r7rs-pending expr))

(define (%r7rs-report ok name expr expected actual)
  (set! %r7rs-pending #f)
  (if ok
      (set! %r7rs-pass (+ %r7rs-pass 1))
      (begin
        (set! %r7rs-fail (+ %r7rs-fail 1))
        (display "FAIL: [")
        (display (%r7rs-section-name))
        (display "] ")
        (if name (begin (write name) (display " ")))
        (write expr)
        (display " expected ")
        (write expected)
        (display " but got ")
        (write actual)
        (newline))))

(define (%r7rs-check name expr expected thunk)
  (%r7rs-start expr)
  (let ((actual (thunk)))
    (%r7rs-report (%r7rs-equal? expected actual) name expr expected actual)))

;; (test [name] expected expr)
(define-syntax test
  (syntax-rules ()
    ((_ name expected expr) (%r7rs-check name 'expr expected (lambda () expr)))
    ((_ expected expr) (%r7rs-check #f 'expr expected (lambda () expr)))))

;; (test-assert [name] expr): passes when expr is true.
(define-syntax test-assert
  (syntax-rules ()
    ((_ name expr) (%r7rs-check name 'expr #t (lambda () (if expr #t #f))))
    ((_ expr) (%r7rs-check #f 'expr #t (lambda () (if expr #t #f))))))

;; (test-values expected-expr expr): compares all returned values.
(define-syntax test-values
  (syntax-rules ()
    ((_ expected expr)
     (%r7rs-check #f 'expr
                  (call-with-values (lambda () expected) list)
                  (lambda () (call-with-values (lambda () expr) list))))))

;; (test-error expr): should pass when expr raises; see above.
(define-syntax test-error
  (syntax-rules ()
    ((_ expr)
     (begin
       (%r7rs-start 'expr)
       (let ((actual expr))
         (%r7rs-report #f #f 'expr 'an-error actual))))))

(define (test-begin . name)
  (%r7rs-settle-pending)
  (set! %r7rs-sections
        (cons (list (if (null? name) "" (car name))
                    %r7rs-pass %r7rs-fail %r7rs-error)
              %r7rs-sections))
  (display "== ")
  (display (%r7rs-section-name))
  (newline))

;; Prints "SECTION pass=P fail=F error=E name" for the section just closed.
(define (test-end . name)
  (%r7rs-settle-pending)
  (if (pair? %r7rs-sections)
      (let ((s (car %r7rs-sections)))
        (display "SECTION pass=")
        (display (- %r7rs-pass (cadr s)))
        (display " fail=")
        (display (- %r7rs-fail (caddr s)))
        (display " error=")
        (display (- %r7rs-error (cadddr s)))
        (display " ")
        (display (car s))
        (newline)
        (set! %r7rs-sections (cdr %r7rs-sections)))))

;; Called by run.sh after the suite: the line run.sh parses for totals.
(define (%r7rs-summary)
  (%r7rs-settle-pending)
  (display "SUMMARY attempted=")
  (display %r7rs-attempted)
  (display " pass=")
  (display %r7rs-pass)
  (display " fail=")
  (display %r7rs-fail)
  (display " error=")
  (display %r7rs-error)
  (newline))
