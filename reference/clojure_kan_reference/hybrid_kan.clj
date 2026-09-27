(ns kan-kat.hybrid-kan
  "Symbolic + Numeric Hybrid KAN.
   
   ═══════════════════════════════════════════════════
   ИДЕЯ: СКУЛЬПТОР С РЕНТГЕНОМ
   ═══════════════════════════════════════════════════
   
   Каждая φ живёт в ДВУХ мирах одновременно:
   
   1. ЧИСЛОВОЙ: обычный forward/backward через autograd
   2. СИМВОЛЬНЫЙ: '(sin x) — s-expression, код = данные
   
   На каждом шаге:
   - Числовой путь вычисляет φ(x)
   - Символьные кандидаты проверяются на совпадение
   - Confidence растёт при совпадении, падает при расхождении
   - При confidence > 0.95 → ЗАМОРАЖИВАНИЕ: φ = символ
   
   Результат: KAN, который сам себя объясняет.
   
   Clojure homoiconicity:
     '(sin x) — это И код, И данные → можно eval, simplify, compose
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.autograd :as ag]))

;; ============================================================
;; СИМВОЛЬНЫЕ КАНДИДАТЫ
;; ============================================================

(def symbolic-library
  "Библиотека символьных функций: имя → [s-expr, eval-fn]."
  {:sin      {:expr '(sin x)
              :fn   math/sin}
   :cos      {:expr '(cos x)
              :fn   math/cos}
   :x²       {:expr '(* x x)
              :fn   #(* % %)}
   :x³       {:expr '(* x x x)
              :fn   #(* % % %)}
   :tanh     {:expr '(tanh x)
              :fn   math/tanh}
   :sigmoid  {:expr '(/ 1 (+ 1 (exp (- x))))
              :fn   (fn [x] (/ 1.0 (+ 1.0 (math/exp (- x)))))}
   :identity {:expr 'x
              :fn   identity}
   :abs      {:expr '(abs x)
              :fn   #(abs %)}
   :exp      {:expr '(exp x)
              :fn   #(math/exp (min % 10.0))}
   :sqrt-abs {:expr '(sqrt (abs x))
              :fn   #(math/sqrt (abs %))}})

;; ============================================================
;; HYBRID PHI
;; ============================================================

(defn make-hybrid-phi
  "Создаёт гибридную φ: числовая + символьная.
   
   coeffs — начальные autograd Value для полинома
   degree — степень полинома"
  [degree]
  {:numeric-coeffs (mapv (fn [_] (ag/value (* 0.1 (- (rand) 0.5))))
                         (range (inc degree)))
   :candidates     (into {} (map (fn [[k v]]
                                   [k {:confidence 0.0
                                       :a 1.0    ; affine: y ≈ a·f(x) + b
                                       :b 0.0
                                       :mse 1.0}])
                                 symbolic-library))
   :frozen?        false
   :frozen-symbol  nil
   :frozen-a       1.0
   :frozen-b       0.0})

;; ============================================================
;; NUMERIC FORWARD (через autograd)
;; ============================================================

(defn numeric-forward
  "Числовой forward: poly через computation graph."
  [coeffs x-val]
  (ag/poly-forward coeffs (ag/value x-val)))

;; ============================================================
;; SYMBOLIC PROBE (проверка кандидатов)
;; ============================================================

(defn fit-affine-fast
  "Быстрый LSQ: y ≈ a·f(x)+b для набора точек."
  [f-vals y-vals]
  (let [n   (count f-vals)
        sf  (reduce + 0.0 f-vals)
        sy  (reduce + 0.0 y-vals)
        sff (reduce + 0.0 (map #(* % %) f-vals))
        sfy (reduce + 0.0 (map * f-vals y-vals))
        det (- (* n sff) (* sf sf))]
    (if (< (abs det) 1e-10)
      {:a 0.0 :b (/ sy (max 1 n)) :mse 1.0}
      (let [a   (/ (- (* n sfy) (* sf sy)) det)
            b   (/ (- (* sff sy) (* sf sfy)) det)
            mse (/ (reduce + 0.0
                     (map (fn [fi yi]
                            (let [err (- yi (+ (* a fi) b))]
                              (* err err)))
                          f-vals y-vals))
                   (max 1 n))]
        {:a a :b b :mse mse}))))

(defn probe-candidates
  "Проверяет символьных кандидатов на текущих данных.
   Сравнивает с ЦЕЛЕВЫМИ значениями y (не с числовым выходом)."
  [phi data-points _numeric-outputs]
  (let [;; data-points: [[xs-vec y] ...]
        xs (map (fn [[xv _]]
                  (if (vector? xv) (first xv) xv))
                data-points)
        ys (mapv (fn [[_ y]] (double y)) data-points)]
    (update phi :candidates
            (fn [candidates]
              (into {}
                (map (fn [[sym-name sym-info]]
                       (let [sym-fn (get-in symbolic-library [sym-name :fn])
                             f-vals (mapv sym-fn xs)
                             {:keys [a b mse]} (fit-affine-fast f-vals ys)
                             ;; Confidence: exp(-mse/σ²), momentum-averaged
                             raw-conf (math/exp (- (/ mse 0.05)))
                             old-conf (:confidence sym-info)
                             new-conf (+ (* 0.6 old-conf) (* 0.4 raw-conf))]
                         [sym-name (assoc sym-info
                                         :confidence new-conf
                                         :a a
                                         :b b
                                         :mse mse)]))
                     candidates))))))

;; ============================================================
;; HYBRID FORWARD
;; ============================================================

(defn hybrid-forward
  "Гибридный forward: символьный если заморожен, числовой иначе."
  [phi x-val]
  (if (:frozen? phi)
    ;; Символьный путь: ТОЧНЫЙ, 0 обучаемых параметров
    (let [sym-fn (get-in symbolic-library [(:frozen-symbol phi) :fn])
          result (+ (* (:frozen-a phi) (sym-fn x-val))
                    (:frozen-b phi))]
      (ag/value result))
    ;; Числовой путь: через autograd
    (numeric-forward (:numeric-coeffs phi) x-val)))

;; ============================================================
;; FREEZE CHECK
;; ============================================================

(defn check-freeze
  "Проверяет: пора ли замораживать φ?"
  [phi threshold]
  (if (:frozen? phi)
    phi
    (let [best (apply max-key #(:confidence (val %)) (:candidates phi))
          [sym-name sym-info] best]
      (if (> (:confidence sym-info) threshold)
        (assoc phi
               :frozen? true
               :frozen-symbol sym-name
               :frozen-a (:a sym-info)
               :frozen-b (:b sym-info))
        phi))))

;; ============================================================
;; HYBRID LAYER: набор гибридных φ
;; ============================================================

(defn make-hybrid-layer
  "Слой гибридных φ: in-features × out-features."
  [in-features out-features degree]
  {:in  in-features
   :out out-features
   :phis (vec (for [_ (range out-features)]
                (vec (for [_ (range in-features)]
                       (make-hybrid-phi degree)))))})

(defn hybrid-layer-forward
  "Forward через гибридный слой."
  [layer x-vals]
  (mapv (fn [row]
          (reduce ag/ag-add
                  (ag/value 0.0)
                  (map (fn [phi xi]
                         (hybrid-forward phi xi))
                       row x-vals)))
        (:phis layer)))

;; ============================================================
;; TRAINING LOOP
;; ============================================================

(defn train-hybrid
  "Обучает гибридный KAN.
   
   target-fn — целевая f(x₁, x₂, ...)
   domain    — [lo hi]
   n-points  — точек данных
   epochs    — эпох
   lr        — learning rate
   probe-every — как часто проверять символьных кандидатов"
  [target-fn in-dim domain n-points degree epochs lr probe-every]
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  Hybrid KAN: symbolic + numeric      ║")
  (println "  ╚══════════════════════════════════════╝")
  (println (format "  in=%d | degree=%d | points=%d | probe every %d epochs"
                   in-dim degree n-points probe-every))
  (println "  ────────────────────────────────────────")
  
  ;; Данные
  (let [data (mapv (fn [_]
                     (let [xs (mapv (fn [_]
                                     (+ (first domain)
                                        (* (rand) (- (second domain) (first domain)))))
                                   (range in-dim))
                           y  (apply target-fn xs)]
                       [xs y]))
                   (range n-points))]
    (loop [layer (make-hybrid-layer in-dim 1 degree)
           epoch 1]
      (if (> epoch epochs)
        (do
          (println "  ────────────────────────────────────────")
          ;; Report frozen
          (doseq [j (range (:out layer))]
            (doseq [i (range (:in layer))]
              (let [phi (get-in layer [:phis j i])]
                (if (:frozen? phi)
                  (let [sym (:frozen-symbol phi)
                        expr (get-in symbolic-library [sym :expr])]
                    (println (format "  φ(%d,%d) FROZEN: %.3f·%s + %.3f"
                                     j i (:frozen-a phi) expr (:frozen-b phi))))
                  (let [best (apply max-key #(:confidence (val %)) (:candidates phi))
                        [sym-name sym-info] best]
                    (println (format "  φ(%d,%d) numeric (best candidate: %s, conf=%.2f)"
                                     j i (name sym-name) (:confidence sym-info))))))))
          layer)
        (let [;; Forward all data points
              outputs (mapv (fn [[xs _]]
                              (first (hybrid-layer-forward layer xs)))
                            data)
              targets (mapv (fn [[_ y]] (ag/value y)) data)
              loss    (ag/mse-loss outputs targets)
              ;; Backward
              _       (ag/backward! loss)
              ;; Update numeric coeffs for unfrozen phis
              new-phis
              (mapv (fn [row]
                      (mapv (fn [phi]
                              (if (:frozen? phi)
                                phi
                                (let [new-c (ag/sgd-step! (:numeric-coeffs phi) lr
                                                          {:max-grad 5.0})]
                                  (assoc phi :numeric-coeffs new-c))))
                            row))
                    (:phis layer))
              new-layer (assoc layer :phis new-phis)
              ;; Probe symbolic candidates?
              new-layer (if (zero? (mod epoch probe-every))
                          (assoc new-layer :phis
                                 (mapv (fn [row]
                                         (mapv (fn [phi]
                                                 (if (:frozen? phi)
                                                   phi
                                                   (-> phi
                                                       (probe-candidates data outputs)
                                                       (check-freeze 0.90))))
                                               row))
                                       (:phis new-layer)))
                          new-layer)
              ;; Count frozen
              n-frozen (count (filter :frozen?
                                (apply concat (:phis new-layer))))]
          (when (zero? (mod epoch (max 1 (quot epochs 6))))
            (println (format "  Epoch %4d | Loss: %.6f | Frozen: %d/%d"
                             epoch (:data loss) n-frozen
                             (* (:in layer) (:out layer)))))
          (recur new-layer (inc epoch)))))))
