(ns kan-kat.tensor
  "Tensor Engine: батчевый autograd для KAN.
   
   ═══════════════════════════════════════════════════
   ИДЕЯ: ОТ ПАЛЬЦЕВ К КАЛЬКУЛЯТОРУ
   ═══════════════════════════════════════════════════
   
   Скалярный autograd: 1 точка = 1 граф.
   Tensor engine: N точек = 1 граф → N× ускорение.
   
   Tensor = {:data flat-vector, :shape [rows cols], :grad, :backward}
   Все операции поэлементные или матричные.
   Backward вычисляет Якобиан батчом.
   
   Clojure: double[] (Java) для скорости,
   protocols для полиморфизма.
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]))

;; ============================================================
;; TENSOR RECORD
;; ============================================================

(defrecord Tensor [data shape grad parents backward label])

(defn tensor
  "Создаёт тензор.
   data  — flat vector of doubles
   shape — [d1 d2 ...], product = (count data)"
  ([data shape]
   (->Tensor (vec (map double data))
             (vec shape)
             (atom (vec (repeat (count data) 0.0)))
             [] nil ""))
  ([data shape label]
   (->Tensor (vec (map double data))
             (vec shape)
             (atom (vec (repeat (count data) 0.0)))
             [] nil label)))

(defn scalar
  "Тензор-скаляр [1]."
  [x]
  (tensor [x] [1]))

(defn zeros
  "Нулевой тензор."
  [shape]
  (tensor (vec (repeat (reduce * shape) 0.0)) shape))

(defn ones
  "Единичный тензор."
  [shape]
  (tensor (vec (repeat (reduce * shape) 1.0)) shape))

