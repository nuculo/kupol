(ns kan-kat.operator-kan
  "KAN как динамическая операторная система.
   
   ═══════════════════════════════════════════════════
   ИДЕЯ: ОБУЧАЕМ ПРАВИЛА, А НЕ ЧИСЛА
   ═══════════════════════════════════════════════════
   
   Обычный ML: φ(x) = число. Оператор: φ = правило.
   
   Оператор A: f → g, где f и g — функции.
   Пример: D(sin) = cos  (дифференцирование)
           X(sin) = x·sin (умножение на x)
   
   KAN-слой = линейная комбинация операторов:
     L = α₁·D + α₂·X + α₃·D² + α₄·Id
   Обучаем α₁,α₂,α₃,α₄ — коэффициенты в алгебре!
   
   Коммутатор [A,B] = AB - BA (алгебраическая структура).
   Каноническое [D,X] = Id (как в квантовой механике).
   
   Clojure: оператор = fn (fn → fn) = first-class.
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]))

;; ============================================================
;; ОПЕРАТОР = тройка (transform, name, arity)
;; ============================================================

(defn make-operator
  "Создаёт оператор: преобразование f → g."
  [transform-fn op-name]
  {:transform transform-fn
   :name      op-name})

;; ============================================================
;; ЭЛЕМЕНТАРНЫЕ ОПЕРАТОРЫ
;; ============================================================

(defn numerical-deriv
  "Числовая производная f'(x)."
  [f x]
  (let [eps 1e-6]
    (/ (- (f (+ x eps)) (f (- x eps))) (* 2.0 eps))))

;; Id: f → f
(def id-op
  (make-operator
    (fn [f] f)
    "Id"))

;; D: f → f' (дифференцирование)
(def D-op
  (make-operator
    (fn [f] (fn [x] (numerical-deriv f x)))
    "D"))

;; D²: f → f'' (второе дифференцирование)
(def D2-op
  (make-operator
    (fn [f] (fn [x]
              (let [eps 1e-4]
                (/ (- (f (+ x eps)) (* 2.0 (f x)) (- (f (- x eps))))
                   (* eps eps)))))
    "D²"))

;; X: f → x·f (умножение на x)
(def X-op
  (make-operator
    (fn [f] (fn [x] (* x (f x))))
    "X·"))

;; S (shift): f(x) → f(x+ε) (сдвиг аргумента)
(defn shift-op [eps]
  (make-operator
    (fn [f] (fn [x] (f (+ x eps))))
    (format "S(%.2f)" eps)))

;; Sq: f → f² (возведение в квадрат)
(def sq-op
  (make-operator
    (fn [f] (fn [x] (let [v (f x)] (* v v))))
    "f²"))

;; Neg: f → -f (отрицание)
(def neg-op
  (make-operator
    (fn [f] (fn [x] (- (f x))))
    "-f"))

;; Compose: f → sin∘f
(def sin-compose-op
  (make-operator
    (fn [f] (fn [x] (math/sin (f x))))
    "sin∘"))

;; ============================================================
;; АЛГЕБРА ОПЕРАТОРОВ
;; ============================================================

(defn op-compose
  "Композиция: (A∘B)(f) = A(B(f))."
  [a b]
  (make-operator
    (fn [f] ((:transform a) ((:transform b) f)))
    (str (:name a) "∘" (:name b))))

(defn op-add
  "Сумма: (A+B)(f) = A(f) + B(f)."
  [a b]
  (make-operator
    (fn [f]
      (let [af ((:transform a) f)
            bf ((:transform b) f)]
        (fn [x] (+ (af x) (bf x)))))
    (str (:name a) "+" (:name b))))

(defn op-scale
  "Масштабирование: (c·A)(f) = c·A(f)."
  [c a]
  (make-operator
    (fn [f]
      (let [af ((:transform a) f)]
        (fn [x] (* c (af x)))))
    (format "%.2f·%s" c (:name a))))

;; ============================================================
;; КОММУТАТОР
;; ============================================================

(defn commutator
  "Коммутатор [A,B] = AB - BA.
   Вычисляет [A,B](f)(x) для конкретных f и x."
  [a b f x]
  (let [ab ((:transform a) ((:transform b) f))
        ba ((:transform b) ((:transform a) f))]
    (- (ab x) (ba x))))

;; ============================================================
;; ЛИНЕЙНАЯ КОМБИНАЦИЯ (KAN OPERATOR LAYER)
;; ============================================================

