(ns kan-kat.kan-symbolic
  "Symbolic Discovery в KAN Framework.
   
   ═══════════════════════════════════════════════════
   Фаза 35: Автоматическое обнаружение символических
   формул во время обучения.
   
   Каждые K эпох:
     1. probe-edge: пробует кандидаты (sin, cos, x², ...)
     2. confidence = 1 - MSE/variance → [0, 1]
     3. confidence > threshold → freeze ребро
     4. Замороженное ребро = exact formula, 0 параметров
   
   Результат: обученная KAN → человекочитаемая формула
   ═══════════════════════════════════════════════════"
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.phi-protocol :as phi]
            [kan-kat.kan-advanced :as adv]
            [clojure.math :as math]))

;; ============================================================
;; БИБЛИОТЕКА КАНДИДАТОВ
;; ============================================================

(def symbolic-candidates
  "Библиотека символических кандидатов: [name f(x)]."
  [[:identity  (fn [x] x)]
   [:square    (fn [x] (* x x))]
   [:cube      (fn [x] (* x x x))]
   [:sin       (fn [x] (math/sin x))]
   [:cos       (fn [x] (math/cos x))]
   [:exp       (fn [x] (math/exp (min x 5.0)))]
   [:log1p     (fn [x] (math/log (+ (abs x) 1.0)))]
   [:sigmoid   (fn [x] (/ 1.0 (+ 1.0 (math/exp (- x)))))]
   [:tanh      (fn [x] (math/tanh x))]
   [:abs       (fn [x] (abs x))]
   [:silu      (fn [x] (* x (/ 1.0 (+ 1.0 (math/exp (- x))))))]])

;; ============================================================
;; PROBING: оценка кандидатов для одного ребра
;; ============================================================

(defn fit-affine
  "Fit y ≈ a·f(x) + b через normal equations (closed form)."
  [xs ys f]
  (let [n  (count xs)
        fxs (mapv f xs)
        sum-f  (reduce + 0.0 fxs)
        sum-y  (reduce + 0.0 ys)
        sum-ff (reduce + 0.0 (map * fxs fxs))
        sum-fy (reduce + 0.0 (map * fxs ys))
        det   (- (* sum-ff n) (* sum-f sum-f))
        a     (if (< (abs det) 1e-12) 0.0
                (/ (- (* n sum-fy) (* sum-f sum-y)) det))
        b     (if (< (abs det) 1e-12) (/ sum-y n)
                (/ (- (* sum-ff sum-y) (* sum-f sum-fy)) det))
        mse   (/ (reduce + 0.0
                   (map (fn [xi yi]
                          (let [pred (+ (* a (f xi)) b)]
                            (* (- yi pred) (- yi pred))))
                        xs ys))
                 n)]
    {:a a :b b :mse mse}))

