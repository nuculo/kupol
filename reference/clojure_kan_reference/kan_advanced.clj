(ns kan-kat.kan-advanced
  "KAN Advanced: PhiFunction Protocol + Grid Refinement в Framework.
   
   ═══════════════════════════════════════════════════
   Фаза 34: Подключение PhiFunction Protocol к training framework
   
   Каждое ребро = PhiFunction (BSpline / Poly / Rational)
   Grid Refinement: удвоение сетки B-spline во время обучения
   Батчевый forward через scalar protocol + numerical grad
   ═══════════════════════════════════════════════════"
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.phi-protocol :as phi]
            [kan-kat.spline :as spl]
            [clojure.math :as math]))

;; ============================================================
;; LAYER: PhiFunction-based KAN Layer
;; ============================================================

(defn make-phi-layer
  "Создаёт KAN-слой с PhiFunction на каждом ребре.
   phi-type = :bspline | :poly | :rational
   phi-opts = {:order 3 :grid-size 5 :grid-range [-2 2]} и т.п."
  [in-dim out-dim phi-type phi-opts]
  (let [phis (vec (for [_j (range out-dim)]
                    (vec (for [_i (range in-dim)]
                           (phi/make-phi phi-type phi-opts)))))]
    {:in-dim   in-dim
     :out-dim  out-dim
     :phi-type phi-type
     :phi-opts phi-opts
     :phis     phis}))    ; phis[j][i] = PhiFunction

(defn phi-layer-forward
  "Forward pass: x [batch × in-dim] → out [batch × out-dim].
   Использует PhiFunction protocol для каждого ребра."
  [layer x-batch]
  (let [{:keys [in-dim out-dim phis]} layer
        batch (first (:shape x-batch))
        ^doubles xd (:data x-batch)
        out-data (double-array (* batch out-dim))]
    ;; Вычисляем каждый выход
    (dotimes [b batch]
      (dotimes [j out-dim]
        (let [sum (loop [i 0 acc 0.0]
                    (if (= i in-dim)
                      acc
                      (let [xi (aget xd (+ (* b in-dim) i))
                            yi (phi/phi-forward (get-in phis [j i]) xi)]
                        (recur (inc i) (+ acc yi)))))]
          (aset out-data (+ (* b out-dim) j) sum))))
    (t2/tensor (vec out-data) [batch out-dim])))

(defn phi-layer-params
  "Извлекает все параметры слоя (flat vector of doubles)."
  [layer]
  (vec (for [j (range (:out-dim layer))
             i (range (:in-dim layer))]
         (phi/phi-params (get-in (:phis layer) [j i])))))

(defn phi-layer-update
  "Обновляет параметры слоя из flat list of param-vectors."
  [layer new-params-list]
  (let [idx (atom 0)]
    (update layer :phis
            (fn [phis]
              (mapv (fn [row]
                      (mapv (fn [phi-fn]
                              (let [p (new-params-list @idx)]
                                (swap! idx inc)
                                (phi/phi-update phi-fn p)))
                            row))
                    phis)))))

;; ============================================================
;; NUMERICAL GRADIENT для PhiFunction Layer
;; ============================================================