(defn linear-combination
  "Линейная комбинация операторов:
   L(f) = Σ αᵢ · Aᵢ(f)"
  [ops coeffs]
  (make-operator
    (fn [f]
      (let [transformed (mapv (fn [op] ((:transform op) f)) ops)]
        (fn [x]
          (reduce + 0.0
            (map (fn [tf c] (* c (tf x)))
                 transformed coeffs)))))
    (str "Σ[" (clojure.string/join "+" 
                (map (fn [op c] (format "%.2f·%s" c (:name op)))
                     ops coeffs)) "]")))

;; ============================================================
;; ОБУЧЕНИЕ ОПЕРАТОРНОГО СЛОЯ
;; ============================================================

(defn train-operator-layer
  "Обучает коэффициенты α в операторной комбинации.
   
   input-fn  — входная функция (напр. sin)
   target-fn — целевая функция (напр. cos = D(sin))
   ops       — базис операторов [A₁, A₂, ...]
   domain    — [lo, hi]
   n-points  — точек для оценки
   epochs    — число эпох
   lr        — learning rate"
  [input-fn target-fn ops domain n-points epochs lr]
  (let [xs (mapv (fn [i]
                   (+ (first domain)
                      (* (/ i (dec n-points))
                         (- (second domain) (first domain)))))
                 (range n-points))
        target-vals (mapv target-fn xs)]
    (println "  ╔══════════════════════════════════════╗")
    (println "  ║  Operator Algebra KAN                ║")
    (println "  ╚══════════════════════════════════════╝")
    (println (format "  Operators: %s" (pr-str (mapv :name ops))))
    (println (format "  Input: f(x) → Target: g(x) | Points: %d" n-points))
    (println "  ────────────────────────────────────────")
    
    (loop [coeffs (vec (repeat (count ops) 0.0))
           epoch 1]
      (if (> epoch epochs)
        (do
          (println "  ────────────────────────────────────────")
          (println (format "  Discovered rule:"))
          (doseq [[op c] (map vector ops coeffs)]
            (when (> (abs c) 0.01)
              (println (format "    %+.4f · %s" c (:name op)))))
          {:coeffs coeffs :ops ops})
        (let [;; Forward: L(f)(x) для каждой точки
              L (linear-combination ops coeffs)
              Lf ((:transform L) input-fn)
              preds (mapv Lf xs)
              ;; MSE
              mse (/ (reduce + 0.0
                       (map (fn [p t] (let [e (- p t)] (* e e)))
                            preds target-vals))
                     n-points)
              ;; Градиенты по коэффициентам
              grads
              (mapv (fn [op-idx]
                      (let [eps 1e-4
                            c+ (assoc coeffs op-idx (+ (nth coeffs op-idx) eps))
                            c- (assoc coeffs op-idx (- (nth coeffs op-idx) eps))
                            L+ (linear-combination ops c+)
                            L- (linear-combination ops c-)
                            p+ (mapv ((:transform L+) input-fn) xs)
                            p- (mapv ((:transform L-) input-fn) xs)
                            mse+ (/ (reduce + 0.0 (map (fn [p t] (let [e (- p t)] (* e e)))
                                                       p+ target-vals)) n-points)
                            mse- (/ (reduce + 0.0 (map (fn [p t] (let [e (- p t)] (* e e)))
                                                       p- target-vals)) n-points)]
                        (/ (- mse+ mse-) (* 2.0 eps))))
                    (range (count ops)))
              ;; SGD
              new-coeffs (mapv (fn [c g] (- c (* lr (max -5.0 (min 5.0 g)))))
                               coeffs grads)]
          (when (zero? (mod epoch (max 1 (quot epochs 5))))
            (println (format "  Epoch %4d | MSE: %.8f | coeffs: %s"
                             epoch mse
                             (pr-str (mapv #(format "%.4f" %) coeffs)))))
          (recur new-coeffs (inc epoch)))))))

;; ============================================================
;; ВЕРИФИКАЦИЯ КОММУТАТОРОВ
;; ============================================================

(defn verify-commutators
  "Проверяет каноническое [D,X] = Id."
  []
  (println "\n  Verifying commutator relations:")
  (println "  ────────────────────────────────────────")
  ;; [D,X](f)(x) = f(x) для любых f,x
  (let [test-fn math/sin
        test-x  1.0
        ;; [D,X](sin)(1.0) should = sin(1.0) = 0.841...
        dx (commutator D-op X-op test-fn test-x)
        expected (test-fn test-x)
        err (abs (- dx expected))]
    (println (format "  [D,X](sin)(1.0) = %.6f" dx))
    (println (format "  Expected sin(1.0) = %.6f" expected))
    (println (format "  Error: %.2e %s" err
                     (if (< err 1e-4) "✅ [D,X]=Id verified" "⚠️")))
    err))
