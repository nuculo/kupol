(ns kan-kat.kan-framework
  "Multi-Layer KAN Training Framework.
   
   ═══════════════════════════════════════════════════
   Объединяет всё: tensor_v2 + autograd + jvm_backend
   в полноценный фреймворк для обучения KAN-сетей.
   
   KAN Layer: y_j = Σ_i φ_{j,i}(x_i)
   где φ_{j,i}(x) = polynomial (Horner scheme)
   
   Multi-layer: Input → KAN₁ → KAN₂ → ... → Output
   Autograd: forward → backward → SGD/Adam
   ═══════════════════════════════════════════════════"
  (:require [kan-kat.tensor-v2 :as t2]
            [clojure.math :as math])
  (:import [java.util Arrays]))

;; ============================================================
;; LAYER: KAN Layer на tensor_v2
;; ============================================================

(defn make-kan-layer
  "Создаёт KAN-слой: in-dim → out-dim, degree = степень полинома.
   Каждое ребро φ_{j,i} — полином степени `degree`.
   Параметры: (out-dim × in-dim × (degree+1)) coefficients."
  [in-dim out-dim degree]
  (let [n-coeffs (inc degree)
        ;; Xavier-like init: σ = sqrt(2 / (in + out))
        sigma (math/sqrt (/ 2.0 (+ in-dim out-dim)))
        ;; Coefficients: [out-dim × in-dim × n-coeffs]
        coeffs (vec
                (for [_j (range out-dim)]
                  (vec
                   (for [_i (range in-dim)]
                     (t2/tensor (repeatedly n-coeffs
                                  (fn [] (* sigma (- (rand) 0.5) 2.0)))
                                [n-coeffs])))))]
    {:in-dim   in-dim
     :out-dim  out-dim
     :degree   degree
     :n-coeffs n-coeffs
     :coeffs   coeffs}))  ; coeffs[j][i] = Tensor [n-coeffs]

(defn kan-layer-forward
  "Forward pass: x [batch × in-dim] → out [batch × out-dim].
   Каждый выход y_j = Σ_i poly(x_i ; coeffs[j][i])."
  [layer x-batch]
  (let [{:keys [in-dim out-dim coeffs]} layer
        batch (first (:shape x-batch))
        ;; Собираем столбцы x
        x-cols (vec (for [i (range in-dim)]
                      ;; x-col[i] = x-batch[:, i], shape [batch]
                      (let [^doubles xd (:data x-batch)
                            col-data (double-array batch)]
                        (dotimes [b batch]
                          (aset col-data b (aget xd (+ (* b in-dim) i))))
                        (let [xcol (t2/tensor (vec col-data) [batch])]
                          (assoc xcol
                                 :parents [x-batch]
                                 :backward
                                 (fn []
                                   (t2/ensure-grad! x-batch)
                                   (let [^doubles gx @(:grad x-batch)
                                         ^doubles gcol @(:grad xcol)]
                                     (dotimes [b batch]
                                       (let [idx (+ (* b in-dim) i)]
                                         (aset gx idx (+ (aget gx idx) (aget gcol b))))))))))))
        ;; Для каждого выхода j: sum_i poly(x_cols[i], coeffs[j][i])
        outputs (vec
                 (for [j (range out-dim)]
                   (reduce t2/t-add
                           (for [i (range in-dim)]
                             (t2/poly-forward-batch (get-in coeffs [j i])
                                                   (x-cols i))))))]
    ;; Stack outputs в [batch × out-dim]
    (if (= out-dim 1)
      ;; Один выход — reshape
      (let [out (first outputs)]
        (t2/->Tensor (:data out) [batch 1] (t2/compute-stride [batch 1])
                     (:numel out) (:grad out) (:parents out) (:backward out)))
      ;; Несколько: interleave
      (let [out-data (double-array (* batch out-dim))]
        (dotimes [b batch]
          (dotimes [j out-dim]
            (aset out-data (+ (* b out-dim) j)
                  (aget ^doubles (:data (outputs j)) b))))
        ;; Нужен backward через все outputs
        (let [out (t2/->Tensor out-data [batch out-dim]
                               (t2/compute-stride [batch out-dim])
                               (* batch out-dim) (atom nil) (vec outputs) nil)]
          (assoc out :backward
                 (fn []
                   (let [^doubles og @(:grad out)]
                     (dotimes [j out-dim]
                       (t2/ensure-grad! (outputs j))
                       (let [^doubles gj @(:grad (outputs j))]
                         (dotimes [b batch]
                           (aset gj b (+ (aget gj b)
                                         (aget og (+ (* b out-dim) j)))))))))))))))