(defn phi-layer-grad
  "Вычисляет gradient по параметрам через numerical differentiation.
   Возвращает list of param-gradient-vectors."
  [layer x-batch y-batch]
  (let [{:keys [in-dim out-dim phis]} layer
        batch (first (:shape x-batch))
        ^doubles xd (:data x-batch)
        ^doubles yd (:data y-batch)
        eps 1e-5]
    ;; Для каждого ребра (j, i): grad по params
    (vec
     (for [j (range out-dim)
           i (range in-dim)]
       (let [phi-fn (get-in phis [j i])
             params (phi/phi-params phi-fn)
             n-params (count params)]
         ;; Численный градиент для каждого параметра
         (vec
          (for [p-idx (range n-params)]
            (let [;; f(params + eps·e_p)
                  params+ (assoc params p-idx (+ (nth params p-idx) eps))
                  phi+ (phi/phi-update phi-fn params+)
                  ;; f(params - eps·e_p)
                  params- (assoc params p-idx (- (nth params p-idx) eps))
                  phi- (phi/phi-update phi-fn params-)
                  ;; MSE с params+ и params-
                  loss+ (loop [b 0 acc 0.0]
                          (if (= b batch) (/ acc batch)
                            (let [pred (loop [ii 0 s 0.0]
                                         (if (= ii in-dim) s
                                           (let [xi (aget xd (+ (* b in-dim) ii))
                                                 phi-ii (if (and (= ii i) (= j 0))
                                                          phi+ (get-in phis [j ii]))]
                                             (recur (inc ii) (+ s (phi/phi-forward phi-ii xi))))))
                                  err (- pred (aget yd b))]
                              (recur (inc b) (+ acc (* err err))))))
                  loss- (loop [b 0 acc 0.0]
                          (if (= b batch) (/ acc batch)
                            (let [pred (loop [ii 0 s 0.0]
                                         (if (= ii in-dim) s
                                           (let [xi (aget xd (+ (* b in-dim) ii))
                                                 phi-ii (if (and (= ii i) (= j 0))
                                                          phi- (get-in phis [j ii]))]
                                             (recur (inc ii) (+ s (phi/phi-forward phi-ii xi))))))
                                  err (- pred (aget yd b))]
                              (recur (inc b) (+ acc (* err err))))))]
              (/ (- loss+ loss-) (* 2.0 eps))))))))))

;; ============================================================
;; SGD для PhiFunction Layer
;; ============================================================

(defn phi-sgd-step
  "SGD step для PhiFunction layer. Возвращает обновлённый layer."
  [layer grads lr max-grad]
  (let [all-params (phi-layer-params layer)
        new-params (mapv (fn [params grad]
                           (mapv (fn [p g]
                                   (- p (* lr (max (- max-grad) (min max-grad g)))))
                                 params grad))
                         all-params grads)]
    (phi-layer-update layer new-params)))

;; ============================================================
;; GRID REFINEMENT
;; ============================================================

(defn refine-grid
  "Удваивает сетку B-spline для всех рёбер слоя.
   knots: [t₀...tₙ] → новые midpoints добавляются.
   coeffs: interpolation на новую сетку."
  [layer]
  (if (not= (:phi-type layer) :bspline)
    (do (println "    Grid refinement: skip (not bspline)")
        layer)
    (let [{:keys [in-dim out-dim phis phi-opts]} layer
          old-gs (get phi-opts :grid-size 5)
          new-gs (* 2 old-gs)
          new-opts (assoc phi-opts :grid-size new-gs)]
      (println (format "    Grid refinement: %d → %d nodes" old-gs new-gs))
      (let [new-phis
            (mapv (fn [row]
                    (mapv (fn [old-phi]
                            (let [old-knots (:knots old-phi)
                                  old-coeffs (:coeffs old-phi)
                                  order (:order old-phi)
                                  new-knots (spl/init-knots order new-gs
                                              (:grid-range phi-opts))
                                  n-new-coeffs (- (count new-knots) order 1)
                                  ;; Interpolate: sample old phi at new grid points
                                  new-coeffs
                                  (vec (for [ci (range n-new-coeffs)]
                                         (let [x-sample (nth new-knots (+ ci (quot order 2)))
                                               ;; Evaluate old spline at sample
                                               basis (spl/eval-splines-at order old-knots x-sample)
                                               val (reduce + 0.0 (map * old-coeffs basis))]
                                           val)))]
                              (phi/->BSplinePhi new-coeffs new-knots order
                                                (:wb old-phi) (:ws old-phi))))
                          row))
                  phis)]
        (assoc layer :phis new-phis :phi-opts new-opts)))))

;; ============================================================
;; TRAINING LOOP для PhiFunction Layer
;; ============================================================

