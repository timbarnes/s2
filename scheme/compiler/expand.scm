;;; The expander: datum to core AST, with hygienic macros, as
;;; src/compile/expand.rs does it. Its output must be the Rust expander's
;;; exactly, down to the variable numbers (plan/08, C7), so the order in
;;; which it expands and numbers things follows the Rust code's.
;;;
;;; The syntactic environment says what each identifier means. Local scopes
;;; are frames, innermost first; below them is a top-level environment (a
;;; Rust object, through %env-lookup and friends). A binding is
;;; (local . id), (macro . m), (core . name), or (global key . env) for a
;;; top-level variable, bound or not, keyed by its identifier in `env`.
;;;
;;; Hygiene is by renaming (syntax-rules.scm): an alias bound nowhere means
;;; what its original means in the macro's definition scope.

(define-record-type expander
  (make-expander env frames nframes next-id nesting)
  expander?
  ;; The top-level environment being compiled for (a library's, while
  ;; define-library expands its body).
  (env expander-env set-expander-env!)
  ;; Each frame is a one-element vector holding ((id . binding) ...).
  (frames expander-frames set-expander-frames!)
  (nframes expander-nframes set-expander-nframes!)
  (next-id expander-next-id set-expander-next-id!)
  ;; Macro expansions in progress, each inside the last.
  (nesting expander-nesting set-expander-nesting!))

;; The deepest nesting of macro uses allowed, to turn a macro that expands
;; forever into an error.
(define max-nested-expansions 10000)