;; ============================================================
;; ACTIVATION (между слоями)
;; ============================================================

(defn silu-forward
  "SiLU activation: σ(x)·x на тензоре."
  [x]
  (let [n (int (:numel x))
        od (double-array n)]
    (dotimes [i n]
      (let [xi (aget ^doubles (:data x) i)
            sig (/ 1.0 (+ 1.0 (math/exp (- xi))))]
        (aset od i (* xi sig))))
    (let [out (t2/->Tensor od (:shape x) (:stride x) n (atom nil) [x] nil)]
      (assoc out :backward
             (fn []
               (t2/ensure-grad! x)
               (let [^doubles og @(:grad out)
                     ^doubles gx @(:grad x)]
                 (dotimes [i n]
                   (let [xi (aget ^doubles (:data x) i)
                         sig (/ 1.0 (+ 1.0 (math/exp (- xi))))
                         dsilu (+ sig (* xi sig (- 1.0 sig)))]
                     (aset gx i (+ (aget gx i) (* (aget og i) dsilu)))))))))))

;; ============================================================
;; MULTI-LAYER KAN MODEL
;; ============================================================

(defn make-model
  "Создаёт multi-layer KAN модель.
   arch = [in-dim hidden₁ hidden₂ ... out-dim]
   degree = степень полиномов."
  [arch degree]
  (let [layers (vec (for [i (range (dec (count arch)))]
                      (make-kan-layer (arch i) (arch (inc i)) degree)))]
    {:arch   arch
     :degree degree
     :layers layers}))

(defn model-forward
  "Forward через все слои: x → KAN₁ → SiLU → KAN₂ → ... → output."
  [model x-batch]
  (let [layers (:layers model)
        n (count layers)]
    (loop [h x-batch
           i 0]
      (if (= i n)
        h
        (let [out (kan-layer-forward (layers i) h)
              ;; SiLU между слоями (кроме последнего)
              activated (if (< i (dec n)) (silu-forward out) out)]
          (recur activated (inc i)))))))

(defn all-params
  "Собирает все параметры модели (для SGD)."
  [model]
  (vec (for [layer (:layers model)
             j (range (:out-dim layer))
             i (range (:in-dim layer))]
         (get-in (:coeffs layer) [j i]))))

(defn model-update
  "Обновляет параметры модели после SGD step."
  [model new-params]
  (let [idx (atom 0)]
    (update model :layers
            (fn [layers]
              (mapv (fn [layer]
                      (update layer :coeffs
                              (fn [coeffs]
                                (mapv (fn [row]
                                        (mapv (fn [_]
                                                (let [p (new-params @idx)]
                                                  (swap! idx inc)
                                                  p))
                                              row))
                                      coeffs))))
                    layers)))))

;; ============================================================
;; TRAINING: SGD (Фаза 32)
;; ============================================================