(defn randn
  "Случайный тензор ~ N(0, σ)."
  [shape sigma]
  (let [n (reduce * shape)]
    (tensor (vec (repeatedly n #(* sigma (- (rand) 0.5) 2.0))) shape)))

(defn numel
  "Число элементов."
  [t]
  (count (:data t)))

;; ============================================================
;; ЭЛЕМЕНТАРНЫЕ ОПЕРАЦИИ (с backward)
;; ============================================================

(defn t-add
  "Поэлементное сложение: a + b (одинаковый shape)."
  [a b]
  (let [out (tensor (mapv + (:data a) (:data b)) (:shape a))]
    (assoc out
           :parents [a b]
           :backward (fn []
                       (let [g @(:grad out)]
                         (swap! (:grad a) #(mapv + % g))
                         (swap! (:grad b) #(mapv + % g)))))))

(defn t-sub
  "Поэлементное вычитание: a - b."
  [a b]
  (let [out (tensor (mapv - (:data a) (:data b)) (:shape a))]
    (assoc out
           :parents [a b]
           :backward (fn []
                       (let [g @(:grad out)]
                         (swap! (:grad a) #(mapv + % g))
                         (swap! (:grad b) #(mapv - % g)))))))

(defn t-mul
  "Поэлементное умножение: a * b (Hadamard)."
  [a b]
  (let [out (tensor (mapv * (:data a) (:data b)) (:shape a))]
    (assoc out
           :parents [a b]
           :backward (fn []
                       (let [g @(:grad out)]
                         (swap! (:grad a) #(mapv + % (mapv * g (:data b))))
                         (swap! (:grad b) #(mapv + % (mapv * g (:data a)))))))))

(defn t-scale
  "Скалярное умножение: c * a (c — число)."
  [c a]
  (let [out (tensor (mapv #(* c %) (:data a)) (:shape a))]
    (assoc out
           :parents [a]
           :backward (fn []
                       (let [g @(:grad out)]
                         (swap! (:grad a) #(mapv + % (mapv (partial * c) g))))))))

;; ============================================================
;; МАТРИЧНЫЕ ОПЕРАЦИИ
;; ============================================================

(defn t-matmul
  "Матричное умножение: A[m×k] @ B[k×n] → C[m×n].
   Backward: ∂L/∂A = ∂L/∂C @ B^T, ∂L/∂B = A^T @ ∂L/∂C."
  [a b]
  (let [[m k1] (:shape a)
        [k2 n] (:shape b)
        _ (assert (= k1 k2) (format "matmul shape mismatch: %s vs %s" (:shape a) (:shape b)))
        k k1
        ;; C[i,j] = Σ_p A[i,p] * B[p,j]
        result (vec (for [i (range m)]
                      (for [j (range n)]
                        (reduce + 0.0
                          (for [p (range k)]
                            (* (nth (:data a) (+ (* i k) p))
                               (nth (:data b) (+ (* p n) j))))))))
        out (tensor (vec (apply concat result)) [m n])]
    (assoc out
           :parents [a b]
           :backward
           (fn []
             (let [g @(:grad out)]
               ;; ∂L/∂A[i,p] = Σ_j ∂L/∂C[i,j] * B[p,j]
               (swap! (:grad a)
                      (fn [ga]
                        (vec (for [idx (range (* m k))]
                               (let [i (quot idx k)
                                     p (rem idx k)]
                                 (+ (nth ga idx)
                                    (reduce + 0.0
                                      (for [j (range n)]
                                        (* (nth g (+ (* i n) j))
                                           (nth (:data b) (+ (* p n) j)))))))))))
               ;; ∂L/∂B[p,j] = Σ_i A[i,p] * ∂L/∂C[i,j]
               (swap! (:grad b)
                      (fn [gb]
                        (vec (for [idx (range (* k n))]
                               (let [p (quot idx n)
                                     j (rem idx n)]
                                 (+ (nth gb idx)
                                    (reduce + 0.0
                                      (for [i (range m)]
                                        (* (nth (:data a) (+ (* i k) p))
                                           (nth g (+ (* i n) j))))))))))))))))

(defn t-sum
  "Сумма всех элементов → скаляр [1]."
  [a]
  (let [s (reduce + 0.0 (:data a))
        out (tensor [s] [1])]
    (assoc out
           :parents [a]
           :backward (fn []
                       (let [g (first @(:grad out))]
                         (swap! (:grad a)
                                #(mapv (fn [gi] (+ gi g)) %)))))))

(defn t-mean
  "Среднее → скаляр [1]."
  [a]
  (let [n (numel a)
        s (/ (reduce + 0.0 (:data a)) n)
        out (tensor [s] [1])]
    (assoc out
           :parents [a]
           :backward (fn []
                       (let [g (first @(:grad out))
                             inv-n (/ g n)]
                         (swap! (:grad a)
                                #(mapv (fn [gi] (+ gi inv-n)) %)))))))

;; ============================================================
;; АКТИВАЦИИ (поэлементные)
;; ============================================================

(defn t-tanh
  "tanh(a), поэлементно."
  [a]
  (let [vals (mapv math/tanh (:data a))
        out  (tensor vals (:shape a))]
    (assoc out
           :parents [a]
           :backward (fn []
                       (let [g @(:grad out)]
                         (swap! (:grad a)
                                #(mapv + %
                                       (mapv (fn [gi ti]
                                               (* gi (- 1.0 (* ti ti))))
                                             g vals))))))))

(defn t-sin
  "sin(a), поэлементно."
  [a]
  (let [out (tensor (mapv math/sin (:data a)) (:shape a))]
    (assoc out
           :parents [a]
           :backward (fn []
                       (let [g @(:grad out)]
                         (swap! (:grad a)
                                #(mapv + %
                                       (mapv (fn [gi xi]
                                               (* gi (math/cos xi)))
                                             g (:data a)))))))))

(defn t-relu
  "ReLU(a), поэлементно."
  [a]
  (let [out (tensor (mapv #(max 0.0 %) (:data a)) (:shape a))]
    (assoc out
           :parents [a]
           :backward (fn []
                       (let [g @(:grad out)]
                         (swap! (:grad a)
                                #(mapv + %
                                       (mapv (fn [gi xi]
                                               (if (pos? xi) gi 0.0))
                                             g (:data a)))))))))

(defn t-square
  "x², поэлементно."
  [a]
  (let [out (tensor (mapv #(* % %) (:data a)) (:shape a))]
    (assoc out
           :parents [a]
           :backward (fn []
                       (let [g @(:grad out)]
                         (swap! (:grad a)
                                #(mapv + %
                                       (mapv (fn [gi xi]
                                               (* 2.0 xi gi))
                                             g (:data a)))))))))

;; ============================================================
;; TOPOLOGICAL SORT + BACKWARD
;; ============================================================

(defn topo-sort [v]
  (let [visited (java.util.HashSet.)
        order   (java.util.ArrayList.)]
    (letfn [(build [node]
              (when-not (.contains visited node)
                (.add visited node)
                (doseq [p (:parents node)]
                  (build p))
                (.add order node)))]
      (build v)
      (vec order))))

(defn backward!
  "Backward по всему computation graph."
  [v]
  ;; Set grad of output to 1.0
  (reset! (:grad v) (vec (repeat (numel v) 1.0)))
  (doseq [node (reverse (topo-sort v))]
    (when-let [bw (:backward node)]
      (bw))))

(defn zero-grad!
  "Обнуляет градиенты."
  [tensors]
  (doseq [t tensors]
    (reset! (:grad t) (vec (repeat (numel t) 0.0)))))

;; ============================================================
;; BATCHED KAN POLY
;; ============================================================

(defn poly-forward-batch
  "PolyPhi через тензорный autograd.
   coeffs = Tensor [degree+1], x-batch = Tensor [N].
   Возвращает Tensor [N]."
  [coeffs x-batch]
  (let [n     (numel x-batch)
        deg   (dec (numel coeffs))
        ;; Для каждого x: Σ c_i * x^i
        result
        (vec (for [j (range n)]
               (let [xj (nth (:data x-batch) j)]
                 (reduce + 0.0
                   (map-indexed (fn [i c]
                                  (* c (math/pow xj i)))
                                (:data coeffs))))))
        out (tensor result [n])]
    (assoc out
           :parents [coeffs x-batch]
           :backward
           (fn []
             (let [g @(:grad out)]
               ;; ∂L/∂c_i = Σ_j g_j * x_j^i
               (swap! (:grad coeffs)
                      (fn [gc]
                        (mapv (fn [i old-g]
                                (+ old-g
                                   (reduce + 0.0
                                     (map (fn [j gj]
                                            (* gj (math/pow (nth (:data x-batch) j) i)))
                                          (range n) g))))
                              (range (numel coeffs)) gc)))
               ;; ∂L/∂x_j = Σ_i g_j * c_i * i * x_j^(i-1)
               (swap! (:grad x-batch)
                      (fn [gx]
                        (mapv (fn [j old-g]
                                (+ old-g
                                   (* (nth g j)
                                      (reduce + 0.0
                                        (map-indexed
                                          (fn [i c]
                                            (if (zero? i) 0.0
                                              (* c i (math/pow (nth (:data x-batch) j) (dec i)))))
                                          (:data coeffs))))))
                              (range n) gx))))))))

;; ============================================================
;; MSE LOSS (тензорная)
;; ============================================================

(defn mse-loss
  "MSE between predictions and targets (both Tensor [N])."
  [preds targets]
  (t-mean (t-square (t-sub preds targets))))

;; ============================================================
;; SGD
;; ============================================================

(defn sgd-step!
  "SGD с gradient clipping. Возвращает новый тензор."
  [param lr max-grad]
  (tensor (mapv (fn [d g]
                  (- d (* lr (max (- max-grad) (min max-grad g)))))
                (:data param) @(:grad param))
          (:shape param)
          (:label param)))

;; ============================================================
;; TRAINING DEMO
;; ============================================================

(defn train-tensor-kan
  "Обучает KAN PolyPhi через tensor autograd.
   Весь батч = 1 forward + 1 backward.
   Сравнение: скалярный autograd = N×forward + N×backward."
  [target-fn domain n-points degree epochs lr]
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  Tensor Engine KAN                   ║")
  (println "  ╚══════════════════════════════════════╝")
  (println (format "  Batch: %d | Degree: %d | lr: %.4f" n-points degree lr))
  (println "  ────────────────────────────────────────")
  
  ;; Data
  (let [xs (mapv (fn [_]
                   (+ (first domain)
                      (* (rand) (- (second domain) (first domain)))))
                 (range n-points))
        ys (mapv target-fn xs)
        x-batch  (tensor xs [n-points] "X")
        y-target (tensor ys [n-points] "Y")]
    (loop [coeffs (tensor (vec (repeatedly (inc degree)
                                           #(* 0.1 (- (rand) 0.5))))
                          [(inc degree)] "coeffs")
           epoch 1]
      (if (> epoch epochs)
        (do
          (println "  ────────────────────────────────────────")
          (println (format "  Final coeffs: %s"
                           (pr-str (mapv #(format "%.4f" %) (:data coeffs)))))
          coeffs)
        (let [preds (poly-forward-batch coeffs x-batch)
              loss  (mse-loss preds y-target)
              _     (backward! loss)
              new-c (sgd-step! coeffs lr 5.0)]
          (when (zero? (mod epoch (max 1 (quot epochs 6))))
            (println (format "  Epoch %4d | Loss: %.6f | |grad|: %.4f"
                             epoch
                             (first (:data loss))
                             (math/sqrt (reduce + 0.0
                                          (map #(* % %) @(:grad coeffs)))))))
          (recur new-c (inc epoch)))))))

;; ============================================================
;; ВЕРИФИКАЦИЯ: TENSOR vs SCALAR AUTOGRAD
;; ============================================================

(defn verify-tensor-grads
  "Проверяет тензорные градиенты vs числовые."
  [coeffs-data x-data]
  (let [coeffs (tensor coeffs-data [(count coeffs-data)])
        x-batch (tensor x-data [(count x-data)])
        y-target (tensor (mapv math/sin x-data) [(count x-data)])
        preds (poly-forward-batch coeffs x-batch)
        loss  (mse-loss preds y-target)
        _ (backward! loss)
        ag-grads @(:grad coeffs)
        ;; Numerical
        eps 1e-5
        num-grads
        (mapv (fn [i]
                (let [c+ (assoc coeffs-data i (+ (nth coeffs-data i) eps))
                      c- (assoc coeffs-data i (- (nth coeffs-data i) eps))
                      loss+ (first (:data (mse-loss
                                            (poly-forward-batch (tensor c+ [(count c+)]) x-batch)
                                            y-target)))
                      loss- (first (:data (mse-loss
                                            (poly-forward-batch (tensor c- [(count c-)]) x-batch)
                                            y-target)))]
                  (/ (- loss+ loss-) (* 2.0 eps))))
              (range (count coeffs-data)))]
    {:autograd ag-grads
     :numerical num-grads
     :max-error (reduce max 0.0 (map #(abs (- %1 %2)) ag-grads num-grads))}))
