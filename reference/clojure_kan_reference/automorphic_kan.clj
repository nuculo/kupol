(ns kan-kat.automorphic-kan
  "Автоморфная KAN — Normalizing Flow на KAN-функциях.
   
   ═══════════════════════════════════════════════════
   ИДЕЯ
   ═══════════════════════════════════════════════════
   
   Normalizing Flow: цепочка обратимых преобразований
   z ~ N(0,I) → x = f(z), где f = f_L ∘ ... ∘ f_1
   
   Каждый f_i — coupling layer на KAN φ-функциях:
   - Вход разбивается на [x_a, x_b]
   - x_a копируется: y_a = x_a
   - x_b трансформируется: y_b = x_b · exp(s(x_a)) + t(x_a)
   - где s,t — KAN φ-функции (обучаемые)
   
   Обратимость:
   - x_b = (y_b - t(y_a)) · exp(-s(y_a))
   
   Log-det Jacobian = Σ s(x_a) — для density estimation.
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.phi-protocol :as phi]))

;; ============================================================
;; COUPLING LAYER (обратимый слой)
;; ============================================================

(defn make-coupling-layer
  "Создаёт coupling layer с KAN φ-функциями для s(x) и t(x).
   
   split-idx — где разбивать вход: [0..split-idx-1] = a, [split-idx..] = b
   phi-type  — тип φ для s и t
   opts      — опции для make-phi"
  [dim split-idx phi-type opts]
  (let [n-a split-idx
        n-b (- dim split-idx)]
    {:dim       dim
     :split-idx split-idx
     :n-a       n-a
     :n-b       n-b
     ;; s(x_a) → scaling factors (одна φ на каждый x_b)
     :s-phis    (mapv (fn [_] (phi/make-phi phi-type opts)) (range n-b))
     ;; t(x_a) → translation factors
     :t-phis    (mapv (fn [_] (phi/make-phi phi-type opts)) (range n-b))}))

;; ============================================================
;; FORWARD (z → x)
;; ============================================================

(defn coupling-forward
  "Forward через coupling layer.
   Возвращает [y, log-det-jacobian]."
  [layer x]
  (let [{:keys [split-idx s-phis t-phis n-a]} layer
        x-a (subvec x 0 split-idx)
        x-b (subvec x split-idx)
        ;; s и t зависят от x_a (суммируем по всем x_a для каждого выхода)
        s-vals (mapv (fn [s-phi]
                       (reduce + 0.0
                         (map (fn [xa] (phi/phi-forward s-phi xa))
                              x-a)))
                     s-phis)
        t-vals (mapv (fn [t-phi]
                       (reduce + 0.0
                         (map (fn [xa] (phi/phi-forward t-phi xa))
                              x-a)))
                     t-phis)
        ;; y_b = x_b · exp(s) + t
        y-b (mapv (fn [xb si ti]
                    (+ (* xb (math/exp si)) ti))
                  x-b s-vals t-vals)
        ;; log|det J| = Σ s_i
        log-det (reduce + 0.0 s-vals)]
    [(vec (concat x-a y-b)) log-det]))

;; ============================================================
;; INVERSE (x → z)
;; ============================================================

(defn coupling-inverse
  "Inverse через coupling layer (точная обратимость!)."
  [layer y]
  (let [{:keys [split-idx s-phis t-phis]} layer
        y-a (subvec y 0 split-idx)
        y-b (subvec y split-idx)
        ;; s и t от y_a (= x_a, потому что a-часть не меняется)
        s-vals (mapv (fn [s-phi]
                       (reduce + 0.0
                         (map (fn [ya] (phi/phi-forward s-phi ya))
                              y-a)))
                     s-phis)
        t-vals (mapv (fn [t-phi]
                       (reduce + 0.0
                         (map (fn [ya] (phi/phi-forward t-phi ya))
                              y-a)))
                     t-phis)
        ;; x_b = (y_b - t) · exp(-s)
        x-b (mapv (fn [yb si ti]
                    (* (- yb ti) (math/exp (- si))))
                  y-b s-vals t-vals)]
    (vec (concat y-a x-b))))

;; ============================================================
;; NORMALIZING FLOW (цепочка coupling layers)
;; ============================================================

(defn make-flow
  "Создаёт Normalizing Flow из чередующихся coupling layers.
   
   dim       — размерность
   n-layers  — число coupling layers
   phi-type  — тип φ
   opts      — опции для make-phi"
  [dim n-layers phi-type opts]
  {:dim    dim
   :layers (mapv (fn [l-idx]
                   ;; Чередуем split: первая половина / вторая половина
                   (let [split (if (even? l-idx)
                                 (quot dim 2)
                                 (- dim (quot dim 2)))]
                     (make-coupling-layer dim split phi-type opts)))
                 (range n-layers))})

