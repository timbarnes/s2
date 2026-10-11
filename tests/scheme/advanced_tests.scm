;; Ported from s1's scheme/advanced_tests.scm, with these changes:
;; - displayln (an s1 extension) becomes display and newline;
;; - s2's wording of the unbound-variable error;
;; - left out: a define as the only form of a guard body (s1 defines a
;;   global; in R7RS a body needs an expression), and a macro redefined
;;   after code using it was compiled (s2 expands when it compiles, P5);
;; - the malformed let, which s2 reports when it compiles the form, is
;;   compiled by eval, so it still raises at each call.

(display "          === Testing let, let* and letrec ===")
(newline)

(test-equal 6 (let ((x 2) (y 3)) (* x y)) "Two binding let with single body expression")
(test-equal 9 (let ((x 3)) (* x x)) "Single binding let with single body expression")
(test-equal 42 (let ((x 6)) (let ((y (+ x 1))) (* x y))) "Nested let expression")
(test-equal 3 (let* ((x 1) (y (+ x 1))) (+ x y)) "let* with sequential dependency")
(test-equal 3  (let ((x 10)) (let* ((x 1) (y (+ x 1))) (+ x y))) "let* with parameter shadowing")
(test-equal 42 (let* () 42) "let* with no bindings")
(test-equal 25 (let* ((x 5)) (* x x)) "let* with single binding")
(test-equal 25 (let* ((x 2) (y 3)) (define z (+ x y)) (* z z)) "Multiple body expressions")
(test-equal 4 (let* ((x 1) (y (let* ((a x) (b (+ a 1))) (+ a b)))) (+ x y)) "Nested let* expressions")
(test-equal '(3 (2 4 6))
    (let* ((lst '(1 2 3)) (len (length lst)) (doubled (map (lambda (x) (* x 2)) lst))) (list len doubled))
    "let* using previous bindings in complex expression")
(test-equal 120
 (letrec ((fact (lambda (n)
                  (if (= n 0)
                      1
                      (* n (fact (- n 1)))))))
   (fact 5)) "letrec factorial")

(test-equal '(#t #f #f #t)
 (letrec ((even? (lambda (n)
                   (if (= n 0)
                       #t
                       (odd? (- n 1)))))
          (odd? (lambda (n)
                  (if (= n 0)
                      #f
                      (even? (- n 1))))))
   (list (even? 4) (odd? 4) (even? 3) (odd? 3)))
   "Mutually recursive functions")

 (test-equal 42 (letrec () 42) "letrec with no bindings")

 (test-equal 49 (letrec ((square (lambda (x) (* x x))))
   (square 7)) "letrec with single binding")
 (test-equal 10
    (letrec ((double (lambda (x) (* x 2))))
    (define result (double 5))
    result)
    "letrec with multiple body expressions")

 ;; Self-referencing non-function (should work but xyz should be undefined initially)
 (test-equal 42 (letrec ((xyz (if #f xyz 42))) xyz) "Self-referencing non-function")

 (test-equal 4 (letrec ((length (lambda (lst)
                    (if (null? lst)
                        0
                        (+ 1 (length (cdr lst)))))))
                (length '(a b c d)))
    "Recursive list processing")

(test-equal 8 (letrec ((outer (lambda (n)
                                (letrec ((inner (lambda (m) (+ n m))))
                                    (inner 5)))))
                (outer 3))
    "Nested letrec")

(test-equal 3 (let ((x 100))
                (letrec ((x 1)
                            (f (lambda () (+ x 2))))
                    (f)))
        "Variable shadowing with recursion")

(test-equal 55 (let loop ((x 10) (acc 0))
                    (if (= x 0)
                        acc
                        (loop (- x 1) (+ acc x))))
    "Named let")

(display "          === Testing do ===")
(newline)

;; Basic counting loop
(test-equal 55
    (do ((i 1 (+ i 1))
         (sum 0 (+ sum i)))
        ((> i 10) sum))
    "do: sum 1 to 10")

;; Simple countdown
(test-equal 0
    (do ((n 5 (- n 1)))
        ((= n 0) n))
    "do: countdown to zero")

;; Factorial calculation
(test-equal 120
    (do ((n 5 (- n 1))
         (fact 1 (* fact n)))
        ((= n 0) fact))
    "do: factorial of 5")

;; List length calculation
(test-equal 4
    (do ((lst '(a b c d) (cdr lst))
         (len 0 (+ len 1)))
        ((null? lst) len))
    "do: calculate list length")

;; List reversal
(test-equal '(d c b a)
    (do ((lst '(a b c d) (cdr lst))
         (rev '() (cons (car lst) rev)))
        ((null? lst) rev))
    "do: reverse list")

;; Empty bindings
(test-equal 42
    (do () (#t 42))
    "do: empty bindings")

;; Single variable, no step (variable unchanged)
(test-equal 10
    (do ((x 10))
        (#t x))
    "do: single variable no step")

;; Multiple result expressions
(test-equal 3
    (do ((i 0 (+ i 1)))
        ((= i 3) (+ i 1) (- i 1) i))
    "do: multiple result expressions returns last")

;; Commands executed during iteration
(test-equal 6
    (let ((result 0))
      (do ((i 1 (+ i 1)))
          ((> i 3) result)
        (set! result (+ result i))))
    "do: commands executed each iteration")

;; No result expressions (returns unspecified, test for not crashing)
(test-equal #t
    (let ((x #f))
      (do ((i 0 (+ i 1)))
          ((= i 1) (set! x #t)))
      x)
    "do: no result expressions")

;; Nested do loops
(test-equal 10
    (do ((i 1 (+ i 1))
         (total 0))
        ((> i 3) total)
      (do ((j 1 (+ j 1)))
          ((> j i))
        (set! total (+ total j))))
    "do: nested do loops")

;; Variable shadowing
(test-equal 10
    (let ((x 100))
      (do ((x 0 (+ x 1))
           (sum 0 (+ sum x)))
          ((= x 5) sum)))
    "do: variable shadowing")

;; Complex step expressions
(test-equal '(1 2 4 8 16)
    (do ((n 16 (quotient n 2))
         (powers '() (cons n powers)))
        ((= n 0) powers))
    "do: complex step expressions")

;; Zero iterations (test immediately true)
(test-equal 42
    (do ((i 0 (+ i 1)))
        (#t 42)
      (error "should not execute"))
    "do: zero iterations")

;; Multiple commands in body
(test-equal '(3 2 1)
    (let ((result '()))
      (do ((i 1 (+ i 1)))
          ((> i 3) result)
        (set! result (cons i result))
        (set! result (reverse result))
        (set! result (reverse result))))
    "do: multiple commands in body")

(display "          === Testing read ===")
(newline)
;; read takes its data from a string port: with no port it reads the
;; current input port, which is standard input (not the file being loaded).
(define read-source (open-input-string "(list a 2 3) 22 [1 2 3] \"a string\""))
(test-equal '(list a 2 3) (read read-source) "Reading a list")
(test-equal 22 (read read-source) "Reading an integer")
(test-equal [1 2 3] (read read-source) "Reading a vector")
(test-equal "a string" (read read-source) "Reading a string")
(test-equal #t (eof-object? (read read-source)) "Reading at end of input")

(display "          === Testing delay and force ===")
(newline)
(define **count** 0)
(define p
  (delay (begin (set! **count** (+ **count** 1))
                (if (> **count** x)
                    **count**
                    (force p)))))
(define x 5)
(test-equal 6 (force p) "First force")
(test-equal 6 (begin (set! **count** 10) (force p)) "Second force")

(display "          === Testing call/cc ===")
(newline)
(define cont1 (call/cc (lambda (k) (k 5))))
(test-equal 5  cont1 "Return a value")
(test-equal 10 (call/cc (lambda (k) (+ 1 (k 10) 3))) "Break out of add")
(define saved #f)
(define result
    (call/cc (lambda (k) (set! saved k) 'ok)))
(test-equal 'ok result "Capture and re-use continuation")
;; Re-entry must stay inside one expression: invoking a continuation captured
;; by a top-level define re-runs that define and abandons the caller.
(test-equal '(99 ok)
  (let ((k #f) (seen '()))
    (let ((r (call/cc (lambda (c) (set! k c) 'ok))))
      (set! seen (cons r seen))
      (if (= (length seen) 1) (k 99))
      seen))
  "Reuse previous continuation")

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

;; Additional call/cc regression tests
;; Basic call/cc with assignment (the original bug case)
(define cont2 (call/cc (lambda (k) (k 42))))
(test-equal 42 cont2 "call/cc assignment should work")

;; call/cc with nested computation
(define result1 (call/cc (lambda (k) (+ 10 (k 5) 20))))
(test-equal 5 result1 "call/cc should escape from nested computation")

;; call/cc without escape (normal return)
(define result2 (call/cc (lambda (k) (+ 1 2 3))))
(test-equal 6 result2 "call/cc should work without escaping")

;; Multiple call/cc in sequence
(define val1 (call/cc (lambda (k) (k 'first))))
(define val2 (call/cc (lambda (k) (k 'second))))
(test-equal 'first val1 "first call/cc should work")
(test-equal 'second val2 "second call/cc should work")

;; call/cc with conditional escape
(define test-escape
  (lambda (x)
    (call/cc (lambda (return)
               (if (< x 0)
                   (return 'negative)
                   (+ x 10))))))
(test-equal 15 (test-escape 5) "call/cc conditional no escape")
(test-equal 'negative (test-escape -3) "call/cc conditional escape")

;; call/cc capturing loop state
(define sum 0)
(define loop-result
  (call/cc (lambda (break)
             (define loop (lambda (i)
               (set! sum (+ sum i))
               (if (> i 5)
                   (break sum)
                   (loop (+ i 1)))))
             (loop 1))))
(test-equal 21 loop-result "call/cc should capture loop state")

;; Nested call/cc
(define nested-result
  (call/cc (lambda (outer)
             (+ 100
                (call/cc (lambda (inner)
                          (outer 42)))
                200))))
(test-equal 42 nested-result "nested call/cc should escape to outer")

;; call/cc with function application
(define add-or-escape
  (lambda (x y escape?)
    (call/cc (lambda (k)
               (if escape?
                   (k 'escaped)
                   (+ x y))))))
(test-equal 7 (add-or-escape 3 4 #f) "call/cc function normal case")
(test-equal 'escaped (add-or-escape 3 4 #t) "call/cc function escape case")

;; call/cc with list processing
(define find-negative
  (lambda (lst)
    (call/cc (lambda (found)
               (define loop (lambda (items)
                 (cond
                   ((null? items) 'none)
                   ((< (car items) 0) (found (car items)))
                   (else (loop (cdr items))))))
               (loop lst)))))
(test-equal -3 (find-negative '(1 2 -3 4)) "call/cc list processing escape")
(test-equal 'none (find-negative '(1 2 3 4)) "call/cc list processing normal")

;; call/cc with string operations
(define process-string
  (lambda (str)
    (call/cc (lambda (return)
               (if (string=? str "")
                   (return 'empty-string)
                   (string-length str))))))
(test-equal 5 (process-string "hello") "call/cc string processing normal")
(test-equal 'empty-string (process-string "") "call/cc string processing escape")

;; call/cc with error simulation
(define safe-divide
  (lambda (x y)
    (call/cc (lambda (error)
               (if (= y 0)
                   (error 'division-by-zero)
                   (/ x y))))))
(test-equal 5 (safe-divide 10 2) "call/cc error simulation normal")
(test-equal 'division-by-zero (safe-divide 10 0) "call/cc error simulation escape")

(display "          === Testing dyamic-wind ===")
(newline)
(test-equal "thunk"
    (dynamic-wind
        (lambda () (display "Before "))
        (lambda () (display "Thunk ") "thunk")
        (lambda () (display "After ") (newline)))
    "dynamic-wind with local exit")

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

(display "          === Testing Nested dynamic-wind ===")
(newline)
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

(display "          === Testing Re-entrant dynamic-wind ===")
(newline)
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

;; Part 2: Re-entry
(set! re-entrant-log '())
(set! re-entrant-result #f) ; Reset the result from part 1

; When (k 're-entry-value) is called, it will escape to where k was captured
; and bind 're-entry-value to re-entrant-result, executing thunks as needed
(call/cc
 (lambda (top-exit)
   (dynamic-wind
     (lambda () (log-re 'before-2))
     (lambda () (top-exit (k 're-entry-value)))
     (lambda () (log-re 'after-2)))))
; The above call/cc will never complete because k escapes to the first dynamic-wind
; So we test that re-entrant-result got the correct value from the escape
(test-equal 're-entry-value re-entrant-result "re-entrant: re-entry result")
(test-equal '(before-2 after-2 before-1 after-1) re-entrant-log "re-entrant: re-entry run order")

;; Additional dynamic-wind edge case tests
(display "          === Testing Dynamic-wind Edge Cases ===")
(newline)

;; Test 1: Error handling with continuations (simulated error in before thunk)
(define error-test-1-result #f)
(define error-test-1-caught #f)
(call/cc
  (lambda (outer-escape)
    (call/cc
      (lambda (error-escape)
        (set! error-test-1-result
          (dynamic-wind
            (lambda () (error-escape "before-error-caught"))
            (lambda () 'should-not-reach)
            (lambda () 'cleanup-not-reached)))
        (set! error-test-1-result 'should-not-reach)))
    (set! error-test-1-caught #t)))
(test-equal #t error-test-1-caught "error simulation: before thunk escape caught")

;; Test 2: Simple escape from dynamic-wind thunk
(define simple-escape-result #f)
(set! simple-escape-result
  (call/cc
    (lambda (escape)
      (dynamic-wind
        (lambda () 'setup)
        (lambda () (escape 'escaped-early))
        (lambda () 'cleanup)))))
(test-equal 'escaped-early simple-escape-result "simple escape: early exit works")

;; Test 3: Multiple escapes from same dynamic-wind
(define multi-escape-log '())
(define multi-k #f)
(define multi-result
  (dynamic-wind
    (lambda () (set! multi-escape-log (cons 'before multi-escape-log)))
    (lambda ()
      (call/cc (lambda (k) (set! multi-k k) 'first)))
    (lambda () (set! multi-escape-log (cons 'after multi-escape-log)))))

(test-equal 'first multi-result "multiple escapes: initial result")
(test-equal '(after before) multi-escape-log "multiple escapes: initial log")

;; The test name promises "multiple escapes" but originally never actually
;; invoked multi-k a second time. Re-invoking it re-enters the dynamic-wind's
;; extent from outside, which re-runs both before and after (not just
;; after), the same way the "Re-entrant dynamic-wind" test above expects.
(multi-k 'second)
(test-equal 'second multi-result "multiple escapes: re-invocation reaches the binding again")
(test-equal '(after before after before) multi-escape-log
    "multiple escapes: re-invocation re-enters the dynamic-wind's extent, re-running before and after")

(multi-k 'third)
(test-equal 'third multi-result "multiple escapes: second re-invocation still works")
(test-equal '(after before after before after before) multi-escape-log
    "multiple escapes: second re-invocation again re-runs before and after")

;; Test 4: Dynamic-wind with minimal thunk
(define empty-log '())
(define empty-thunk-result
  (dynamic-wind
    (lambda () (set! empty-log (cons 'before empty-log)))
    (lambda () #f)
    (lambda () (set! empty-log (cons 'after empty-log)))))
(test-equal #f empty-thunk-result "empty thunk: returns #f")
(test-equal '(before after) (reverse empty-log) "empty thunk: thunks execute")

;; Test 5: Deep nesting (simplified)
(define deep-simple-result
  (dynamic-wind
    (lambda () 'outer-setup)
    (lambda ()
      (dynamic-wind
        (lambda () 'inner-setup)
        (lambda () 'inner-completed)
        (lambda () 'inner-cleanup)))
    (lambda () 'outer-cleanup)))

(test-equal 'inner-completed deep-simple-result "deep nesting: simple nested completion")

;; Test 6: Dynamic-wind with side effects
(define side-effect-counter 0)
(define side-effect-result
  (call/cc
    (lambda (escape)
      (dynamic-wind
        (lambda () (set! side-effect-counter (+ side-effect-counter 1)))
        (lambda () (escape 'escaped))
        (lambda () (set! side-effect-counter (+ side-effect-counter 10)))))))

(test-equal 'escaped side-effect-result "side effects: escape result")
(test-equal 11 side-effect-counter "side effects: counter value")

;; Test 7: Nested dynamic-wind (simplified)
(define nested-simple-log '())
(define nested-simple-result
  (dynamic-wind
    (lambda () (set! nested-simple-log (cons 'outer-before nested-simple-log)))
    (lambda ()
      (dynamic-wind
        (lambda () (set! nested-simple-log (cons 'inner-before nested-simple-log)))
        (lambda () 'nested-normal)
        (lambda () (set! nested-simple-log (cons 'inner-after nested-simple-log))))
      'outer-normal)
    (lambda () (set! nested-simple-log (cons 'outer-after nested-simple-log)))))

(test-equal 'outer-normal nested-simple-result "nested simple: normal completion")
(test-equal '(outer-before inner-before inner-after outer-after) (reverse nested-simple-log) "nested simple: normal log")

;; Test 8: Simplified chain test
(define simple-chain-log '())
(define simple-chain-result
  (dynamic-wind
    (lambda () (set! simple-chain-log (cons 'dw1-before simple-chain-log)))
    (lambda ()
      (dynamic-wind
        (lambda () (set! simple-chain-log (cons 'dw2-before simple-chain-log)))
        (lambda () 'chain-normal)
        (lambda () (set! simple-chain-log (cons 'dw2-after simple-chain-log)))))
    (lambda () (set! simple-chain-log (cons 'dw1-after simple-chain-log)))))

(test-equal 'chain-normal simple-chain-result "simple chain: normal completion")
(test-equal '(dw1-before dw2-before dw2-after dw1-after) (reverse simple-chain-log) "simple chain: normal log")

(display "          === Testing values / call-with-values ===")
(newline)
(test-equal 5 (+ (values 2)3) "Singleton values call")
(test-equal '(1 2) (call-with-values (lambda () (values 1 2)) list) "values returned through a closure body")
(test-equal 3 (call-with-values (lambda () (values 1 2)) +) "call-with-values applies consumer to all values")
(test-equal '() (call-with-values (lambda () (values)) list) "zero values")
(test-equal '(7) (call-with-values (lambda () 7) list) "single plain value to consumer")
(define (two-values) (values 3 4))
(test-equal '(3 . 4) (call-with-values two-values cons) "producer defined with define")
(test-equal '(5 6)
    (call-with-values
        (lambda () (dynamic-wind (lambda () #f) (lambda () (values 5 6)) (lambda () #f)))
        list)
    "values pass through dynamic-wind")
(test-equal 4 (+ 1 (call-with-values (lambda () (values 1 2)) +)) "call-with-values in argument position")
(test-equal '(1 2) (call-with-values (lambda () (call/cc (lambda (k) (k 1 2)))) list) "continuation accepts several values")
(test-equal '() (call-with-values (lambda () (call/cc (lambda (k) (k)))) list) "continuation accepts zero values")
(test-equal 9 (let ((escape 'shadowed)) (call/cc (lambda (k) (k 9)))) "call/cc unaffected by a local named escape")

(display "          === Testing eqv? ===")
(newline)
(test-equal #t (eqv? 2 2) "eqv? equal exact integers")
(test-equal #f (eqv? 2 2.0) "eqv? differs on exactness")
(test-equal #f (eqv? 0.0 -0.0) "eqv? distinguishes signed zeros")
(test-equal #t (eqv? 100000000000000000000 100000000000000000000) "eqv? bignums by value")
(test-equal #t (eqv? #\a #\a) "eqv? characters")
(test-equal #f (eqv? "" "") "eqv? distinct strings")
(test-equal #t (eqv? car car) "eqv? same procedure")
(test-equal '(2 3) (memv 2 '(1 2 3)) "memv uses eqv?")

(display "          === Testing number->string ===")
(newline)
(test-equal "ff" (number->string 255 16) "number->string radix 16")
(test-equal "11111111" (number->string 255 2) "number->string radix 2")
(test-equal "-377" (number->string -255 8) "number->string radix 8, negative")
(test-equal "ab54a98ceb1f0ad2" (number->string 12345678901234567890 16) "number->string bignum radix 16")
(test-equal "2.0" (number->string 2.0) "number->string keeps inexactness")
(test-equal "+inf.0" (number->string (/ 1.0 0)) "number->string infinity")

(display "          === Testing nested quasiquote ===")
(newline)
(test-equal '(a (quasiquote (b (unquote (c 3))))) `(a `(b ,(c ,(+ 1 2)))) "nested unquote evaluates at depth 1")
(test-equal '(1 (quasiquote (2 (unquote-splicing (3 4))))) `(1 `(2 ,@(3 ,(+ 1 3)))) "nested unquote-splicing lowers depth")

(display "          === Testing map termination ===")
(newline)
(test-equal '(11 22) (map + '(1 2 3) '(10 20)) "map stops at the shortest list")
(test-equal '(11 22 31)
    (let ((ls (list 1 2))) (set-cdr! (cdr ls) ls) (map + ls '(10 20 30)))
    "map over a circular first list stops at the finite one")
(test-equal '() (apply map list '()) "map with no lists returns the empty list")
(test-equal '((1 3) (2 4)) (apply map list '((1 2) (3 4))) "map over several lists")

;; Expressions that need no machine step (constants, bound variables, and
;; built-in calls on those) are evaluated directly as arguments, if tests and
;; define/set! values. These check that the shortcut changes nothing.
(display "          === Testing direct evaluation of simple expressions ===")
(newline)
(define direct-calls 0)
(define (direct-bump!) (set! direct-calls (+ direct-calls 1)) direct-calls)
(test-equal '(1 1 2) (list (+ direct-calls 1) (direct-bump!) (+ direct-calls 1))
    "arguments are still evaluated left to right")
(test-equal "unbound variable: no-such-variable-here"
    (guard (e (#t (error-object-message e))) (list 1 no-such-variable-here))
    "an unbound variable argument raises as before")
(test-equal 'caught (guard (e (#t 'caught)) (list 1 (car 5) 3)) "a built-in's error in an argument is catchable")
(test-equal 'caught (guard (e (#t 'caught)) (if (car 5) 1 2)) "a built-in's error in an if test is catchable")
(test-equal 'shadow (let ((car (lambda (x) 'shadow))) (car '(1 2))) "a locally rebound built-in isn't called directly")
(test-equal 6 (+ 1 (call/cc (lambda (k) (k 2))) 3) "call/cc in an argument after direct ones")
(test-equal '(1 2) (let ((x 1) (y 0)) (set! y (+ x 1)) (list x y)) "set! of a built-in call's value")
(test-equal 'yes (if (pair? '(a)) 'yes 'no) "an if test decided directly")

;; Quoted data and nested built-in calls are evaluated directly too, and a
;; call whose arguments all are is applied without an EvalArg frame. Nothing
;; in a nested expression runs unless all of it qualifies.
(define direct-port (open-input-string "abc"))
(define (direct-id x) x)
(test-equal '(97 #\b) (list (char->integer (read-char direct-port)) (direct-id (read-char direct-port)))
    "a nested built-in call runs once, left to right")
(test-equal "cz" (string (read-char direct-port) (direct-id #\z)) "a nested call next to a closure call isn't repeated")
(test-equal '((a b) 2 (3)) (list '(a b) (car (cdr '(1 2 3))) (cons (+ 1 2) '())) "quoted and nested arguments")
(test-equal 'caught (guard (e (#t 'caught)) (list 1 (car (cdr '(1))))) "an error in a nested call is catchable")
(test-equal '(5) (let ((quote (lambda (x) 'shadowed))) (list (car (list 5)))) "a rebound quote is not taken for quote")
(test-equal '(shadowed) (let ((quote (lambda (x) 'shadowed))) (list (quote 5))) "a rebound quote is called")

;; let, named let, let*, letrec, do, guard and lambda bodies are rewritten
;; once per form from its second evaluation on (see rewrite_once in
;; special_forms.rs). Each evaluation must still behave as a fresh one.
(display "          === Testing cached rewrites of binding forms ===")
(newline)
(define (rw-counter) (let ((n 0)) (lambda () (set! n (+ n 1)) n)))
(define rw-c1 (rw-counter))
(define rw-c2 (rw-counter))
(define rw-c3 (rw-counter))
(rw-c1)
(test-equal '(2 1 1) (list (rw-c1) (rw-c2) (rw-c3)) "each let evaluation makes its own closure and frame")
(define (rw-adders) (let loop ((i 0) (acc '())) (if (= i 3) (reverse acc) (loop (+ i 1) (cons (lambda (x) (+ x i)) acc)))))
(test-equal '((10 11 12) (10 11 12) #f)
    (list (map (lambda (f) (f 10)) (rw-adders)) (map (lambda (f) (f 10)) (rw-adders)) (eq? (car (rw-adders)) (car (rw-adders))))
    "named let and lambda: a fresh closure per evaluation")
(define (rw-mk i) (let* ((a i) (b (* a 2))) (letrec ((f (lambda () (+ a b)))) (do ((j 0 (+ j 1)) (acc '() (cons (f) acc))) ((= j 2) acc)))))
(test-equal '((3 3) (6 6) (9 9)) (map rw-mk '(1 2 3)) "let*, letrec and do evaluated repeatedly")
(define (rw-inner x) (define (sq y) (* y y)) (define z (sq x)) (+ z 1))
(test-equal '(5 10 17) (map rw-inner '(2 3 4)) "internal definitions evaluated repeatedly")
(define-syntax rw-twice (syntax-rules () ((_ x) (* 2 x))))
(define (rw-macro-user n) (let loop ((i 0) (acc '())) (if (= i n) acc (loop (+ i 1) (cons (rw-twice i) acc)))))
(rw-macro-user 2)
(rw-macro-user 2)
(define (rw-guarded x) (guard (e ((symbol? e) (list 'sym e)) ((string? e) (list 'str e))) (raise x)))
(test-equal '((sym a) (str "b") (sym c)) (map rw-guarded (list 'a "b" 'c)) "guard evaluated repeatedly")
(test-equal '(outer 1) (guard (e (#t (list 'outer e))) (rw-guarded 1) (rw-guarded 1)) "guard re-raises an unmatched condition each time")
(define rw-ks '())
(define (rw-cc n) (let ((a (call/cc (lambda (k) (set! rw-ks (cons k rw-ks)) n))) (b (* n 10))) (list a b)))
(test-equal '((1 10) (2 20) (3 30)) (list (rw-cc 1) (rw-cc 2) (rw-cc 3)) "call/cc in a let init")
(define (rw-bad n) (eval '(let ((x)) x) (interaction-environment)))
(test-equal '(e e e) (map (lambda (n) (guard (e (#t 'e)) (rw-bad n))) '(1 2 3)) "a malformed let raises every time")
