(ns kan-kat.symbolic
  "Символическая регрессия для KAN.
   
   ═══════════════════════════════════════════════════
   ИДЕЯ (из KAN paper, Liu et al. 2024)
   ═══════════════════════════════════════════════════
   
   После обучения KAN каждая φ_{j,i} — это B-spline,
   приближающий некоторую простую функцию.
   
   Мы пробуем *библиотеку символических кандидатов*:
     sin(x), cos(x), x², x³, |x|, exp(x), log(|x|+1),
     σ(x), tanh(x), identity(x), constant
   
   Для каждого ребра: fit кандидатов через LSQ → выбираем
   с минимальной ошибкой → заменяем B-spline на формулу.
   
   Это ключевая фича KAN = **интерпретируемость**.
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.kan-layer :as kan]
            [kan-kat.spline :as spl]))

;; ============================================================
;; БИБЛИОТЕКА СИМВОЛИЧЕСКИХ КАНДИДАТОВ
;; ============================================================

(def candidate-fns
  "Библиотека кандидатных функций: [имя, f(x), f'(x)]."
  [[:identity  (fn [x] x)              (fn [x] 1.0)]
   [:square    (fn [x] (* x x))        (fn [x] (* 2.0 x))]
   [:cube      (fn [x] (* x x x))      (fn [x] (* 3.0 x x))]
   [:abs       (fn [x] (abs x))        (fn [x] (if (pos? x) 1.0 -1.0))]
   [:sin       (fn [x] (math/sin x))   (fn [x] (math/cos x))]
   [:cos       (fn [x] (math/cos x))   (fn [x] (- (math/sin x)))]
   [:exp       (fn [x] (math/exp (min x 5.0)))
                                        (fn [x] (math/exp (min x 5.0)))]
   [:log1p     (fn [x] (math/log (+ (abs x) 1.0)))
                                        (fn [x] (/ 1.0 (+ (abs x) 1.0)))]
   [:sigmoid   (fn [x] (/ 1.0 (+ 1.0 (math/exp (- x)))))
                                        (fn [x] (let [s (/ 1.0 (+ 1.0 (math/exp (- x))))]
                                                   (* s (- 1.0 s))))]
   [:tanh      (fn [x] (math/tanh x))  (fn [x] (- 1.0 (let [t (math/tanh x)] (* t t))))]
   [:silu      (fn [x] (let [s (/ 1.0 (+ 1.0 (math/exp (- x))))]
                          (* x s)))
                                        (fn [x] (let [s (/ 1.0 (+ 1.0 (math/exp (- x))))]
                                                   (+ s (* x s (- 1.0 s)))))]])

;; ============================================================
;; ОЦЕНКА φ НА СЕТКЕ ТОЧЕК
;; ============================================================

(defn eval-phi-on-grid
  "Вычислить обученную φ_{j,i} на равномерной сетке.
   
   Структура слоя:
     :spline-weights [out][in][coeffs]
     :base-weights   [out][in]
     :spline-scales  [out][in]
     :grids          [in] → knots vector
   
   Возвращает [[x1 y1] [x2 y2] ...]."
  [layer j i n-points]
  (let [knots  (nth (:grids layer) i)
        order  (:spline-order layer)
        coeffs (get-in (:spline-weights layer) [j i])
        wb     (get-in (:base-weights layer) [j i])
        ws     (get-in (:spline-scales layer) [j i])
        x-min  (+ (first knots) 0.01)
        x-max  (- (last knots) 0.01)
        step   (/ (- x-max x-min) (dec n-points))]
    (mapv (fn [idx]
            (let [x  (+ x-min (* idx step))
                  ;; SiLU base
                  sig (/ 1.0 (+ 1.0 (math/exp (- x))))
                  base (* x sig)
                  ;; B-spline
                  basis (spl/eval-splines-at order knots x)
                  spline (reduce + 0.0 (map * coeffs basis))
                  y  (+ (* wb base) (* ws spline))]
              [x y]))
          (range n-points))))