(defn train-phi-layer
  "Обучение одного PhiFunction-слоя: SGD с numerical gradients.
   Поддерживает grid refinement каждые refine-every эпох."
  [layer data epochs lr & [{:keys [print-every refine-every max-grad]
                             :or {print-every 10 refine-every 0 max-grad 5.0}}]]
  (let [{:keys [x y]} data]
    (loop [l layer ep 0 history []]
      (if (= ep epochs) [l history]
        (let [;; Grid refinement
              l2 (if (and (pos? refine-every)
                          (pos? ep)
                          (zero? (mod ep refine-every)))
                   (refine-grid l)
                   l)
              ;; Forward + loss
              pred (phi-layer-forward l2 x)
              pred-flat (vec (:data pred))
              ^doubles yd (:data y)
              batch (first (:shape x))
              loss (/ (reduce + (map (fn [i]
                                       (let [e (- (nth pred-flat i) (aget yd i))]
                                         (* e e)))
                                     (range batch)))
                      batch)
              ;; Gradient
              grads (phi-layer-grad l2 x y)
              ;; SGD
              l3 (phi-sgd-step l2 grads lr max-grad)]
          (when (zero? (mod (inc ep) print-every))
            (println (format "    Epoch %3d | Loss: %.6f" (inc ep) loss)))
          (recur l3 (inc ep) (conj history loss)))))))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-kan-advanced []
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  KAN Advanced: PhiProtocol + Grid    ║")
  (println "  ╚══════════════════════════════════════╝")

  ;; === Part 1: PolyPhi Layer → sin(x) ===
  (println "\n  Part 1: PolyPhi [1→1] → sin(x)")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        layer (make-phi-layer 1 1 :poly {:degree 5})
        [trained history] (train-phi-layer layer
                            {:x (t2/tensor xs [n 1]) :y (t2/tensor ys [n])}
                            100 0.01 {:print-every 25})]
    (println (format "    Final loss: %.6f (started: %.6f)" (last history) (first history)))
    (println (format "    Improvement: %.1f%%" (* 100 (- 1 (/ (last history) (first history)))))))

  ;; === Part 2: BSplinePhi Layer → sin(x) ===
  (println "\n  Part 2: BSplinePhi [1→1] → sin(x)")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        layer (make-phi-layer 1 1 :bspline {:order 3 :grid-size 5 :grid-range [-2.0 2.0]})
        [trained history] (train-phi-layer layer
                            {:x (t2/tensor xs [n 1]) :y (t2/tensor ys [n])}
                            100 0.01 {:print-every 25})]
    (println (format "    Final loss: %.6f (started: %.6f)" (last history) (first history)))
    (println (format "    BSpline params: %d (wb+ws+coeffs)"
                     (count (phi/phi-params (get-in (:phis trained) [0 0]))))))

  ;; === Part 3: Grid Refinement ===
  (println "\n  Part 3: BSpline + Grid Refinement (5→10→20)")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        layer (make-phi-layer 1 1 :bspline {:order 3 :grid-size 5 :grid-range [-2.0 2.0]})
        [trained history] (train-phi-layer layer
                            {:x (t2/tensor xs [n 1]) :y (t2/tensor ys [n])}
                            150 0.01
                            {:print-every 50 :refine-every 50})]
    (println (format "    Final loss: %.6f (started: %.6f)" (last history) (first history)))
    (println (format "    Improvement: %.1f%%" (* 100 (- 1 (/ (last history) (first history)))))))

  ;; === Part 4: RationalPhi ===
  (println "\n  Part 4: RationalPhi [1→1] → sin(x)")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        layer (make-phi-layer 1 1 :rational {:p-deg 4 :q-deg 3})
        [trained history] (train-phi-layer layer
                            {:x (t2/tensor xs [n 1]) :y (t2/tensor ys [n])}
                            100 0.005 {:print-every 25})]
    (println (format "    Final loss: %.6f (started: %.6f)" (last history) (first history)))
    (println (format "    Improvement: %.1f%%" (* 100 (- 1 (/ (last history) (first history)))))))

  ;; === Part 5: Summary ===
  (println "\n  Part 5: φ type comparison")
  (println "    PolyPhi:     degree=5, 6 params, fast")
  (println "    BSplinePhi:  order=3, grid=5, ~10 params, local control")
  (println "    RationalPhi: p=4/q=3, 9 params, good for poles")
  (println "    Grid Refine: 5→10→20 nodes, improving approximation"))
