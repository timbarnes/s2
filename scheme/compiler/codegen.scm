;;; Code generation: core AST to templates, as src/compile/codegen.rs does
;;; it, instruction for instruction (plan/08, C7).
;;;
;;; A post-order walk. Each expression's code leaves exactly one value on
;;; the stack. Two things are tracked:
;;;
;;; - the depth: the number of values in the current frame. A let
;;;   variable is the slot its init's value landed in;
;;; - whether an expression is in tail position: a call there becomes
;;;   tail-call, and anything else is followed by return.
;;;
;;; Instructions are lists, `(name operand ...)` (src/compile/data.rs),
;;; and %make-template turns them into a template. A jump is emitted with
;;; target 0 and patched when the target is known.

(define-record-type gen
  (make-gen boxed code consts depth locs names local-names free-names self-var next-self params)
  gen?
  (boxed gen-boxed)
  ;; a buffer of instructions, and one of constants
  (code gen-code)
  (consts gen-consts)
  (depth gen-depth set-gen-depth!)
  ;; Where each variable lives: (id local . slot) or (id free . index).
  (locs gen-locs set-gen-locs!)
  ;; Each variable's name: (id . string).
  (names gen-names set-gen-names!)
  ;; Each slot's names, a list, joined with / at the end.
  (local-names gen-local-names)
  (free-names gen-free-names)
  ;; The lambda-group variable this procedure is bound to, if any: a
  ;; tail call to it is a self-tail-call.
  (self-var gen-self-var set-gen-self-var!)
  ;; While compiling a lambda group's init: the variable it is bound to.
  (next-self gen-next-self set-gen-next-self!)
  (params gen-params set-gen-params!))