(defn flow-forward
  "Forward через весь flow: z → x.
   Возвращает [x, total-log-det]."
  [flow z]
  (reduce (fn [[x log-det] layer]
            (let [[y ld] (coupling-forward layer x)]
              [y (+ log-det ld)]))
          [z 0.0]
          (:layers flow)))

(defn flow-inverse
  "Inverse через весь flow: x → z."
  [flow x]
  (reduce (fn [y layer]
            (coupling-inverse layer y))
          x
          (reverse (:layers flow))))

;; ============================================================
;; GAUSSIAN LOG-PROBABILITY
;; ============================================================

(defn gaussian-log-prob
  "Log-probability under standard Gaussian N(0,I)."
  [z]
  (let [d (count z)
        log2pi (math/log (* 2.0 Math/PI))]
    (- (* -0.5 d log2pi)
       (* 0.5 (reduce + 0.0 (map #(* % %) z))))))

;; ============================================================
;; DENSITY ESTIMATION (log p(x))
;; ============================================================

(defn log-prob
  "Log-probability of x under the flow.
   log p(x) = log p_z(f^{-1}(x)) + log|det J_{f^{-1}}|
   
   Для coupling layers: log|det J^{-1}| = -log|det J|."
  [flow x]
  (let [z (flow-inverse flow x)
        ;; Forward для log-det
        [_ log-det] (flow-forward flow z)]
    (- (gaussian-log-prob z) log-det)))

;; ============================================================
;; ОБУЧЕНИЕ (maximum likelihood)
;; ============================================================

(defn train-flow
  "Обучение flow через maximum likelihood.
   data   — вектор точек [[x1 x2 ...] ...]
   epochs — число эпох
   lr     — learning rate
   
   Минимизируем -E[log p(x)] = -E[log p_z(z) + log|det J|]."
  [flow data epochs lr]
  (loop [f flow epoch 1]
    (if (> epoch epochs)
      f
      (let [;; Числовые градиенты по параметрам каждого слоя
            new-layers
            (mapv (fn [layer l-idx]
                    ;; Обновляем s-phis и t-phis
                    (let [update-phis
                          (fn [phis kind]
                            (mapv (fn [a-phi b-idx]
                                    (let [params (phi/phi-params a-phi)
                                          eps    1e-4
                                          grads
                                          (mapv (fn [p-idx]
                                                  (let [p+ (assoc params p-idx (+ (nth params p-idx) eps))
                                                        p- (assoc params p-idx (- (nth params p-idx) eps))
                                                        l+ (assoc-in f [:layers l-idx kind b-idx]
                                                                     (phi/phi-update a-phi p+))
                                                        l- (assoc-in f [:layers l-idx kind b-idx]
                                                                     (phi/phi-update a-phi p-))
                                                        ;; Average log-prob over data
                                                        lp+ (/ (reduce + 0.0 (map #(log-prob l+ %) data))
                                                               (count data))
                                                        lp- (/ (reduce + 0.0 (map #(log-prob l- %) data))
                                                               (count data))]
                                                    (/ (- lp+ lp-) (* 2.0 eps))))
                                                (range (count params)))
                                          ;; Gradient ASCENT (maximize log-prob)
                                          new-params (mapv (fn [p g] (+ p (* lr g))) params grads)]
                                      (phi/phi-update a-phi new-params)))
                                  phis (range)))]
                      (-> layer
                          (assoc :s-phis (update-phis (:s-phis layer) :s-phis))
                          (assoc :t-phis (update-phis (:t-phis layer) :t-phis)))))
                  (:layers f) (range))
            new-f (assoc f :layers new-layers)
            avg-lp (/ (reduce + 0.0 (map #(log-prob new-f %) data))
                      (count data))]
        (when (zero? (mod epoch 5))
          (println (format "  Epoch %3d | Avg log p(x): %.4f" epoch avg-lp)))
        (recur new-f (inc epoch))))))

;; ============================================================
;; SAMPLING
;; ============================================================

(defn sample
  "Генерация сэмпла: z ~ N(0,I), x = flow(z)."
  [flow]
  (let [dim (:dim flow)
        ;; Box-Muller для Gaussian samples
        z (vec (for [_ (range dim)]
                 (let [u1 (max 1e-10 (rand))
                       u2 (rand)]
                   (* (math/sqrt (* -2.0 (math/log u1)))
                      (math/cos (* 2.0 Math/PI u2))))))]
    (first (flow-forward flow z))))

(defn sample-n
  "Генерация n сэмплов."
  [flow n]
  (mapv (fn [_] (sample flow)) (range n)))