;; ============================================================
;; ЛИНЕЙНАЯ РЕГРЕССИЯ: y ≈ a·f(x) + b
;; ============================================================

(defn fit-affine
  "Fit y ≈ a·f(x) + b через least squares (closed form).
   
   Возвращает {:a, :b, :mse}."
  [xy-pairs f]
  (let [n  (count xy-pairs)
        ;; Собираем f(x_i) и y_i
        fxs (mapv (fn [[x _]] (f x)) xy-pairs)
        ys  (mapv second xy-pairs)
        ;; Least squares: [a b] = (A^T A)^-1 A^T y
        sum-f  (reduce + 0.0 fxs)
        sum-y  (reduce + 0.0 ys)
        sum-ff (reduce + 0.0 (map * fxs fxs))
        sum-fy (reduce + 0.0 (map * fxs ys))
        ;; 2x2 normal equations
        det   (- (* sum-ff n) (* sum-f sum-f))
        a     (if (< (abs det) 1e-12) 0.0
                (/ (- (* n sum-fy) (* sum-f sum-y)) det))
        b     (if (< (abs det) 1e-12) (/ sum-y n)
                (/ (- (* sum-ff sum-y) (* sum-f sum-fy)) det))
        ;; MSE
        mse   (/ (reduce + 0.0
                   (map (fn [[xi yi]]
                          (let [pred (+ (* a (f xi)) b)]
                            (* (- yi pred) (- yi pred))))
                        xy-pairs))
                 n)]
    {:a a :b b :mse mse}))

;; ============================================================
;; СИМВОЛИЧЕСКАЯ РЕГРЕССИЯ ОДНОГО РЕБРА
;; ============================================================

(defn symbolify-edge
  "Анализирует обученную φ_{j,i} и находит лучшую символическую формулу.
   
   Возвращает {:name, :a, :b, :mse, :formula-str}."
  [layer j i & [{:keys [n-points]
                 :or   {n-points 50}}]]
  (let [xy-data (eval-phi-on-grid layer j i n-points)
        results (mapv (fn [[name f _f']]
                        (let [fit (fit-affine xy-data f)]
                          (assoc fit :name name)))
                      candidate-fns)
        best    (apply min-key :mse results)
        formula (cond
                  (< (abs (:a best)) 1e-4) (format "%.4f" (:b best))
                  (< (abs (:b best)) 1e-4) (format "%.3f·%s(x)" (:a best) (name (:name best)))
                  :else (format "%.3f·%s(x) + %.3f" (:a best) (name (:name best)) (:b best)))]
    (assoc best :formula-str formula)))

;; ============================================================
;; СИМВОЛИЧЕСКАЯ РЕГРЕССИЯ ВСЕГО СЛОЯ
;; ============================================================

(defn symbolify-layer
  "Анализирует все φ_{j,i} слоя и выводит символические формулы."
  [layer & [opts]]
  (let [n-in  (:in-features layer)
        n-out (:out-features layer)]
    (println "╔══════════════════════════════════════════╗")
    (println "║  Symbolic Regression: φ → формулы        ║")
    (println "╚══════════════════════════════════════════╝")
    (println (format "  Layer [%d → %d] | Edges: %d\n" n-in n-out (* n-in n-out)))
    (println "  Edge    | Best fit        | a       | b       | MSE")
    (println "  --------|-----------------|---------|---------|--------")
    (let [results
          (vec (for [j (range n-out)
                     i (range n-in)]
                 (let [r (symbolify-edge layer j i opts)]
                   (println (format "  φ(%d,%d)  | %-15s | %7.4f | %7.4f | %.2e"
                                    j i (name (:name r)) (:a r) (:b r) (:mse r)))
                   (assoc r :j j :i i))))]
      (println)
      (println "  Discovered formulas:")
      (doseq [r results]
        (println (format "    φ(%d,%d) ≈ %s" (:j r) (:i r) (:formula-str r))))
      (println)
      results)))