(define (new-gen boxed)
  (make-gen boxed (make-buffer) (make-buffer) 0 '() '() (make-buffer) (make-buffer) #f #f 0))

;; Compile a top-level form into a template of no arguments.
(define (codegen e)
  (let ((g (new-gen (boxed-variables e))))
    (expr g e #t)
    (finish g "top-level" 0 #f)))

(define (finish g name required rest?)
  (let ((slot-names (map (lambda (names)
                           (if (null? names)
                               ""
                               (let join ((names (cdr names)) (text (car names)))
                                 (if (null? names) text (join (cdr names) (string-append text "/" (car names)))))))
                         (vector->list (buffer->vector (gen-local-names g))))))
    (%make-template name required rest? (buffer-count (gen-free-names g))
                    (buffer->vector (gen-code g))
                    (buffer->vector (gen-consts g))
                    (list->vector slot-names)
                    (buffer->vector (gen-free-names g)))))

(define (depth+! g n) (set-gen-depth! g (+ (gen-depth g) n)))

;; The pool index of `v`, adding it if it isn't there. Constants are
;; shared by identity.
(define (constant g v)
  (or (buffer-index (gen-consts g) v eq?) (buffer-push! (gen-consts g) v)))

;; Emit an instruction; its index.
(define (emit g . instruction) (buffer-push! (gen-code g) instruction))

;; Point the jump at `at` to the next instruction to be emitted.
(define (patch g at)
  (set-car! (last-pair (buffer-ref (gen-code g) at)) (buffer-count (gen-code g))))

;; Return the value just pushed, if in tail position.
(define (ret g tail?) (if tail? (emit g 'return)))

(define (boxed? g id) (memv id (gen-boxed g)))
(define (loc g id) (cdr (assv id (gen-locs g))))
(define (name-of g id) (cdr (assv id (gen-names g))))

(define (bind-local g var slot)
  (let ((name (identifier-name (var-name var)))
        (slots (gen-local-names g)))
    (set-gen-locs! g (cons (cons (var-id var) (cons 'local slot)) (gen-locs g)))
    (if (<= (buffer-count slots) slot) (buffer-set! slots slot '() '()))
    (let ((names (buffer-ref slots slot)))
      (if (not (member name names)) (buffer-set! slots slot (append names (list name)) '())))
    (set-gen-names! g (cons (cons (var-id var) name) (gen-names g)))))

;; Push the value of local variable `id`.
(define (load g id)
  (load-raw g id)
  (if (boxed? g id) (emit g 'box-ref)))

;; Push what `id`'s slot holds: its box, if it has one.
(define (load-raw g id)
  (let ((l (loc g id)))
    (if (eq? (car l) 'local) (emit g 'local-ref (cdr l)) (emit g 'free-ref (cdr l))))
  (depth+! g 1))

(define (box-if-needed g var slot)
  (if (boxed? g (var-id var)) (emit g 'make-box slot)))

;; Assign the value of `value` to `id`, leaving nothing on the stack.
(define (assign g id value)
  (if (boxed? g id)
      (begin (load-raw g id)
             (expr g value #f)
             (emit g 'box-set)
             (depth+! g -2))
      (begin (expr g value #f)
             ;; Captured and assigned means boxed, so this is a slot.
             (emit g 'local-set (cdr (loc g id)))
             (depth+! g -1))))

(define (unspecified g tail?)
  (emit g 'unspecified)
  (depth+! g 1)
  (ret g tail?))

(define (expr g e tail?)
  (case (node-tag e)
    ((const) (constant-op g (const-value e)) (depth+! g 1) (ret g tail?))
    ((local-ref) (load g (local-ref-id e)) (ret g tail?))
    ((local-set) (assign g (local-set-id e) (local-set-value e)) (unspecified g tail?))
    ((global-set)
     (expr g (global-value e) #f)
     (emit g 'global-set (constant g (global-cell e)))
     (depth+! g -1)
     (unspecified g tail?))
    ((global-ref)
     (emit g 'global-ref (constant g (global-cell e)))
     (depth+! g 1)
     (ret g tail?))
    ((global-define)
     (expr g (global-value e) #f)
     (emit g 'global-define (constant g (global-cell e)))
     (ret g tail?))
    ((if) (if-expr g e tail?))
    ((seq)
     (let loop ((body (seq-body e)))
       (if (null? (cdr body))
           (expr g (car body) tail?)
           (begin (expr g (car body) #f)
                  (emit g 'pop)
                  (depth+! g -1)
                  (loop (cdr body))))))
    ((lambda)
     (let ((self-var (gen-next-self g))
           (free (free-variables e)))
       (set-gen-next-self! g #f)
       (for-each (lambda (id) (load-raw g id)) free)
       (let ((t (lambda-template g e free self-var)))
         (emit g 'make-closure (constant g t))
         (depth+! g (- 1 (length free)))
         (ret g tail?))))
    ((call) (call-expr g e tail?))
    ((let)
     (let ((base (gen-depth g)) (bindings (let-bindings e)))
       (for-each (lambda (b) (expr g (binding-init b) #f)) bindings)
       (let loop ((bindings bindings) (i 0))
         (if (pair? bindings)
             (let ((var (binding-var (car bindings))))
               (bind-local g var (+ base i))
               (box-if-needed g var (+ base i))
               (loop (cdr bindings) (+ i 1)))))
       (end-scope g (let-body e) tail? (length bindings))))
    ((letrec)
     ;; Every variable gets its slot (and box) first, undefined, so the
     ;; inits can capture them; then each init is assigned in order.
     (let ((base (gen-depth g)) (bindings (let-bindings e)))
       (let loop ((bs bindings) (i 0))
         (if (pair? bs)
             (let ((var (binding-var (car bs))))
               (emit g 'undefined)
               (depth+! g 1)
               (bind-local g var (+ base i))
               (box-if-needed g var (+ base i))
               (loop (cdr bs) (+ i 1)))))
       (let ((group? (lambda-group? bindings (gen-boxed g))))
         (for-each (lambda (b)
                     (set-gen-next-self! g (and group? (var-id (binding-var b))))
                     (assign g (var-id (binding-var b)) (binding-init b))
                     (set-gen-next-self! g #f))
                   bindings)
         (if group? (patch-group g bindings)))
       (end-scope g (let-body e) tail? (length bindings))))))

(define (if-expr g e tail?)
  ;; (if (not x) a b) is (if x b a).
  (let loop ((test (if-test e)) (then (if-then e)) (els (if-else e)))
    (if (and (eq? (node-tag test) 'call)
             (eq? (node-tag (call-fn test)) 'global-ref)
             (equal? (inlinable (call-fn test) (length (call-args test))) '(not)))
        (loop (car (call-args test)) els then)
        (let* ((fused (fused-test g test))
               ;; A comparison with a small constant.
               (imm (and fused
                         (let ((t (car fused)) (args (cdr fused)))
                           (and (= (test-arity t) 2)
                                (not (eq? t 'eq?))
                                (eq? (node-tag (cadr args)) 'const)
                                (small? (const-value (cadr args)) -32768 32767)
                                (list t (car args) (const-value (cadr args)))))))
               (to-else
                (cond (imm
                       (expr g (cadr imm) #f)
                       (depth+! g -1)
                       (emit g 'jump-if-not-imm (car imm) (caddr imm) 0))
                      (fused
                       (for-each (lambda (a) (expr g a #f)) (cdr fused))
                       (depth+! g (- (length (cdr fused))))
                       (emit g 'jump-if-not (car fused) 0))
                      (else
                       (expr g test #f)
                       (depth+! g -1)
                       (emit g 'jump-if-false 0))))
               (depth (gen-depth g)))
          (expr g then tail?)
          (set-gen-depth! g depth)
          (if tail?
              ;; The then-branch has returned, so no jump over the else.
              (begin (patch g to-else) (expr g els #t))
              (let ((to-end (emit g 'jump 0)))
                (patch g to-else)
                (expr g els #f)
                (patch g to-end)))))))

(define (call-expr g e tail?)
  (let* ((f (call-fn e)) (args (call-args e)) (n (length args)))
    (cond
     ;; A loop: a tail call from a lambda-group procedure to itself.
     ((and tail?
           (eq? (node-tag f) 'local-ref)
           (gen-self-var g)
           (eqv? (local-ref-id f) (gen-self-var g))
           (= n (gen-params g)))
      (for-each (lambda (a) (expr g a #f)) args)
      (emit g 'self-tail-call n))
     ((and (eq? (node-tag f) 'global-ref) (inlinable f n))
      => (lambda (op)
           (if (not (superinstruction g op args tail?))
               (begin (for-each (lambda (a) (expr g a #f)) args)
                      (apply emit g op)
                      (depth+! g (- 1 n))
                      (ret g tail?)))))
     (else
      (expr g f #f)
      (for-each (lambda (a) (expr g a #f)) args)
      (emit g (if tail? 'tail-call 'call) n)
      (depth+! g (- n))))))

;; Whether `v` is an exact integer from `lo` to `hi`.
(define (small? v lo hi) (and (exact-integer? v) (<= lo v hi)))

(define (small-constant e)
  (and (eq? (node-tag e) 'const)
       (small? (const-value e) -2147483648 2147483647)
       (const-value e)))

;; Compile the inlined call `op` of `args` as a superinstruction, if it is
;; one: car or cdr of an unboxed local, or adding or subtracting a
;; constant. #f if it isn't.
(define (superinstruction g op args tail?)
  (define (done) (ret g tail?) #t)
  (case (car op)
    ((car cdr)
     (let ((a (car args)))
       (and (eq? (node-tag a) 'local-ref)
            (not (boxed? g (local-ref-id a)))
            (let ((l (loc g (local-ref-id a))))
              (and (eq? (car l) 'local)
                   (begin (emit g (if (eq? (car op) 'car) 'car-local 'cdr-local) (cdr l))
                          (depth+! g 1)
                          (done)))))))
    ((add)
     (let ((a (car args)) (b (cadr args)))
       (cond ((small-constant b) => (lambda (k) (expr g a #f) (emit g 'add-imm k) (done)))
             ((small-constant a) => (lambda (k) (expr g b #f) (emit g 'add-imm k) (done)))
             (else #f))))
    ((sub)
     (let ((k (small-constant (cadr args))))
       (and k (begin (expr g (car args) #f) (emit g 'sub-imm k) (done)))))
    (else #f)))

;; The instruction a call to the global `ref` (a global-ref node) with
;; `argc` arguments is inlined as, or #f.
(define (inlinable ref argc) (%inline-instruction (global-cell ref) argc))

;; The test of an if whose test is an inlinable predicate or comparison,
;; and its arguments: (test arg ...), or #f.
(define (fused-test g test)
  (and (eq? (node-tag test) 'call)
       (eq? (node-tag (call-fn test)) 'global-ref)
       (let* ((op (inlinable (call-fn test) (length (call-args test))))
              (t (and op (test-of (car op)))))
         (and t (cons t (call-args test))))))

(define (test-of name)
  (case name
    ((null?) 'null?) ((pair?) 'pair?) ((eq?) 'eq?)
    ((num-eq) '=) ((lt) '<) ((gt) '>) ((le) '<=) ((ge) '>=)
    (else #f)))

(define (test-arity t) (if (memq t '(null? pair?)) 1 2))

;; After a lambda group's closures are made, in order: each one that
;; captured a member made at or after it captured a placeholder, so fill
;; in the member.
(define (patch-group g bindings)
  (let ((ids (map (lambda (b) (var-id (binding-var b))) bindings)))
    (let loop ((bs bindings) (i 0))
      (if (pair? bs)
          (let ((var (binding-var (car bs))))
            (let free ((fvs (free-variables (binding-init (car bs)))) (j 0))
              (if (pair? fvs)
                  (let ((k (index-of (car fvs) ids)))
                    (if (and k (>= k i))
                        (let ((c (loc g (var-id var))) (v (loc g (car fvs))))
                          (if (and (eq? (car c) 'local) (eq? (car v) 'local))
                              (emit g 'patch-free (cdr c) j (cdr v)))))
                    (free (cdr fvs) (+ j 1)))))
            (loop (cdr bs) (+ i 1)))))))

;; Compile the body of a let or letrec whose `n` variables are the top
;; `n` slots, then drop them unless the frame is ending anyway.
(define (end-scope g body tail? n)
  (expr g body tail?)
  (if (and (not tail?) (> n 0))
      (begin (emit g 'slide n) (depth+! g (- n)))))

;; Compile an inner lambda into its own template. Its free variables,
;; `free`, are free-ref 0, free-ref 1, ... inside it.
(define (lambda-template g l free self-var)
  (let ((inner (new-gen (gen-boxed g)))
        (vars (lambda-vars l)))
    (set-gen-self-var! inner (and (not (lambda-rest l)) self-var))
    (set-gen-params! inner (length (lambda-params l)))
    (let loop ((vars vars) (i 0))
      (if (pair? vars)
          (begin (bind-local inner (car vars) i)
                 (box-if-needed inner (car vars) i)
                 (loop (cdr vars) (+ i 1)))))
    (let loop ((free free) (i 0))
      (if (pair? free)
          (let ((name (name-of g (car free))))
            (set-gen-locs! inner (cons (cons (car free) (cons 'free i)) (gen-locs inner)))
            (buffer-push! (gen-free-names inner) name)
            (set-gen-names! inner (cons (cons (car free) name) (gen-names inner)))
            (loop (cdr free) (+ i 1)))))
    (set-gen-depth! inner (length vars))
    (expr inner (lambda-body l) #t)
    (finish inner (lambda-name l) (length (lambda-params l)) (and (lambda-rest l) #t))))

;; Push a literal, with a dedicated instruction where there is one.
(define (constant-op g v)
  (cond ((small? v -2147483648 2147483647) (emit g 'fixnum v))
        ((null? v) (emit g 'nil))
        ((eq? v (if #f #f)) (emit g 'unspecified))
        ((eq? v #t) (emit g 'true))
        ((eq? v #f) (emit g 'false))
        (else (emit g 'const (constant g v)))))
