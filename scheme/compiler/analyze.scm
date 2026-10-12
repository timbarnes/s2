;;; Analysis over the core AST, as src/compile/analyze.rs does it:
;;;
;;; - the free variables of each lambda, the values its flat closure copies;
;;; - which variables need boxes;
;;; - which letrecs are lambda groups.
;;;
;;; Variables are numbers here, unique within a top-level form, so no
;;; scopes are needed. Sets are lists.

;; The local variables that lambda `l` references but doesn't bind, in
;; order of first reference. A variable is free if it is referenced
;; anywhere inside (nested lambdas included) and bound nowhere inside.
;; Assigning a variable counts as a reference: a captured, assigned
;; variable's box must be copied.
(define (free-variables l)
  (let ((bound (map var-id (lambda-vars l)))
        (refs '()))
    (define (bind! v) (set! bound (cons (var-id v) bound)))
    (define (ref! id) (set! refs (cons id refs)))
    (define (walk e)
      (case (node-tag e)
        ((const global-ref) #f)
        ((local-ref) (ref! (local-ref-id e)))
        ((local-set) (ref! (local-set-id e)) (walk (local-set-value e)))
        ((global-define global-set) (walk (global-value e)))
        ((if) (walk (if-test e)) (walk (if-then e)) (walk (if-else e)))
        ((seq) (for-each walk (seq-body e)))
        ((lambda) (for-each bind! (lambda-vars e)) (walk (lambda-body e)))
        ((call) (walk (call-fn e)) (for-each walk (call-args e)))
        ((let)
         (for-each (lambda (b) (bind! (binding-var b)) (walk (binding-init b))) (let-bindings e))
         (walk (let-body e)))
        ((letrec)
         (for-each (lambda (b) (bind! (binding-var b))) (let-bindings e))
         (for-each (lambda (b) (walk (binding-init b))) (let-bindings e))
         (walk (let-body e)))))
    (walk (lambda-body l))
    (let loop ((refs (reverse refs)) (free '()))
      (cond ((null? refs) (reverse free))
            ((or (memv (car refs) bound) (memv (car refs) free)) (loop (cdr refs) free))
            (else (loop (cdr refs) (cons (car refs) free)))))))

;; Whether a letrec's bindings are a lambda group: every init a lambda,
;; and no variable boxed.
(define (lambda-group? bindings boxed)
  (every? (lambda (b)
            (and (eq? (node-tag (binding-init b)) 'lambda)
                 (not (memv (var-id (binding-var b)) boxed))))
          bindings))

;; The variables that live in a box (see analyze.rs for why): those
;; assigned with set!, and letrec variables referenced before their init
;; has run, outside lambda groups.
(define (boxed-variables e)
  (let ((assigned '()) (pending '()) (early '()))
    (define (add id set) (if (memv id set) set (cons id set)))
    (define (collect e)
      (case (node-tag e)
        ((const global-ref local-ref) #f)
        ((local-set)
         (set! assigned (add (local-set-id e) assigned))
         (collect (local-set-value e)))
        ((global-define global-set) (collect (global-value e)))
        ((if) (collect (if-test e)) (collect (if-then e)) (collect (if-else e)))
        ((seq) (for-each collect (seq-body e)))
        ((lambda) (collect (lambda-body e)))
        ((call) (collect (call-fn e)) (for-each collect (call-args e)))
        ((let letrec)
         (for-each (lambda (b) (collect (binding-init b))) (let-bindings e))
         (collect (let-body e)))))
    (define (used! id)
      (if (memv id pending) (set! early (add id early))))
    (define (walk e)
      (case (node-tag e)
        ((const global-ref) #f)
        ((local-ref) (used! (local-ref-id e)))
        ((local-set) (used! (local-set-id e)) (walk (local-set-value e)))
        ((global-define global-set) (walk (global-value e)))
        ((if) (walk (if-test e)) (walk (if-then e)) (walk (if-else e)))
        ((seq) (for-each walk (seq-body e)))
        ((lambda) (walk (lambda-body e)))
        ((call) (walk (call-fn e)) (for-each walk (call-args e)))
        ((let)
         (for-each (lambda (b) (walk (binding-init b))) (let-bindings e))
         (walk (let-body e)))
        ((letrec)
         ;; In a lambda group nothing runs during the inits, so no
         ;; reference is early.
         (if (not (lambda-group? (let-bindings e) assigned))
             (for-each (lambda (b) (set! pending (cons (var-id (binding-var b)) pending)))
                       (let-bindings e)))
         (for-each (lambda (b)
                     (walk (binding-init b))
                     (set! pending (remove-id (var-id (binding-var b)) pending)))
                   (let-bindings e))
         (walk (let-body e)))))
    (collect e)
    (walk e)
    (let loop ((early early) (boxed assigned))
      (if (null? early) boxed (loop (cdr early) (add (car early) boxed))))))

(define (remove-id id ids)
  (cond ((null? ids) '())
        ((eqv? (car ids) id) (remove-id id (cdr ids)))
        (else (cons (car ids) (remove-id id (cdr ids))))))
