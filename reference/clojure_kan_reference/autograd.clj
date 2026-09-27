(ns kan-kat.autograd
  "Reverse-mode автоматическое дифференцирование (как micrograd).
   
   ═══════════════════════════════════════════════════
   АРХИТЕКТУРА
   ═══════════════════════════════════════════════════
   
   Каждый узел (Value) хранит:
   - data      : числовое значение
   - grad      : atom (накопленный градиент)
   - parents   : входные узлы
   - backward  : fn → обновляет grad родителей
   
   backward() :
   1. Topological sort (DFS)
   2. Обратный порядок
   3. Накопление градиентов (chain rule)
   
   Это настоящий dynamic computation graph,
   как в PyTorch, но на Clojure.
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]))

;; ============================================================
;; VALUE NODE
;; ============================================================

(defrecord Value [data grad parents backward label])

(defn value
  "Создаёт узел computation graph."
  ([data]
   (->Value data (atom 0.0) [] nil ""))
  ([data label]
   (->Value data (atom 0.0) [] nil label))
  ([data parents backward-fn]
   (->Value data (atom 0.0) parents backward-fn ""))
  ([data parents backward-fn label]
   (->Value data (atom 0.0) parents backward-fn label)))

;; ============================================================
;; БАЗОВЫЕ ОПЕРАЦИИ (+, -, *, /)
;; ============================================================

(defn ag-add
  "a + b"
  [a b]
  (let [out (value (+ (:data a) (:data b)) [a b] nil "+")]
    (assoc out :backward
           (fn []
             (swap! (:grad a) + @(:grad out))
             (swap! (:grad b) + @(:grad out))))))

(defn ag-sub
  "a - b"
  [a b]
  (let [out (value (- (:data a) (:data b)) [a b] nil "-")]
    (assoc out :backward
           (fn []
             (swap! (:grad a) + @(:grad out))
             (swap! (:grad b) - @(:grad out))))))

(defn ag-mul
  "a * b"
  [a b]
  (let [out (value (* (:data a) (:data b)) [a b] nil "*")]
    (assoc out :backward
           (fn []
             (swap! (:grad a) + (* (:data b) @(:grad out)))
             (swap! (:grad b) + (* (:data a) @(:grad out)))))))

(defn ag-div
  "a / b"
  [a b]
  (let [out (value (/ (:data a) (:data b)) [a b] nil "/")]
    (assoc out :backward
           (fn []
             (swap! (:grad a) + (/ @(:grad out) (:data b)))
             (swap! (:grad b) - (/ (* (:data a) @(:grad out))
                                   (* (:data b) (:data b))))))))

;; ============================================================
;; АКТИВАЦИИ
;; ============================================================

(defn ag-tanh
  "tanh(a)"
  [a]
  (let [t (math/tanh (:data a))
        out (value t [a] nil "tanh")]
    (assoc out :backward
           (fn []
             (swap! (:grad a)
                    + (* (- 1.0 (* t t)) @(:grad out)))))))

(defn ag-sin
  "sin(a)"
  [a]
  (let [out (value (math/sin (:data a)) [a] nil "sin")]
    (assoc out :backward
           (fn []
             (swap! (:grad a)
                    + (* (math/cos (:data a)) @(:grad out)))))))

(defn ag-cos
  "cos(a)"
  [a]
  (let [out (value (math/cos (:data a)) [a] nil "cos")]
    (assoc out :backward
           (fn []
             (swap! (:grad a)
                    - (* (math/sin (:data a)) @(:grad out)))))))

(defn ag-exp
  "exp(a)"
  [a]
  (let [e (math/exp (min (:data a) 20.0))
        out (value e [a] nil "exp")]
    (assoc out :backward
           (fn []
             (swap! (:grad a)
                    + (* e @(:grad out)))))))

(defn ag-pow
  "a^n (n — числовая константа, не Value)"
  [a n]
  (let [out (value (math/pow (:data a) n) [a] nil (str "^" n))]
    (assoc out :backward
           (fn []
             (swap! (:grad a)
                    + (* n
                         (math/pow (:data a) (dec n))
                         @(:grad out)))))))

(defn ag-relu
  "ReLU(a)"
  [a]
  (let [out (value (max 0.0 (:data a)) [a] nil "relu")]
    (assoc out :backward
           (fn []
             (swap! (:grad a)
                    + (* (if (pos? (:data a)) 1.0 0.0)
                         @(:grad out)))))))

(defn ag-sigmoid
  "σ(a)"
  [a]
  (let [s (/ 1.0 (+ 1.0 (math/exp (- (:data a)))))
        out (value s [a] nil "σ")]
    (assoc out :backward
           (fn []
             (swap! (:grad a)
                    + (* s (- 1.0 s) @(:grad out)))))))

;; ============================================================
;; TOPOLOGICAL SORT
;; ============================================================

