;;; Libraries at compile time (R7RS 5.2, 5.6), as src/libraries.rs does
;;; them: library names, import sets, define-library's declarations,
;;; include and cond-expand. The registry and the search path stay Rust
;;; (%library-exports, %library-file).
;;;
;;; A library name is a list of strings, ("scheme" "base"). Import sets
;;; resolve to ((name . binding) ...).

;; Parse a library name datum such as (scheme base).
(define (library-name datum who)
  (let ((parts (if (list? datum) datum '())))
    (let loop ((parts parts) (names '()))
      (cond ((null? parts)
             (if (null? names)
                 (fail (string-append who ": not a library name: " (show datum)))
                 (reverse names)))
            ((identifier? (car parts)) (loop (cdr parts) (cons (id-name (car parts)) names)))
            ((and (exact-integer? (car parts)) (<= 0 (car parts) 140737488355327))
             (loop (cdr parts) (cons (number->string (car parts)) names)))
            (else (fail (string-append who ": not a library name: " (show datum))))))))

(define (format-library-name names) (string-append "(" (join names " ") ")"))

;; The exports of the library `names`, loading its file first if it has
;; one that hasn't been loaded.
(define (find-library names who)
  (or (%library-exports names)
      (let ((path (%library-file names)))
        (cond ((not path)
               (fail (string-append who ": unknown library " (format-library-name names))))
              ((%library-file-loaded? path)
               (fail (string-append who ": " path " does not define " (format-library-name names))))
              (else (%load-library path)
                    (find-library names who))))))

;; The (name . binding) pairs an import set denotes: a library name, or
;; only, except, prefix or rename applied to an import set. Identifiers
;; are compared by name, so an import made by a macro works.
(define (resolve-import-set set who)
  (if (not (list? set)) (fail (string-append who ": not an import set: " (show set))))
  (let ((keyword (and (pair? set) (identifier? (car set)) (id-name (car set)))))
    (if (not (and (member keyword '("only" "except" "prefix" "rename")) (>= (length set) 2)))
        (find-library (library-name set who) who)
        (let ((bindings (resolve-import-set (cadr set) who))
              (args (cddr set)))
          (define (name-of id) (if (identifier? id) (id-name id) ""))
          (define (position bindings id)
            (let ((name (name-of id)))
              (let loop ((bs bindings) (i 0))
                (cond ((null? bs)
                       (fail (string-append who ": " name " is not in the import set " (show (cadr set)))))
                      ((string=? (id-name (car (car bs))) name) i)
                      (else (loop (cdr bs) (+ i 1)))))))
          (define (replace-name bindings i name)
            (let loop ((bs bindings) (j 0))
              (cond ((null? bs) '())
                    ((= j i) (cons (cons (string->symbol name) (cdr (car bs))) (cdr bs)))
                    (else (cons (car bs) (loop (cdr bs) (+ j 1)))))))
          (cond
           ((string=? keyword "only")
            (map-in-order (lambda (id) (list-ref bindings (position bindings id))) args))
           ((string=? keyword "except")
            (let loop ((args args) (bindings bindings))
              (if (null? args)
                  bindings
                  (let ((i (position bindings (car args))))
                    (loop (cdr args) (append (take bindings i) (list-tail bindings (+ i 1))))))))
           ((string=? keyword "prefix")
            (if (not (= (length args) 1))
                (fail (string-append who ": prefix takes an import set and one identifier")))
            (let ((prefix (name-of (car args))))
              (map (lambda (b) (cons (string->symbol (string-append prefix (id-name (car b)))) (cdr b)))
                   bindings)))
           (else
            (let loop ((args args) (bindings bindings))
              (if (null? args)
                  bindings
                  (let ((ids (if (list? (car args)) (car args) '())))
                    (if (not (= (length ids) 2))
                        (fail (string-append who ": rename expects (from to) pairs")))
                    (let ((i (position bindings (car ids))))
                      (loop (cdr args) (replace-name bindings i (name-of (cadr ids))))))))))))))

;;; define-library declarations

(define-record-type declarations
  (make-declarations exports imports body)
  declarations?
  ;; ((internal . external) ...)
  (exports declarations-exports set-declarations-exports!)
  ;; The import sets, in order.
  (imports declarations-imports set-declarations-imports!)
  ;; The body forms, from begin and include declarations, in order.
  (body declarations-body set-declarations-body!))

;; What a define-library's declarations add up to, expanding
;; include-library-declarations and cond-expand. Declarations are
;; recognised by name: they aren't bindings anywhere.
(define (collect-declarations decls)
  (let ((out (make-declarations '() '() '()))
        (who "define-library"))
    (define (add-exports! items) (set-declarations-exports! out (append (declarations-exports out) items)))
    (define (add-imports! items) (set-declarations-imports! out (append (declarations-imports out) items)))
    (define (add-body! items) (set-declarations-body! out (append (declarations-body out) items)))
    (let collect ((decls decls))
      (for-each
       (lambda (decl)
         (if (not (list? decl)) (fail (string-append who ": not a declaration: " (show decl))))
         (let ((keyword (if (and (pair? decl) (identifier? (car decl))) (id-name (car decl)) ""))
               (args (if (pair? decl) (cdr decl) '())))
           (cond
            ((string=? keyword "export")
             (for-each (lambda (spec)
                         (cond ((identifier? spec) (add-exports! (list (cons spec spec))))
                               ((and (list? spec) (= (length spec) 3) (identifier? (car spec))
                                     (string=? (id-name (car spec)) "rename"))
                                (add-exports! (list (cons (cadr spec) (caddr spec)))))
                               (else (fail (string-append who ": bad export spec " (show spec))))))
                       args))
            ((string=? keyword "import") (add-imports! args))
            ((string=? keyword "begin") (add-body! args))
            ((or (string=? keyword "include") (string=? keyword "include-ci"))
             (add-body! (include-forms args (string=? keyword "include-ci") keyword)))
            ((string=? keyword "include-library-declarations")
             (collect (include-forms args #f keyword)))
            ((string=? keyword "cond-expand")
             (collect (choose-cond-expand-clause args who)))
            (else (fail (string-append who ": unknown declaration " (show decl)))))))
       decls))
    out))

;; The forms of the files named by `files`, strings, in order: what
;; include and include-ci splice in.
(define (include-forms files fold-case? who)
  (if (null? files) (fail (string-append who ": expects at least one file name")))
  (let loop ((files files) (forms '()))
    (cond ((null? files) forms)
          ((string? (car files)) (loop (cdr files) (append forms (%read-forms (car files) fold-case? who))))
          (else (fail (string-append who ": file names must be strings"))))))

;;; cond-expand

;; The body of the first clause whose requirement holds (an else clause,
;; last, always does), or nothing.
(define (choose-cond-expand-clause clauses who)
  (let ((features (map symbol->string (features))))
    (let loop ((clauses clauses))
      (if (null? clauses)
          '()
          (let ((parts (if (list? (car clauses)) (car clauses) '())))
            (if (null? parts) (fail (string-append who ": bad clause " (show (car clauses)))))
            (let ((holds (if (and (identifier? (car parts)) (string=? (id-name (car parts)) "else"))
                             (if (pair? (cdr clauses))
                                 (fail (string-append who ": else must be the last clause"))
                                 #t)
                             (requirement-holds? (car parts) features who))))
              (if holds (cdr parts) (loop (cdr clauses)))))))))

;; A feature identifier, (library name), or and, or, not of requirements.
(define (requirement-holds? req features who)
  (define (bad) (fail (string-append who ": bad feature requirement " (show req))))
  (if (identifier? req)
      (and (member (id-name req) features) #t)
      (let ((parts (if (and (list? req) (pair? req)) req (bad))))
        (let ((head (and (identifier? (car parts)) (id-name (car parts))))
              (args (cdr parts)))
          (cond ((equal? head "and")
                 (let loop ((args args))
                   (or (null? args) (and (requirement-holds? (car args) features who) (loop (cdr args))))))
                ((equal? head "or")
                 (let loop ((args args))
                   (and (pair? args) (or (requirement-holds? (car args) features who) (loop (cdr args))))))
                ((equal? head "not")
                 (if (= (length args) 1) (not (requirement-holds? (car args) features who)) (bad)))
                ((equal? head "library")
                 (if (= (length args) 1)
                     (let ((names (library-name (car args) who)))
                       (and (or (%library-exports names) (%library-file names)) #t))
                     (bad)))
                (else (bad)))))))