(defn train-step
  "Один шаг SGD: forward → loss → backward → sgd → new model."
  [model x-batch y-batch lr]
  (let [pred (model-forward model x-batch)
        pred-flat (if (> (count (:shape pred)) 1)
                    (let [n (:numel pred)]
                      (t2/->Tensor (:data pred) [n] [1] n
                                   (:grad pred) (:parents pred) (:backward pred)))
                    pred)
        loss (t2/mse-loss pred-flat y-batch)]
    (t2/backward! loss)
    (let [params (all-params model)
          new-params (mapv #(t2/sgd-step! % lr 5.0) params)
          new-model (model-update model new-params)
          loss-val (aget ^doubles (:data loss) 0)]
      [new-model loss-val])))

(defn train
  "Обучение SGD. Возвращает [trained-model history]."
  [model data epochs lr & [{:keys [print-every] :or {print-every 10}}]]
  (let [x (:x data) y (:y data)]
    (loop [m model ep 0 history []]
      (if (= ep epochs) [m history]
        (let [[m2 loss] (train-step m x y lr)]
          (when (zero? (mod (inc ep) print-every))
            (println (format "    Epoch %3d | Loss: %.6f" (inc ep) loss)))
          (recur m2 (inc ep) (conj history loss)))))))

;; ============================================================
;; ADAM OPTIMIZER (Фаза 33)
;; ============================================================

(defn make-adam-state
  "Инициализирует Adam state для всех параметров модели.
   m = first moment (zeros), v = second moment (zeros), t = step counter."
  [model]
  (let [params (all-params model)]
    {:m (mapv (fn [p] (double-array (:numel p))) params)   ; first moment
     :v (mapv (fn [p] (double-array (:numel p))) params)   ; second moment
     :t 0}))                                                ; step counter

(defn adam-step!
  "Adam update для одного параметра. Возвращает новый Tensor.
   β₁=0.9, β₂=0.999, ε=1e-8."
  [param ^doubles m-arr ^doubles v-arr lr t max-grad]
  (let [beta1 0.9
        beta2 0.999
        eps   1e-8
        n     (int (:numel param))
        nd    (double-array n)
        t     (double t)]
    (when-let [^doubles g @(:grad param)]
      (dotimes [i n]
        (let [gi (max (- max-grad) (min max-grad (aget g i)))]
          ;; Update biased moments
          (aset m-arr i (+ (* beta1 (aget m-arr i)) (* (- 1.0 beta1) gi)))
          (aset v-arr i (+ (* beta2 (aget v-arr i)) (* (- 1.0 beta2) (* gi gi))))
          ;; Bias correction
          (let [m-hat (/ (aget m-arr i) (- 1.0 (math/pow beta1 t)))
                v-hat (/ (aget v-arr i) (- 1.0 (math/pow beta2 t)))]
            (aset nd i (- (aget ^doubles (:data param) i)
                         (* lr (/ m-hat (+ (math/sqrt v-hat) eps)))))))))
    (t2/tensor (vec nd) (:shape param))))

(defn cosine-lr
  "Cosine annealing: lr oscillates between lr-max and lr-min.
   lr(t) = lr_min + ½(lr_max - lr_min)(1 + cos(π·t/T))"
  [epoch total-epochs lr-max lr-min]
  (+ lr-min (* 0.5 (- lr-max lr-min)
               (+ 1.0 (math/cos (* Math/PI (/ (double epoch) (double total-epochs))))))))

;; ============================================================
;; MINI-BATCH (Фаза 33)
;; ============================================================

(defn shuffle-data
  "Перемешивает данные (x, y) поэлементно. Возвращает {:x Tensor, :y Tensor}."
  [x-data y-data]
  (let [n (first (:shape x-data))
        in-dim (if (> (count (:shape x-data)) 1) (second (:shape x-data)) 1)
        indices (shuffle (range n))
        ^doubles xd (:data x-data)
        ^doubles yd (:data y-data)
        new-x (double-array (* n in-dim))
        new-y (double-array n)]
    (dotimes [dst n]
      (let [src (int (nth indices dst))]
        (aset new-y dst (aget yd src))
        (dotimes [d in-dim]
          (aset new-x (+ (* dst in-dim) d)
                (aget xd (+ (* src in-dim) d))))))
    {:x (t2/tensor (vec new-x) (:shape x-data))
     :y (t2/tensor (vec new-y) [n])}))

(defn get-batch
  "Извлекает mini-batch [start, start+batch-size) из данных."
  [x-data y-data start batch-size]
  (let [n (first (:shape x-data))
        in-dim (if (> (count (:shape x-data)) 1) (second (:shape x-data)) 1)
        end (min (+ start batch-size) n)
        bs (- end start)
        ^doubles xd (:data x-data)
        ^doubles yd (:data y-data)
        bx (double-array (* bs in-dim))
        by (double-array bs)]
    (dotimes [i bs]
      (aset by i (aget yd (+ start i)))
      (dotimes [d in-dim]
        (aset bx (+ (* i in-dim) d)
              (aget xd (+ (* (+ start i) in-dim) d)))))
    {:x (t2/tensor (vec bx) (if (= in-dim 1) [bs 1] [bs in-dim]))
     :y (t2/tensor (vec by) [bs])}))

;; ============================================================
;; TRAIN-ADAM (Фаза 33)
;; ============================================================

(defn adam-train-step
  "Один шаг Adam: forward → loss → backward → adam → new model."
  [model adam-state x-batch y-batch lr]
  (let [pred (model-forward model x-batch)
        pred-flat (if (> (count (:shape pred)) 1)
                    (let [n (:numel pred)]
                      (t2/->Tensor (:data pred) [n] [1] n
                                   (:grad pred) (:parents pred) (:backward pred)))
                    pred)
        loss (t2/mse-loss pred-flat y-batch)]
    (t2/backward! loss)
    (let [params (all-params model)
          t (inc (:t adam-state))
          new-params (mapv (fn [p m v] (adam-step! p m v lr t 5.0))
                           params (:m adam-state) (:v adam-state))
          new-model (model-update model new-params)
          loss-val (aget ^doubles (:data loss) 0)]
      [new-model (assoc adam-state :t t) loss-val])))

(defn train-adam
  "Обучение с Adam + mini-batch + cosine LR.
   opts: {:batch-size N, :lr-min 1e-5, :print-every 10, :cosine? true}"
  [model data epochs lr & [{:keys [batch-size lr-min print-every cosine?]
                             :or {batch-size nil lr-min 1e-5
                                  print-every 10 cosine? false}}]]
  (let [n (first (:shape (:x data)))
        bs (or batch-size n)]  ; nil = full batch
    (loop [m model
           adam (make-adam-state model)
           ep 0
           history []]
      (if (= ep epochs) [m history]
        (let [cur-lr (if cosine?
                       (cosine-lr ep epochs lr lr-min)
                       lr)
              ;; Shuffle + mini-batch
              shuffled (shuffle-data (:x data) (:y data))
              ;; Process all mini-batches
              [m2 adam2 ep-loss]
              (loop [m-inner m
                     adam-inner adam
                     offset 0
                     batch-losses []]
                (if (>= offset n)
                  [m-inner adam-inner
                   (/ (reduce + batch-losses) (count batch-losses))]
                  (let [batch (get-batch (:x shuffled) (:y shuffled) offset bs)
                        [m3 adam3 loss] (adam-train-step m-inner adam-inner
                                                        (:x batch) (:y batch) cur-lr)]
                    (recur m3 adam3 (+ offset bs) (conj batch-losses loss)))))]
          (when (zero? (mod (inc ep) print-every))
            (println (format "    Epoch %3d | Loss: %.6f | LR: %.6f"
                             (inc ep) ep-loss cur-lr)))
          (recur m2 adam2 (inc ep) (conj history ep-loss)))))))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-kan-framework []
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  Multi-Layer KAN Framework           ║")
  (println "  ║  Phase 32: SGD + Phase 33: Adam      ║")
  (println "  ╚══════════════════════════════════════╝")

  ;; === Part 1: SGD vs Adam on sin(x) ===
  (println "\n  Part 1: SGD vs Adam → sin(x)")
  (let [n 50
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        x-data (t2/tensor xs [n 1])
        y-data (t2/tensor ys [n])
        data {:x x-data :y y-data}
        ;; SGD
        model-sgd (make-model [1 1] 5)
        [_ hist-sgd] (train model-sgd data 100 0.01 {:print-every 200})
        ;; Adam (same init would be unfair, use fresh)
        model-adam (make-model [1 1] 5)
        [_ hist-adam] (train-adam model-adam data 100 0.01 {:print-every 200})]
    (println (format "    SGD  100ep: %.6f" (last hist-sgd)))
    (println (format "    Adam 100ep: %.6f" (last hist-adam)))
    (println (format "    Adam/SGD ratio: %.2f×" (/ (last hist-sgd) (max 1e-10 (last hist-adam))))))

  ;; === Part 2: Adam on sin(x₁)+x₂² ===
  (println "\n  Part 2: Adam [2→4→1] → sin(x₁)+x₂²")
  (let [n 40
        xs (vec (for [_ (range n)] [(- (* 4.0 (rand)) 2.0)
                                     (- (* 4.0 (rand)) 2.0)]))
        ys (mapv (fn [[x1 x2]] (+ (math/sin x1) (* x2 x2))) xs)
        x-data (t2/tensor (flatten xs) [n 2])
        y-data (t2/tensor ys [n])
        model (make-model [2 4 1] 4)
        [trained history] (train-adam model {:x x-data :y y-data} 300 0.01
                                     {:print-every 75})]
    (println (format "    Final loss: %.6f (started: %.6f)" (last history) (first history)))
    (println (format "    Improvement: %.1f%%" (* 100 (- 1 (/ (last history) (first history)))))))

  ;; === Part 3: Mini-batch + Cosine LR ===
  (println "\n  Part 3: Mini-batch(10) + Cosine LR → sin(πx₁)·cos(x₂)")
  (let [n 60
        xs (vec (for [_ (range n)] [(- (* 2.0 (rand)) 1.0)
                                     (- (* 2.0 (rand)) 1.0)]))
        ys (mapv (fn [[x1 x2]] (* (math/sin (* Math/PI x1)) (math/cos x2))) xs)
        x-data (t2/tensor (flatten xs) [n 2])
        y-data (t2/tensor ys [n])
        model (make-model [2 4 4 1] 4)
        [trained history] (train-adam model {:x x-data :y y-data} 200 0.01
                                     {:batch-size 10 :cosine? true
                                      :lr-min 1e-4 :print-every 50})]
    (println (format "    Final loss: %.6f (started: %.6f)" (last history) (first history)))
    (println (format "    Improvement: %.1f%%" (* 100 (- 1 (/ (last history) (first history)))))))

  ;; === Part 4: Cosine LR visualization ===
  (println "\n  Part 4: Cosine annealing schedule (100 epochs)")
  (let [lrs (mapv #(cosine-lr % 100 0.01 0.0001) (range 101))]
    (println (format "    Epoch  0: lr=%.6f" (first lrs)))
    (println (format "    Epoch 25: lr=%.6f" (nth lrs 25)))
    (println (format "    Epoch 50: lr=%.6f" (nth lrs 50)))
    (println (format "    Epoch 75: lr=%.6f" (nth lrs 75)))
    (println (format "    Epoch100: lr=%.6f" (last lrs))))

  ;; === Part 5: Architecture summary ===
  (println "\n  Part 5: Architecture summary")
  (let [model (make-model [2 4 4 1] 4)
        params (all-params model)
        total (reduce + (map :numel params))]
    (println (format "    Architecture: [2→4→4→1], degree=4"))
    (println (format "    Total params: %d" total))
    (println "    Optimizer:    Adam (β₁=0.9, β₂=0.999)")
    (println "    Mini-batch:   shuffle + split")
    (println "    LR schedule:  cosine annealing")))