(defn topo-sort
  "Топологическая сортировка графа вычислений (DFS)."
  [v]
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

;; ============================================================
;; BACKWARD PASS
;; ============================================================

(defn backward!
  "Вычисляет градиенты всех узлов относительно v.
   
   1. Сортируем граф топологически
   2. Устанавливаем grad(v) = 1.0
   3. Идём в обратном порядке
   4. Для каждого узла вызываем backward-fn"
  [v]
  (reset! (:grad v) 1.0)
  (doseq [node (reverse (topo-sort v))]
    (when-let [bw (:backward node)]
      (bw))))

;; ============================================================
;; ZERO GRAD (обнулить перед новой итерацией)
;; ============================================================

(defn zero-grad!
  "Обнуляет градиенты всех узлов."
  [nodes]
  (doseq [n nodes]
    (reset! (:grad n) 0.0)))

;; ============================================================
;; SGD UPDATE
;; ============================================================

(defn sgd-step!
  "SGD с gradient clipping: param.data -= lr * clip(grad).
   Возвращает обновлённые Value с новыми data."
  [params lr & [{:keys [max-grad] :or {max-grad 5.0}}]]
  (mapv (fn [p]
          (let [g (max (- max-grad) (min max-grad @(:grad p)))
                new-data (- (:data p) (* lr g))]
            (->Value new-data (atom 0.0) [] nil (:label p))))
        params))

;; ============================================================
;; KAN POLY PHI (через autograd)
;; ============================================================

(defn poly-forward
  "Полиномиальная φ через computation graph.
   coeffs = [Value, Value, ...], x = Value.
   φ(x) = c₀ + c₁·x + c₂·x² + ..."
  [coeffs x]
  (reduce (fn [acc [i c]]
            (if (zero? i)
              (ag-add acc c)
              (ag-add acc (ag-mul c (ag-pow x i)))))
          (value 0.0)
          (map-indexed vector coeffs)))

;; ============================================================
;; MSE LOSS
;; ============================================================

(defn mse-loss
  "MSE loss через computation graph.
   predictions и targets — вектора Value."
  [predictions targets]
  (let [n (count predictions)
        sum-sq (reduce ag-add
                       (value 0.0)
                       (map (fn [pred target]
                              (let [diff (ag-sub pred target)]
                                (ag-pow diff 2)))
                            predictions targets))]
    (ag-div sum-sq (value (double n)))))

;; ============================================================
;; ВЕРИФИКАЦИЯ AUTOGRAD
;; ============================================================

(defn verify-gradients
  "Проверяет autograd vs numerical gradients."
  [f params x-val eps]
  (let [;; Autograd
        x (value x-val)
        result (f params x)
        _ (backward! result)
        ag-grads (mapv #(deref (:grad %)) params)
        ;; Numerical
        num-grads
        (mapv (fn [i]
                (let [p+ (mapv (fn [j p]
                                 (if (= i j)
                                   (value (+ (:data p) eps))
                                   (value (:data p))))
                               (range) params)
                      p- (mapv (fn [j p]
                                 (if (= i j)
                                   (value (- (:data p) eps))
                                   (value (:data p))))
                               (range) params)
                      f+ (:data (f p+ (value x-val)))
                      f- (:data (f p- (value x-val)))]
                  (/ (- f+ f-) (* 2.0 eps))))
              (range (count params)))]
    {:autograd ag-grads
     :numerical num-grads
     :max-error (reduce max 0.0
                        (map #(abs (- %1 %2)) ag-grads num-grads))}))

;; ============================================================
;; ТРЕНИРОВКА KAN POLY С AUTOGRAD
;; ============================================================

(defn train-kan-autograd
  "Обучает KAN PolyPhi через autograd.
   target  — целевая f(x) (обычная fn)
   domain  — [lo hi]
   degree  — степень полинома
   epochs  — число эпох
   lr      — learning rate"
  [target-fn domain n-points degree epochs lr]
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  KAN Autograd Training               ║")
  (println "  ╚══════════════════════════════════════╝")
  (println (format "  Poly degree: %d | Points: %d | lr: %.4f" degree n-points lr))
  (println "  ────────────────────────────────────────")
  
  ;; Обучающие данные
  (let [data (mapv (fn [_]
                     (let [x (+ (first domain)
                                (* (rand) (- (second domain) (first domain))))]
                       [x (target-fn x)]))
                   (range n-points))]
    ;; Начальные параметры (random small)
    (loop [coeffs (mapv (fn [_] (value (* 0.1 (- (rand) 0.5))))
                        (range (inc degree)))
           epoch 1]
      (if (> epoch epochs)
        (do
          (println "  ────────────────────────────────────────")
          (println (format "  Final coeffs: %s"
                           (pr-str (mapv #(format "%.4f" (:data %)) coeffs))))
          coeffs)
        (let [;; Forward + loss
              preds (mapv (fn [[xi _]]
                            (poly-forward coeffs (value xi)))
                          data)
              targets (mapv (fn [[_ yi]]
                              (value yi))
                            data)
              loss (mse-loss preds targets)
              ;; Backward
              _ (backward! loss)
              ;; Gradient step
              new-coeffs (sgd-step! coeffs lr {:max-grad 5.0})]
          (when (zero? (mod epoch (max 1 (quot epochs 6))))
            (println (format "  Epoch %4d | Loss: %.6f | |grad|: %.4f"
                             epoch (:data loss)
                             (math/sqrt (reduce + 0.0
                                          (map #(let [g @(:grad %)] (* g g))
                                               coeffs))))))
          (recur new-coeffs (inc epoch)))))))