;; Expand a top-level form for the environment `env`.
(define (expand form env)
  (expr (make-expander env '() 0 0 0) form #t))

(define (push-frame! x bindings)
  (set-expander-frames! x (cons (vector bindings) (expander-frames x)))
  (set-expander-nframes! x (+ (expander-nframes x) 1)))

(define (pop-frame! x)
  (set-expander-frames! x (cdr (expander-frames x)))
  (set-expander-nframes! x (- (expander-nframes x) 1)))

;; Bind `id` in the innermost frame.
(define (frame-bind! x id binding)
  (let ((frame (car (expander-frames x))))
    (vector-set! frame 0 (cons (cons id binding) (vector-ref frame 0)))))

;;; Resolving identifiers

(define (resolve x id) (resolve-in x id (expander-nframes x) (expander-env x)))

;; What `id` means in the scope of the outermost `depth` frames over the
;; top-level environment `env`.
(define (resolve-in x id depth env)
  (let loop ((id id) (depth depth) (env env))
    (let find ((frames (list-tail (expander-frames x) (- (expander-nframes x) depth))))
      (cond ((null? frames)
             (let ((top (%env-lookup env id)))
               (cond ((and top (eq? (car top) 'variable)) (cons 'global (cons id env)))
                     (top top)
                     ((%alias? id) (loop (alias-original id) (alias-depth id) (alias-env id)))
                     (else (cons 'global (cons id env))))))
            ((assq id (vector-ref (car frames) 0)) => cdr)
            (else (find (cdr frames)))))))

;; The cell of a bound global variable, if `key` is one in `env`.
(define (bound-cell key env)
  (let ((top (%env-lookup env key)))
    (and top (eq? (car top) 'variable) (%cell-bound? (cdr top)) (cdr top))))

;; free-identifier=? on resolved bindings. Two globals are the same if they
;; are the same variable, or are both unbound and named alike.
(define (same-binding? a b)
  (if (and (eq? (car a) 'global) (eq? (car b) 'global))
      (let ((c1 (bound-cell (cadr a) (cddr a))) (c2 (bound-cell (cadr b) (cddr b))))
        (cond ((and c1 c2) (eq? c1 c2))
              ((or c1 c2) #f)
              (else (eq? (strip (cadr a)) (strip (cadr b))))))
      (and (eq? (car a) (car b))
           (not (eq? (car a) 'global))
           (eqv? (cdr a) (cdr b)))))

;; The core form or macro `form` is a use of: its binding, or #f.
(define (head-binding x form)
  (and (pair? form) (identifier? (car form)) (resolve x (car form))))

(define (head-core x form)
  (let ((b (head-binding x form))) (and b (eq? (car b) 'core) (cdr b))))

(define (head-macro x form)
  (let ((b (head-binding x form))) (and b (eq? (car b) 'macro) (cdr b))))

;;; Macro uses

(define (expand-macro x m form)
  (let* ((sr (macro->transformer m))
         (depth (transformer-depth sr))
         (env (transformer-env sr))
         (literal-matches? (lambda (literal input)
                             (same-binding? (resolve-in x literal depth env) (resolve x input))))
         (match (match-use sr form literal-matches?)))
    (instantiate sr (car match) (cdr match))))

;; Expand macro uses at the head of `form` until it isn't one.
(define (expand-head x form)
  (let ((m (head-macro x form)))
    (if m (expand-head x (expand-macro x m form)) form)))

;; A fresh variable for the identifier `id`.
(define (fresh x id)
  (set-expander-next-id! x (+ (expander-next-id x) 1))
  (vector 'var (expander-next-id x) id))

;; Run `thunk` in a new frame binding `vars`.
(define (scoped x vars thunk)
  (push-frame! x (map (lambda (v) (cons (var-name v) (cons 'local (var-id v)))) vars))
  (let ((result (thunk)))
    (pop-frame! x)
    result))

(define (items-of form)
  (if (list? form) form (fail (string-append "improper list in code: " (show form)))))

(define (syntax-error keyword items)
  (fail (string-append keyword ": bad syntax: (" (join (map show items) " ") ")")))

(define (join strings sep)
  (if (null? strings)
      ""
      (let loop ((rest (cdr strings)) (text (car strings)))
        (if (null? rest) text (loop (cdr rest) (string-append text sep (car rest)))))))

(define unspecified-node (vector 'const (if #f #f)))

;;; Expressions

;; `top?` is true for a top-level form, or a begin at top level, where
;; define and define-syntax define globally.
(define (expr x form top?)
  (cond ((identifier? form) (variable x form))
        ((head-core x form) => (lambda (c) (core x c form top?)))
        ((head-macro x form)
         => (lambda (m)
              (if (= (expander-nesting x) max-nested-expansions)
                  (fail (string-append "macro uses nested more than " (number->string max-nested-expansions)
                                       " deep: " (show form))))
              (let ((expansion (expand-macro x m form)))
                (set-expander-nesting! x (+ (expander-nesting x) 1))
                (let ((result (expr x expansion top?)))
                  (set-expander-nesting! x (- (expander-nesting x) 1))
                  result))))
        ((pair? form) (call x form))
        ((null? form) (fail "empty combination: ()"))
        ;; Numbers, strings, vectors and so on evaluate to themselves.
        (else (vector 'const (%strip-datum form)))))

(define (variable x id)
  (let ((b (resolve x id)))
    (case (car b)
      ((local) (vector 'local-ref (cdr b)))
      ((global) (vector 'global-ref (%env-cell (cddr b) (cadr b))))
      (else (fail (string-append (show id) ": syntax used as a variable"))))))

(define (core x c form top?)
  (let* ((items (items-of form))
         (keyword (id-name (car items)))
         (n (length items)))
    (case c
      ((quote)
       (if (= n 2) (vector 'const (%strip-datum (cadr items))) (syntax-error keyword items)))
      ((quasiquote)
       (if (= n 2) (quasi x (cadr items) 1) (syntax-error keyword items)))
      ((unquote unquote-splicing)
       (fail (string-append keyword ": not in a quasiquote: " (show form))))
      ((if) (if-form x items))
      ((define define-syntax)
       (cond ((not top?)
              (fail (string-append keyword ": only allowed at top level or at the start of a body: " (show form))))
             ((eq? c 'define) (define-form x items))
             (else (define-syntax-form x items))))
      ((set!) (set-form x items))
      ((begin) (begin-form x items top?))
      ((lambda)
       (if (>= n 3) (lambda-form x "" (cadr items) (cddr items)) (syntax-error keyword items)))
      ((let) (let-form x items))
      ((letrec letrec*) (letrec-form x keyword items))
      ((let-syntax) (let-syntax-form x keyword items #f))
      ((letrec-syntax) (let-syntax-form x keyword items #t))
      ((syntax-rules)
       (fail (string-append "syntax-rules: only allowed as a macro's transformer: " (show form))))
      ;; (syntax-error message irritant ...), from a macro's template: an
      ;; error when the macro use is expanded.
      ((syntax-error)
       (if (not (and (>= n 2) (string? (cadr items))))
           (syntax-error "syntax-error" items))
       (fail (apply string-append (cadr items)
                    (map (lambda (i) (string-append " " (show (%strip-datum i)))) (cddr items)))))
      ((import define-library)
       (cond ((not top?) (fail (string-append keyword ": only allowed at top level")))
             ((eq? c 'import) (import-form x items))
             (else (define-library-form x items))))
      ((include include-ci cond-expand)
       (let ((forms (spliced-forms x c items)))
         (if (null? forms) unspecified-node (sequence x forms top?))))
      ;; Auxiliary syntax: else, =>, _ and ...
      (else (fail (string-append keyword ": auxiliary syntax can't be used as an expression"))))))

(define (if-form x items)
  (let ((n (length items)))
    (if (not (or (= n 3) (= n 4))) (syntax-error "if" items))
    ;; The else branch first, as the Rust expander does.
    (let* ((els (if (= n 4) (expr x (cadddr items) #f) unspecified-node))
           (test (expr x (cadr items) #f))
           (then (expr x (caddr items) #f)))
      (vector 'if test then els))))

;; A define form's parts: (name procedure formals . body) or (name expr
;; . value), or #f if it is malformed.
(define (definition-parts items)
  (cond ((and (>= (length items) 3)
              (pair? (cadr items))
              (identifier? (car (cadr items))))
         (cons (car (cadr items)) (cons 'procedure (cons (cdr (cadr items)) (cddr items)))))
        ((and (= (length items) 3) (identifier? (cadr items)))
         (cons (cadr items) (cons 'expr (caddr items))))
        (else #f)))

(define (definition items)
  (or (definition-parts items) (syntax-error "define" items)))

;; A top-level (define name expr) or (define (name . formals) body ...).
(define (define-form x items)
  (let* ((d (definition items))
         ;; The cell first: if the name is an alias, only its own cell
         ;; makes recursive references inside the value find it.
         (cell (%env-define-cell (expander-env x) (car d)))
         (value (definition-value x (car d) (cdr d))))
    (vector 'global-define cell value)))

;; A top-level define-syntax. Its value, like define's, is the name.
(define (define-syntax-form x items)
  (if (not (and (= (length items) 3) (identifier? (cadr items))))
      (syntax-error "define-syntax" items))
  (let ((m (transformer x (caddr items) 0)))
    (%env-define-syntax (expander-env x) (cadr items) m)
    (vector 'const (strip (cadr items)))))

;; A macro from `spec`, which must be a syntax-rules form, defined in the
;; scope of the outermost `depth` frames.
(define (transformer x spec depth)
  (if (not (eq? (head-core x spec) 'syntax-rules))
      (fail (string-append "not a syntax-rules transformer: " (show spec))))
  (parse-syntax-rules spec depth (expander-env x)))

;; The value of a definition of `name`. A procedure gets `name` as its
;; name.
(define (definition-value x name def)
  (let ((text (show name)))
    (if (eq? (car def) 'procedure)
        (lambda-form x text (cadr def) (cddr def))
        (let ((value (expr x (cdr def) #f)))
          (if (and (eq? (node-tag value) 'lambda) (string=? (lambda-name value) ""))
              (vector-set! value 1 text))
          value))))

(define (set-form x items)
  (if (not (and (= (length items) 3) (identifier? (cadr items))))
      (syntax-error "set!" items))
  (let* ((name (cadr items))
         (value (expr x (caddr items) #f))
         (b (resolve x name)))
    (case (car b)
      ((local) (vector 'local-set (cdr b) value))
      ((global) (vector 'global-set (%env-set-cell (cddr b) (cadr b)) value))
      (else (fail (string-append "set!: can't assign to syntax: " (show name)))))))

(define (begin-form x items top?)
  (cond ((and (= (length items) 1) top?)
         ;; (begin) is only legal at top level.
         unspecified-node)
        ((= (length items) 1) (fail "begin: empty in an expression"))
        (else
         (if top?
             ;; Make the cells of the begin's definitions first, so a form
             ;; can refer to one defined later in the same begin.
             (for-each (lambda (form)
                         (if (eq? (head-core x form) 'define)
                             (let ((d (definition-parts (items-of form))))
                               (if (and d (%env-mutable? (expander-env x)))
                                   (%env-define-cell (expander-env x) (car d))))))
                       (cdr items)))
         (sequence x (cdr items) top?))))

;; One expression, or a seq.
(define (sequence x forms top?)
  (let ((body (map-in-order (lambda (f) (expr x f top?)) forms)))
    (cond ((null? body) (fail "empty body"))
          ((null? (cdr body)) (car body))
          (else (vector 'seq body)))))

;; A body, of a lambda, let, letrec, let-syntax or letrec-syntax: its
;; definitions get a new frame, and become a letrec around the
;; expressions.
(define (body x forms)
  (push-frame! x '())
  (let ((result (body-in-frame x forms)))
    (pop-frame! x)
    result))

;; Two passes: the first finds the definitions (expanding each form's head
;; until it knows whether it is one, splicing begins), and stops at the
;; first expression; the second expands the definitions' values and the
;; expressions, with every definition in scope.
(define (body-in-frame x forms)
  (let loop ((pending forms) (defs '()))
    (if (null? pending)
        (fail "body: no expression after the definitions")
        (let* ((form (expand-head x (car pending)))
               (c (head-core x form)))
          (case c
            ((begin) (loop (append (cdr (items-of form)) (cdr pending)) defs))
            ((include include-ci cond-expand)
             (loop (append (spliced-forms x c (items-of form)) (cdr pending)) defs))
            ((define)
             (let ((d (definition (items-of form))))
               (check-unbound-in-frame x (car d))
               (let ((var (fresh x (car d))))
                 (frame-bind! x (car d) (cons 'local (var-id var)))
                 (loop (cdr pending) (cons (cons var (cdr d)) defs)))))
            ((define-syntax)
             (let ((items (items-of form)))
               (if (not (and (= (length items) 3) (identifier? (cadr items))))
                   (syntax-error "define-syntax" items))
               (check-unbound-in-frame x (cadr items))
               ;; The body's whole frame is the macro's scope.
               (let ((m (transformer x (caddr items) (expander-nframes x))))
                 (frame-bind! x (cadr items) (cons 'macro m))
                 (loop (cdr pending) defs))))
            (else
             (let* ((bindings (map-in-order (lambda (d) (cons (car d) (definition-value x (var-name (car d)) (cdr d))))
                                            (reverse defs)))
                    (body (sequence x (cons form (cdr pending)) #f)))
               (if (null? bindings) body (vector 'letrec bindings body)))))))))

(define (check-unbound-in-frame x name)
  (if (assq name (vector-ref (car (expander-frames x)) 0))
      (fail (string-append "define: duplicate definition: " (show name)))))

;; (lambda formals body ...). `formals` is a list of identifiers, possibly
;; improper, or one identifier for all the arguments.
(define (lambda-form x name formals body-forms)
  (let* ((parts (list-parts formals))
         (params (map-in-order (lambda (item)
                                 (if (not (identifier? item))
                                     (fail (string-append "lambda: bad parameter: " (show item))))
                                 (fresh x item))
                               (car parts)))
         (rest (cond ((identifier? (cdr parts)) (fresh x (cdr parts)))
                     ((null? (cdr parts)) #f)
                     (else (fail (string-append "lambda: bad parameter list: " (show formals))))))
         (all (if rest (append params (list rest)) params)))
    (let check ((seen '()) (vars all))
      (if (pair? vars)
          (begin
            (if (memq (var-name (car vars)) seen)
                (fail (string-append "lambda: duplicate parameter: " (show (var-name (car vars))))))
            (check (cons (var-name (car vars)) seen) (cdr vars)))))
    (vector 'lambda name params rest (scoped x all (lambda () (body x body-forms))))))

;; (let ((v init) ...) body ...), or a named let.
(define (let-form x items)
  (cond ((< (length items) 3) (syntax-error "let" items))
        ((identifier? (cadr items)) (named-let x (cadr items) items))
        (else
         (let* ((specs (binding-specs (cadr items) "let"))
                ;; The inits are expanded outside the new scope, each
                ;; before its variable is numbered.
                (bindings (map-in-order (lambda (spec)
                                          (let ((init (expr x (cdr spec) #f)))
                                            (cons (fresh x (car spec)) init)))
                                        specs)))
           (vector 'let bindings (scoped x (map car bindings) (lambda () (body x (cddr items)))))))))

;; (let name ((v init) ...) body ...) is
;; ((letrec ((name (lambda (v ...) body ...))) name) init ...).
(define (named-let x name items)
  (if (< (length items) 4) (syntax-error "let" items))
  (let* ((specs (binding-specs (caddr items) "let"))
         (inits (map-in-order (lambda (spec) (expr x (cdr spec) #f)) specs))
         (formals (map car specs))
         (var (fresh x name))
         (proc (scoped x (list var)
                       (lambda ()
                         (let ((l (lambda-form x (show name) formals (cdddr items))))
                           (vector 'letrec (list (cons var l)) (vector 'local-ref (var-id var))))))))
    (vector 'call proc inits)))

;; letrec* and letrec, which R7RS allows to be the same.
(define (letrec-form x keyword items)
  (if (< (length items) 3) (syntax-error keyword items))
  (let* ((specs (binding-specs (cadr items) keyword))
         (vars (map-in-order (lambda (spec) (fresh x (car spec))) specs)))
    (scoped x vars
            (lambda ()
              (let* ((bindings (let loop ((specs specs) (vars vars) (out '()))
                                 (if (null? specs)
                                     (reverse out)
                                     (loop (cdr specs) (cdr vars)
                                           (cons (cons (car vars) (definition-value x (car (car specs)) (cons 'expr (cdr (car specs)))))
                                                 out)))))
                     (body (body x (cddr items))))
                (vector 'letrec bindings body))))))

;; let-syntax, or letrec-syntax (`rec?`), whose transformers are defined
;; inside the new frame.
(define (let-syntax-form x keyword items rec?)
  (if (< (length items) 3) (syntax-error keyword items))
  (let ((specs (binding-specs (cadr items) keyword))
        (depth (+ (expander-nframes x) (if rec? 1 0))))
    (push-frame! x '())
    (for-each (lambda (spec)
                (frame-bind! x (car spec) (cons 'macro (transformer x (cdr spec) depth))))
              specs)
    (let ((result (body x (cddr items))))
      (pop-frame! x)
      result)))

;; The ((v init) ...) of a let, letrec or let-syntax: ((name . init) ...).
(define (binding-specs specs who)
  (if (not (list? specs)) (fail (string-append who ": bad bindings: " (show specs))))
  (let loop ((specs specs) (out '()))
    (if (null? specs)
        (reverse out)
        (let ((spec (car specs)))
          (if (not (and (list? spec) (= (length spec) 2) (identifier? (car spec))))
              (fail (string-append who ": bad binding: " (show spec))))
          (if (assq (car spec) out)
              (fail (string-append who ": duplicate variable: " (show (car spec)))))
          (loop (cdr specs) (cons (cons (car spec) (cadr spec)) out))))))

(define (call x form)
  (let* ((items (items-of form))
         (f (expr x (car items) #f)))
    (vector 'call f (map-in-order (lambda (a) (expr x a #f)) (cdr items)))))

;;; Quasiquote: the template as calls to cons, append and list->vector.
;;; Parts with no unquote in them are constants.

(define (quasi x template depth)
  (if (not (has-unquote? x template))
      (vector 'const (%strip-datum template))
      (let ((u (unquote-form x template)))
        (if u
            (let ((c (car u)) (arg (cdr u)))
              (case c
                ((unquote) (if (= depth 1) (expr x arg #f) (tagged x "unquote" arg (- depth 1))))
                ((unquote-splicing)
                 (if (= depth 1)
                     (fail (string-append "unquote-splicing: not in a list: " (show template)))
                     (tagged x "unquote-splicing" arg (- depth 1))))
                (else (tagged x "quasiquote" arg (+ depth 1)))))
            (if (pair? template)
                (let ((spliced (and (= depth 1) (unquote-form x (car template)))))
                  (if (and spliced (eq? (car spliced) 'unquote-splicing))
                      (let* ((s (expr x (cdr spliced) #f))
                             (rest (quasi x (cdr template) depth)))
                        (global-call x "append" (list s rest)))
                      (let* ((a (quasi x (car template) depth))
                             (d (quasi x (cdr template) depth)))
                        (global-call x "cons" (list a d)))))
                ;; A vector.
                (global-call x "list->vector" (list (quasi x (vector->list template) depth))))))))

;; (tag arg) as data, with `arg` quasiquoted at `depth`.
(define (tagged x tag arg depth)
  (let* ((tag (vector 'const (string->symbol tag)))
         (inner (quasi x arg depth))
         (tail (global-call x "cons" (list inner (vector 'const '())))))
    (global-call x "cons" (list tag tail))))

;; `form` as (unquote e), (unquote-splicing e) or (quasiquote e): (keyword
;; . e), or #f.
(define (unquote-form x form)
  (let ((parts (list-parts form)))
    (and (null? (cdr parts))
         (= (length (car parts)) 2)
         (identifier? (car (car parts)))
         (let ((b (resolve x (car (car parts)))))
           (and (eq? (car b) 'core)
                (memq (cdr b) '(unquote unquote-splicing quasiquote))
                (cons (cdr b) (cadr (car parts))))))))

(define (has-unquote? x form)
  (cond ((pair? form)
         (let ((u (unquote-form x form)))
           (or (and u (memq (car u) '(unquote unquote-splicing)) #t)
               (has-unquote? x (car form))
               (has-unquote? x (cdr form)))))
        ((vector? form) (any? (lambda (item) (has-unquote? x item)) (vector->list form)))
        (else #f)))

;; A call to the system environment's procedure `name`, whatever the
;; environment being compiled for binds.
(define (global-call x name args)
  (let ((env (or (%system-environment) (expander-env x))))
    (vector 'call (vector 'global-ref (%env-cell env (string->symbol name))) args)))

;;; Libraries (libraries.scm)

;; The forms include, include-ci or cond-expand stand for.
(define (spliced-forms x c items)
  (if (eq? c 'cond-expand)
      (choose-cond-expand-clause (cdr items) "cond-expand")
      (include-forms (cdr items) (eq? c 'include-ci) (symbol->string c))))

;; (import import-set ...): bind the imports now, so the forms after it
;; are compiled with them. Every set is resolved before anything is bound.
(define (import-form x items)
  (if (not (%env-mutable? (expander-env x))) (fail "import: this environment is immutable"))
  (let ((all (apply append (map-in-order (lambda (set) (resolve-import-set set "import")) (cdr items)))))
    (for-each (lambda (b) (%env-import (expander-env x) (car b) (cdr b))) all)
    unspecified-node))

;; (define-library name declaration ...). The imports go into a new
;; environment and the body is expanded there now; the code ends with
;; %register-library, which checks the exports and registers the library.
(define (define-library-form x items)
  (let ((who "define-library"))
    (if (< (length items) 2) (fail (string-append who ": expects a library name")))
    (library-name (cadr items) who)
    (let ((decls (collect-declarations (cddr items)))
          (lib (%make-environment)))
      ;; Importing one name with two different bindings is an error.
      (let loop ((sets (declarations-imports decls)) (seen '()))
        (if (pair? sets)
            (let each ((bindings (resolve-import-set (car sets) who)) (seen seen))
              (if (null? bindings)
                  (loop (cdr sets) seen)
                  (let* ((b (car bindings)) (old (assq (car b) seen)))
                    (if (and old (not (same-import? (cdr old) (cdr b))))
                        (fail (string-append who ": " (show (car b)) " is imported with two different bindings")))
                    (%env-import lib (car b) (cdr b))
                    (each (cdr bindings) (cons b seen)))))))
      (let ((saved (expander-env x)))
        (set-expander-env! x lib)
        (let ((body (map-in-order (lambda (f) (expr x f #t)) (declarations-body decls))))
          (set-expander-env! x saved)
          (let ((exports (map (lambda (e) (cons (car e) (cdr e))) (declarations-exports decls))))
            (vector 'seq (append body (list (global-call x "%register-library"
                                                         (list (vector 'const (cadr items))
                                                               (vector 'const exports)
                                                               (vector 'const lib))))))))))))

(define (same-import? a b) (and (eq? (car a) (car b)) (eq? (cdr a) (cdr b))))
