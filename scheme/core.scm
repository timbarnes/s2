;;; core.scm: R7RS derived syntax (§7.3) as syntax-rules macros, and the
;;; standard procedures that are written in Scheme: the ones that call
;;; procedures they are given (map, for-each, member with a predicate…),
;;; which a builtin can't do (src/builtins/mod.rs).
;;;
;;; Compiled into s2 and loaded into every session. The core forms the
;;; expander implements itself are quote, quasiquote, lambda, define, set!,
;;; if, begin, let (and named let), letrec, letrec*, define-syntax,
;;; let-syntax, letrec-syntax and syntax-rules (src/compile/expand.rs).
;;;
;;; Identifiers these templates introduce, such as `if`, `let` and `memv`,
;;; mean what they mean here, at top level, whatever the code that uses
;;; the macro binds: that is hygiene.
;;;

(define-syntax and
  (syntax-rules ()
    ((_) #t)
    ((_ test) test)
    ((_ test1 test2 ...) (if test1 (and test2 ...) #f))))

(define-syntax or
  (syntax-rules ()
    ((_) #f)
    ((_ test) test)
    ((_ test1 test2 ...) (let ((x test1)) (if x x (or test2 ...))))))

(define-syntax when
  (syntax-rules ()
    ((_ test result1 result2 ...) (if test (begin result1 result2 ...)))))

(define-syntax unless
  (syntax-rules ()
    ((_ test result1 result2 ...) (if test (if #f #f) (begin result1 result2 ...)))))

(define-syntax let*
  (syntax-rules ()
    ((_ () body1 body2 ...) (let () body1 body2 ...))
    ((_ ((name1 val1) (name2 val2) ...) body1 body2 ...)
     (let ((name1 val1)) (let* ((name2 val2) ...) body1 body2 ...)))))

(define-syntax cond
  (syntax-rules (else =>)
    ((_ (else result1 result2 ...)) (begin result1 result2 ...))
    ((_ (test => result)) (let ((temp test)) (if temp (result temp))))
    ((_ (test => result) clause1 clause2 ...)
     (let ((temp test)) (if temp (result temp) (cond clause1 clause2 ...))))
    ((_ (test)) test)
    ((_ (test) clause1 clause2 ...) (let ((temp test)) (if temp temp (cond clause1 clause2 ...))))
    ((_ (test result1 result2 ...)) (if test (begin result1 result2 ...)))
    ((_ (test result1 result2 ...) clause1 clause2 ...)
     (if test (begin result1 result2 ...) (cond clause1 clause2 ...)))))

(define-syntax case
  (syntax-rules (else =>)
    ((_ (key ...) clauses ...)
     (let ((atom-key (key ...))) (case atom-key clauses ...)))
    ((_ key (else => result)) (result key))
    ((_ key (else result1 result2 ...)) (begin result1 result2 ...))
    ((_ key ((atoms ...) => result)) (if (memv key '(atoms ...)) (result key)))
    ((_ key ((atoms ...) result1 result2 ...)) (if (memv key '(atoms ...)) (begin result1 result2 ...)))
    ((_ key ((atoms ...) => result) clause clauses ...)
     (if (memv key '(atoms ...)) (result key) (case key clause clauses ...)))
    ((_ key ((atoms ...) result1 result2 ...) clause clauses ...)
     (if (memv key '(atoms ...)) (begin result1 result2 ...) (case key clause clauses ...)))))

(define-syntax do
  (syntax-rules ()
    ((_ ((var init step ...) ...) (test expr ...) command ...)
     (letrec ((loop (lambda (var ...)
                      (if test
                          (begin (if #f #f) expr ...)
                          (begin command ... (loop (do "step" var step ...) ...))))))
       (loop init ...)))
    ((_ "step" x) x)
    ((_ "step" x y) y)))
;;; ---------------------------------------------------------------------
;;; Records (R7RS §5.5), over the primitives in src/builtins/record.rs.
;;; Forked from s1-core.scm.

(define-syntax define-record-type
  (syntax-rules ()
    ((_ type (constructor field ...) predicate fieldspec ...)
     (begin
       (define type (%make-record-type 'type '(fieldspec ...)))
       (define (constructor field ...) (%record-make type '(field ...) (list field ...)))
       (define (predicate obj) (%record? obj type))
       (%define-record-field type fieldspec) ...))))

;; One field spec: (field), (field accessor) or (field accessor modifier).
(define-syntax %define-record-field
  (syntax-rules ()
    ((_ type (field)) 'field)
    ((_ type (field accessor))
     (define (accessor obj) (%record-get obj type 'field)))
    ((_ type (field accessor modifier))
     (begin
       (define (accessor obj) (%record-get obj type 'field))
       (define (modifier obj value) (%record-set! obj type 'field value))))))

;;; ---------------------------------------------------------------------
;;; Lists (R7RS §6.4), forked from s1-core.scm. map and for-each stop at
;;; the shortest list.

(define (%any-null? lists)
  (cond ((null? lists) #f)
        ((null? (car lists)) #t)
        (else (%any-null? (cdr lists)))))

;; Zero lists are allowed (the result is empty), as in s1.
(define (map f . lists)
  (if (and (pair? lists) (null? (cdr lists)))
      ;; One list, the common case: walk it directly.
      (let loop ((l (car lists)))
        (if (pair? l) (cons (f (car l)) (loop (cdr l))) '()))
      (let loop ((ls lists))
        (if (or (null? ls) (%any-null? ls))
            '()
            (cons (apply f (map car ls)) (loop (map cdr ls)))))))

(define (for-each f . lists)
  (if (and (pair? lists) (null? (cdr lists)))
      (let loop ((l (car lists)))
        (when (pair? l) (f (car l)) (loop (cdr l))))
      (let loop ((ls lists))
        (unless (or (null? ls) (%any-null? ls))
          (apply f (map car ls))
          (loop (map cdr ls))))))

(define (member x lst . compare)
  (let ((same? (if (null? compare) equal? (car compare))))
    (let loop ((l lst))
      (cond ((null? l) #f)
            ((same? x (car l)) l)
            (else (loop (cdr l)))))))

(define (assoc x alist . compare)
  (let ((same? (if (null? compare) equal? (car compare))))
    (let loop ((l alist))
      (cond ((null? l) #f)
            ((same? x (caar l)) (car l))
            (else (loop (cdr l)))))))

;;; Strings and vectors (R7RS §6.10), via lists, as in s1.

(define (string-map f s . strings)
  (list->string (apply map f (map string->list (cons s strings)))))
(define (string-for-each f s . strings)
  (apply for-each f (map string->list (cons s strings))))
(define (vector-map f v . vectors)
  (list->vector (apply map f (map vector->list (cons v vectors)))))
(define (vector-for-each f v . vectors)
  (apply for-each f (map vector->list (cons v vectors))))

;;; Ports (R7RS §6.13.1).

(define (call-with-port port proc)
  (let ((result (proc port)))
    (close-port port)
    result))
(define (call-with-input-file filename proc)
  (call-with-port (open-input-file filename) proc))
(define (call-with-output-file filename proc)
  (call-with-port (open-output-file filename) proc))
;;; ---------------------------------------------------------------------
;;; Multiple values (R7RS §6.10). `values` is a builtin, which returns one
;;; value as itself and any other number as a values object.

(define (call-with-values producer consumer)
  (apply consumer (%values->list (producer))))

;;; ---------------------------------------------------------------------
;;; Continuations and dynamic-wind (R7RS §6.10), on the VM's raw %call/cc
;;; and %call/ec, which know nothing of either (plan/03 §10).
;;;
;;; %winders: the (before . after) pairs of the dynamic-wind calls whose
;;; thunks are running, innermost first.
;;; %handlers: the exception handlers installed, innermost first. A
;;; continuation saves and restores it like the winders, but installing a
;;; handler runs nothing, so it isn't a dynamic-wind: crossing a handler
;;; (a guard, say) costs nothing.

(define %winders '())
(define %handlers '())

(define (dynamic-wind before thunk after)
  (before)
  (set! %winders (cons (cons before after) %winders))
  (call-with-values thunk
    (lambda results
      (set! %winders (cdr %winders))
      (after)
      (apply values results))))

;; The longest common tail of two wind lists.
(define (%common-tail x y)
  (let ((lx (length x)) (ly (length y)))
    (let loop ((x (if (> lx ly) (list-tail x (- lx ly)) x))
               (y (if (> ly lx) (list-tail y (- ly lx)) y)))
      (if (eq? x y) x (loop (cdr x) (cdr y))))))

;; Move the dynamic extent from the wind list `from` to `to`: run the after
;; thunks of the frames being left, innermost first, then the before thunks
;; of those being entered, outermost first. %winders is set before each
;; thunk runs, so a thunk that escapes leaves it right.
(define (%do-winds from to)
  (unless (eq? from to)
    (let ((common (%common-tail from to)))
      (let leave ((w from))
        (unless (eq? w common)
          (set! %winders (cdr w))
          ((cdar w))
          (leave (cdr w))))
      (let enter ((w to))
        (unless (eq? w common)
          (enter (cdr w))
          ((caar w))
          (set! %winders w))))))

(define (call-with-current-continuation proc)
  (let ((winders %winders) (handlers %handlers))
    (%call/cc
      (lambda (k)
        (proc (lambda results
                (%do-winds %winders winders)
                (set! %handlers handlers)
                (apply k results)))))))

(define call/cc call-with-current-continuation)

;;; ---------------------------------------------------------------------
;;; Exceptions (R7RS §6.11). Errors from builtins and from the VM (an
;;; unbound variable, a bad call) arrive through %raise-error.

(define (with-exception-handler handler thunk)
  (let ((outer %handlers))
    (set! %handlers (cons handler outer))
    (call-with-values thunk
      (lambda results (set! %handlers outer) (apply values results)))))

;; Call the innermost handler, with the outer ones installed while it runs.
(define (raise-continuable obj)
  (let ((handlers %handlers))
    (if (null? handlers)
        (%uncaught obj)
        (begin
          (set! %handlers (cdr handlers))
          (call-with-values (lambda () ((car handlers) obj))
            (lambda results (set! %handlers handlers) (apply values results)))))))

(define (raise obj)
  (let ((handlers %handlers))
    (if (null? handlers) (%uncaught obj))
    (set! %handlers (cdr handlers))
    ((car handlers) obj)
    ;; The handler returned: a secondary error, where the handler ran.
    (raise (%make-error-object "exception handler returned from a non-continuable raise of" (list obj)))))

;; Nothing handles obj: leave every dynamic-wind, running the after
;; thunks, as returning to the top level does, and end the form with obj.
(define (%uncaught obj)
  (set! %handlers '())
  (%do-winds %winders '())
  (%abort obj))

(define (%raise-error obj) (raise obj))

(define (error message . irritants)
  (raise (%make-error-object message irritants)))

;; guard (R7RS §4.2.7). R7RS's reference implementation captures a full
;; continuation on entry and another to re-raise, which for s2's copied
;; stacks costs time in proportion to the stack depth: quadratic for nested
;; guards. This one costs constant time per guard (plan/phases/P7.md):
;; - the body runs under an escape-only continuation, which is free;
;; - a raised object is matched against the clauses inside the handler,
;;   with the guard's dynamic environment (winders and handlers) in place;
;;   only when a clause applies does control escape to the guard, where the
;;   clause's body runs;
;; - when none applies, the dynamic environment is put back and the object
;;   is re-raised right there, continuably, as R7RS specifies.
(define-syntax guard
  (syntax-rules ()
    ((_ (var clause ...) body1 body2 ...)
     (%guard (lambda () body1 body2 ...)
             (lambda (var) (%guard-select clause ...))))))

;; The clauses as a choice: #f if none applies, else a thunk for the
;; chosen clause's body.
(define-syntax %guard-select
  (syntax-rules (else =>)
    ((_) #f)
    ((_ (else result1 result2 ...)) (lambda () result1 result2 ...))
    ((_ (test => receiver) clause ...)
     (let ((t test)) (if t (lambda () (receiver t)) (%guard-select clause ...))))
    ((_ (test) clause ...)
     (let ((t test)) (if t (lambda () t) (%guard-select clause ...))))
    ((_ (test result1 result2 ...) clause ...)
     (if test (lambda () result1 result2 ...) (%guard-select clause ...)))))

(define (%guard body select)
  (let ((winders %winders) (handlers %handlers))
    ((%call/ec
       (lambda (k)
         (with-exception-handler
           (lambda (condition)
             (let ((here-winders %winders) (here-handlers %handlers))
               (%do-winds here-winders winders)
               (set! %handlers handlers)
               (let ((chosen (select condition)))
                 (if chosen
                     (k chosen)
                     (begin
                       (%do-winds winders here-winders)
                       (set! %handlers here-handlers)
                       (raise-continuable condition))))))
           (lambda ()
             (call-with-values body
               (lambda results (lambda () (apply values results)))))))))))

;;; ---------------------------------------------------------------------
;;; Binding constructs for multiple values (R7RS §4.2.2, §5.3.3), from
;;; s1-core.scm.

(define-syntax let*-values
  (syntax-rules ()
    ((_ () body1 body2 ...) (let () body1 body2 ...))
    ((_ ((formals init) binding ...) body1 body2 ...)
     (call-with-values (lambda () init)
       (lambda formals (let*-values (binding ...) body1 body2 ...))))))

;; All inits are evaluated before any variable is bound: each formal is first
;; bound to a fresh temporary, and the real names are bound together at the end.
(define-syntax let-values
  (syntax-rules ()
    ((_ (binding ...) body0 body1 ...)
     (let-values "bind" (binding ...) () (let () body0 body1 ...)))
    ((_ "bind" () tmps body)
     (let tmps body))
    ((_ "bind" ((b0 e0) binding ...) tmps body)
     (let-values "mktmp" b0 e0 () (binding ...) tmps body))
    ((_ "mktmp" () e0 args bindings tmps body)
     (call-with-values (lambda () e0)
       (lambda args (let-values "bind" bindings tmps body))))
    ((_ "mktmp" (a . b) e0 (arg ...) bindings (tmp ...) body)
     (let-values "mktmp" b e0 (arg ... x) bindings (tmp ... (a x)) body))
    ((_ "mktmp" a e0 (arg ...) bindings (tmp ...) body)
     (call-with-values (lambda () e0)
       (lambda (arg ... . x) (let-values "bind" bindings (tmp ... (a x)) body))))))

(define-syntax define-values
  (syntax-rules ()
    ((_ () expr)
     (define dummy (call-with-values (lambda () expr) (lambda args #f))))
    ((_ (var) expr)
     (define var expr))
    ((_ (var0 var1 ... varn) expr)
     (begin
       (define var0 (call-with-values (lambda () expr) list))
       (define var1 (let ((v (cadr var0))) (set-cdr! var0 (cddr var0)) v)) ...
       (define varn (let ((v (cadr var0))) (set! var0 (car var0)) v))))
    ((_ (var0 var1 ... . varn) expr)
     (begin
       (define var0 (call-with-values (lambda () expr) list))
       (define var1 (let ((v (cadr var0))) (set-cdr! var0 (cddr var0)) v)) ...
       (define varn (let ((v (cdr var0))) (set! var0 (car var0)) v))))
    ((_ var expr)
     (define var (call-with-values (lambda () expr) list)))))

;;; ---------------------------------------------------------------------
;;; Parameters (R7RS §4.2.6), from s1-core.scm. A parameter object is a
;;; procedure: called with no arguments it returns the current value.
;;; parameterize talks to it through two private markers, which the
;;; built-in current-input-port and friends understand too.

(define %param-set '%param-set)
(define %param-converter '%param-converter)

(define (make-parameter value . converter)
  (let* ((convert (if (null? converter) (lambda (x) x) (car converter)))
         (current (convert value)))
    (lambda args
      (cond ((null? args) current)
            ((eq? (car args) %param-set) (set! current (cadr args)))
            ((eq? (car args) %param-converter) convert)
            (else (error "parameter object called with arguments" args))))))

;; Each binding swaps the converted value in on entry to its dynamic extent
;; and back out on exit, including exits and re-entries through continuations.
(define-syntax parameterize
  (syntax-rules ()
    ((_ () body ...) (let () body ...))
    ((_ ((param value) rest ...) body ...)
     (let* ((p param)
            (new ((p %param-converter) value))
            (old #f))
       (dynamic-wind
         (lambda () (set! old (p)) (p %param-set new))
         (lambda () (parameterize (rest ...) body ...))
         (lambda () (set! new (p)) (p %param-set old)))))))

(define (with-input-from-file filename thunk)
  (let ((port (open-input-file filename)))
    (let ((result (parameterize ((current-input-port port)) (thunk))))
      (close-port port)
      result)))

(define (with-output-to-file filename thunk)
  (let ((port (open-output-file filename)))
    (let ((result (parameterize ((current-output-port port)) (thunk))))
      (close-port port)
      result)))

;;; ---------------------------------------------------------------------
;;; Promises (R7RS §4.2.5), from s1-core.scm, following the reference
;;; implementation, which forces chains of delay-force iteratively (in
;;; constant space). A promise holds a box (done? . value-or-thunk);
;;; promises that share a box were merged by force.

(define-record-type %promise
  (%make-promise-record box)
  promise?
  (box %promise-box %promise-set-box!))
(define (%make-promise done? value) (%make-promise-record (cons done? value)))
(define (%promise-done? p) (car (%promise-box p)))
(define (%promise-value p) (cdr (%promise-box p)))
(define (%promise-update! new old)
  (set-car! (%promise-box old) (%promise-done? new))
  (set-cdr! (%promise-box old) (%promise-value new))
  (%promise-set-box! new (%promise-box old)))

(define (force promise)
  (if (promise? promise)
      (if (%promise-done? promise)
          (%promise-value promise)
          (%force-step promise ((%promise-value promise))))
      promise))
(define (%force-step promise promise*)
  (if (not (%promise-done? promise)) (%promise-update! promise* promise))
  (force promise))

(define (make-promise obj)
  (if (promise? obj) obj (%make-promise #t obj)))

(define-syntax delay-force
  (syntax-rules () ((_ expr) (%make-promise #f (lambda () expr)))))
(define-syntax delay
  (syntax-rules () ((_ expr) (delay-force (%make-promise #t expr)))))

;;; ---------------------------------------------------------------------
;;; case-lambda (R7RS §4.2.9): a procedure that takes any number of
;;; arguments and applies the first clause whose formals accept them.
;;; Proper-list formals are tried by exact length, dotted ones by minimum
;;; length; a single identifier takes anything.

(define-syntax case-lambda
  (syntax-rules ()
    ((_ (formals body1 body2 ...) ...)
     (lambda args (%case-lambda args (formals body1 body2 ...) ...)))))

(define-syntax %case-lambda
  (syntax-rules ()
    ((_ args)
     (error (string-append "case-lambda: no clause accepts "
                           (number->string (length args)) " arguments")))
    ((_ args ((p ...) body ...) clause ...)
     (if (= (length args) (length '(p ...)))
         (apply (lambda (p ...) body ...) args)
         (%case-lambda args clause ...)))
    ((_ args ((p ... . r) body ...) clause ...)
     (if (>= (length args) (length '(p ...)))
         (apply (lambda (p ... . r) body ...) args)
         (%case-lambda args clause ...)))
    ((_ args (r body ...) clause ...)
     (apply (lambda r body ...) args))))

;;; ---------------------------------------------------------------------
;;; Loading (R7RS §6.12, §5.6) and leaving (§6.14).

;; Load a library file, for an import that needs one (src/libraries.rs):
;; marked loaded first, so a library that imports itself can't loop. Its
;; forms are evaluated in the interaction environment.
(define (%load-library path)
  (%library-file-loading path)
  (for-each (lambda (form) (eval form (interaction-environment)))
            (%read-library-file path)))

;; What eval becomes when its expression imports a library whose file
;; isn't loaded yet.
(define (%load-then-eval path expr env)
  (%load-library path)
  (eval expr env))

(define (load filename . env)
  (let ((env (if (null? env) (interaction-environment) (car env))))
    (for-each (lambda (form) (eval form env)) (%read-file-forms filename))))

;; exit runs the after thunks of every dynamic-wind it is inside, then
;; ends the process; emergency-exit doesn't.
(define (exit . status)
  (%do-winds %winders '())
  (apply %exit status))

(define (emergency-exit . status)
  (apply %exit status))