(defn probe-edge
  "Пробует все символические кандидаты для одного ребра.
   phi = PhiFunction, test-points = [x₁ x₂ ...].
   Возвращает best match: {:name :a :b :mse :confidence :formula}."
  [phi-fn test-points]
  (let [xs test-points
        ys (mapv #(phi/phi-forward phi-fn %) xs)
        ;; Variance of ys (для confidence)
        y-mean (/ (reduce + ys) (count ys))
        y-var (/ (reduce + (map (fn [y] (* (- y y-mean) (- y y-mean))) ys))
                 (count ys))
        ;; Fit каждый кандидат
        results (mapv (fn [[cand-name cand-f]]
                        (let [fit (fit-affine xs ys cand-f)]
                          (assoc fit :name cand-name)))
                      symbolic-candidates)
        best (apply min-key :mse results)
        ;; Confidence = 1 - MSE/Var (R²-like)
        confidence (if (< y-var 1e-10) 1.0
                     (max 0.0 (- 1.0 (/ (:mse best) y-var))))
        ;; Human-readable formula
        formula (cond
                  (< (abs (:a best)) 1e-4) (format "%.4f" (:b best))
                  (< (abs (:b best)) 1e-4) (format "%.3f·%s(x)" (:a best) (name (:name best)))
                  :else (format "%.3f·%s(x) + %.3f" (:a best) (name (:name best)) (:b best)))]
    (assoc best :confidence confidence :formula formula)))

;; ============================================================
;; FROZEN PHI: замороженная функция (exact formula)
;; ============================================================

(defrecord FrozenPhi [name f a b formula]
  phi/PhiFunction
  (phi-forward [this x]
    (+ (* a (f x)) b))
  (phi-backward [this x]
    ;; Numerical derivative (frozen, не обучается)
    (let [eps 1e-5]
      (/ (- (phi/phi-forward this (+ x eps))
            (phi/phi-forward this (- x eps)))
         (* 2.0 eps))))
  (phi-grad [this x]
    ;; Нет обучаемых параметров!
    [])
  (phi-params [this]
    ;; Frozen = 0 обучаемых параметров
    [])
  (phi-update [this new-params]
    ;; Ничего не обновляем
    this))

(defn make-frozen-phi
  "Создаёт замороженную φ из символического результата."
  [{:keys [name a b formula]}]
  (let [f (second (first (filter #(= (first %) name) symbolic-candidates)))]
    (->FrozenPhi name f a b formula)))

;; ============================================================
;; AUTO-DISCOVER: probing + freeze для всего слоя
;; ============================================================

(defn probe-layer
  "Пробует символические кандидаты для всех рёбер слоя.
   Возвращает results: [[{:name :confidence :formula ...}]]."
  [layer test-points]
  (let [{:keys [in-dim out-dim phis]} layer]
    (vec (for [j (range out-dim)]
           (vec (for [i (range in-dim)]
                  (probe-edge (get-in phis [j i]) test-points)))))))

(defn freeze-layer
  "Замораживает рёбра с confidence > threshold.
   Возвращает [updated-layer n-frozen]."
  [layer probe-results threshold]
  (let [{:keys [in-dim out-dim phis]} layer
        frozen-count (atom 0)
        new-phis (vec
                  (for [j (range out-dim)]
                    (vec
                     (for [i (range in-dim)]
                       (let [result (get-in probe-results [j i])
                             conf (:confidence result)]
                         (if (>= conf threshold)
                           (do (swap! frozen-count inc)
                               (make-frozen-phi result))
                           (get-in phis [j i])))))))]
    [(assoc layer :phis new-phis) @frozen-count]))

(defn count-trainable
  "Считает количество обучаемых параметров."
  [layer]
  (reduce + (map (fn [row]
                   (reduce + (map #(count (phi/phi-params %)) row)))
                 (:phis layer))))

;; ============================================================
;; TRAINING С SYMBOLIC DISCOVERY
;; ============================================================

(defn train-with-discovery
  "Обучение с автоматическим symbolic discovery.
   Каждые probe-every эпох:
     1. Probe все рёбра
     2. Freeze с confidence > threshold
     3. Продолжить обучение
   
   opts: {:probe-every 50, :threshold 0.90, :print-every 10,
          :test-range [-2 2], :n-test-points 30}"
  [layer data epochs lr
   & [{:keys [probe-every threshold print-every test-range n-test-points max-grad]
       :or {probe-every 50 threshold 0.90 print-every 10
            test-range [-2.0 2.0] n-test-points 30 max-grad 5.0}}]]
  (let [test-points (vec (for [i (range n-test-points)]
                           (+ (first test-range)
                              (* (/ i (dec n-test-points))
                                 (- (second test-range) (first test-range))))))]
    (loop [l layer ep 0 history [] discoveries []]
      (if (= ep epochs) [l history discoveries]
        (let [;; Probing phase
              [l2 disc2]
              (if (and (pos? probe-every)
                       (pos? ep)
                       (zero? (mod ep probe-every)))
                (let [results (probe-layer l test-points)
                      [frozen n-frozen] (freeze-layer l results threshold)]
                  (when (pos? n-frozen)
                    (println (format "    >>> Discovered %d symbolic edges! Trainable: %d → %d"
                                     n-frozen
                                     (count-trainable l)
                                     (count-trainable frozen))))
                  ;; Print discoveries
                  (doseq [j (range (:out-dim l))
                          i (range (:in-dim l))]
                    (let [r (get-in results [j i])]
                      (when (>= (:confidence r) threshold)
                        (println (format "        φ(%d,%d) ≈ %s (conf=%.3f)"
                                         j i (:formula r) (:confidence r))))))
                  [frozen (conj discoveries {:epoch ep
                                             :results results
                                             :n-frozen n-frozen})])
                [l discoveries])
              ;; Training step (skip frozen edges — they have 0 params)
              pred (adv/phi-layer-forward l2 (:x data))
              pred-flat (vec (:data pred))
              ^doubles yd (:data (:y data))
              batch (first (:shape (:x data)))
              loss (/ (reduce + (map (fn [idx]
                                       (let [e (- (nth pred-flat idx) (aget yd idx))]
                                         (* e e)))
                                     (range batch)))
                      batch)
              ;; Gradient + SGD (only for non-frozen)
              grads (adv/phi-layer-grad l2 (:x data) (:y data))
              l3 (adv/phi-sgd-step l2 grads lr max-grad)]
          (when (zero? (mod (inc ep) print-every))
            (println (format "    Epoch %3d | Loss: %.6f | Trainable: %d"
                             (inc ep) loss (count-trainable l3))))
          (recur l3 (inc ep) (conj history loss) disc2))))))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-kan-symbolic []
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  Symbolic Discovery in Framework     ║")
  (println "  ╚══════════════════════════════════════╝")

  ;; === Part 1: Probing sin(x) edge ===
  (println "\n  Part 1: Probe trained φ → sin(x)")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        ;; Обучаем PolyPhi и пробуем кандидаты
        layer (adv/make-phi-layer 1 1 :poly {:degree 5})
        [trained _] (adv/train-phi-layer layer
                      {:x (t2/tensor xs [n 1]) :y (t2/tensor ys [n])}
                      80 0.01 {:print-every 200})
        phi-fn (get-in (:phis trained) [0 0])
        test-pts (vec (for [i (range 30)] (- (* 4.0 (/ i 29)) 2.0)))
        result (probe-edge phi-fn test-pts)]
    (println (format "    Best match: %s" (name (:name result))))
    (println (format "    Formula:    %s" (:formula result)))
    (println (format "    Confidence: %.4f" (:confidence result)))
    (println (format "    MSE:        %.6f" (:mse result))))

  ;; === Part 2: Auto-discovery during training ===
  (println "\n  Part 2: Auto-discovery [1→1] sin(x), probe every 40 ep")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        layer (adv/make-phi-layer 1 1 :poly {:degree 5})
        [trained history disc] (train-with-discovery layer
                                 {:x (t2/tensor xs [n 1]) :y (t2/tensor ys [n])}
                                 120 0.01
                                 {:probe-every 40 :threshold 0.85 :print-every 30})]
    (println (format "    Final loss: %.6f" (last history)))
    (println (format "    Discoveries: %d" (count disc)))
    (println (format "    Trainable params: %d" (count-trainable trained))))

  ;; === Part 3: Multi-edge discovery ===
  (println "\n  Part 3: [2→1] f(x₁,x₂)=sin(x₁)+x₂²")
  (let [n 40
        xs (vec (for [_ (range n)] [(- (* 4.0 (rand)) 2.0)
                                     (- (* 4.0 (rand)) 2.0)]))
        ys (mapv (fn [[x1 x2]] (+ (math/sin x1) (* x2 x2))) xs)
        layer (adv/make-phi-layer 2 1 :poly {:degree 5})
        [trained history disc] (train-with-discovery layer
                                 {:x (t2/tensor (flatten xs) [n 2])
                                  :y (t2/tensor ys [n])}
                                 200 0.005
                                 {:probe-every 60 :threshold 0.80
                                  :print-every 50})]
    (println (format "    Final loss: %.6f" (last history)))
    (println (format "    Discoveries: %d" (count disc)))
    ;; Print final edge formulas
    (let [test-pts (vec (for [i (range 30)] (- (* 4.0 (/ i 29)) 2.0)))]
      (doseq [i (range 2)]
        (let [r (probe-edge (get-in (:phis trained) [0 i]) test-pts)]
          (println (format "    φ(0,%d) ≈ %s (conf=%.3f)" i (:formula r) (:confidence r)))))))

  ;; === Part 4: Frozen edge demo ===
  (println "\n  Part 4: FrozenPhi — exact formula, 0 params")
  (let [frozen (make-frozen-phi {:name :sin :a 1.0 :b 0.0
                                  :formula "1.000·sin(x)"})]
    (println (format "    FrozenPhi(0.0) = %.6f  (expected: 0.000000)" (phi/phi-forward frozen 0.0)))
    (println (format "    FrozenPhi(π/2) = %.6f  (expected: 1.000000)" (phi/phi-forward frozen (/ Math/PI 2))))
    (println (format "    FrozenPhi(π)   = %.6f  (expected: 0.000000)" (phi/phi-forward frozen Math/PI)))
    (println (format "    Params: %d (frozen = 0 trainable)" (count (phi/phi-params frozen)))))

  ;; === Part 5: Summary ===
  (println "\n  Part 5: Symbolic Discovery summary")
  (println "    Candidates: 11 (sin, cos, x², x³, exp, ...)")
  (println "    Probing:    fit-affine + confidence = 1 - MSE/var")
  (println "    Freeze:     FrozenPhi = exact formula, 0 params")
  (println "    Training:   auto-probe every K epochs, freeze > threshold"))
