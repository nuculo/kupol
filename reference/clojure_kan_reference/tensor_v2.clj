(ns kan-kat.tensor-v2
  "Production Tensor Engine: минимальный PyTorch-backend на JVM.
   
   Тензор = double-array + shape + stride.
   Мутабельный backend для скорости,
   иммутабельный граф для autograd.
   
   - N-мерные тензоры, stride-based индексация
   - Broadcasting (NumPy-совместимый)
   - Reverse-mode autograd
   - MatMul с полным backward
   - Batched KAN layer"
  (:require [clojure.math :as math])
  (:import [java.util Arrays]))

;; ============ STRIDE ============

(defn compute-stride [shape]
  (loop [i (dec (count shape)) acc 1 result (list)]
    (if (< i 0) (vec result)
      (recur (dec i) (* acc (nth shape i)) (conj result acc)))))

;; ============ TENSOR ============

(defrecord Tensor [^doubles data shape stride numel grad parents backward])

(defn tensor [values shape]
  (let [n (int (reduce * shape))
        da (double-array n)]
    (dotimes [i (min n (count values))]
      (aset da i (double (nth values i))))
    (->Tensor da (vec shape) (compute-stride shape) n
              (atom nil) [] nil)))

(defn zeros [shape] (tensor (repeat (reduce * shape) 0.0) shape))
(defn ones  [shape] (tensor (repeat (reduce * shape) 1.0) shape))
(defn randn [shape sigma]
  (tensor (repeatedly (reduce * shape) #(* sigma (- (rand) 0.5) 2.0)) shape))

;; ============ GRAD ============

(defn ensure-grad! [t]
  (when (nil? @(:grad t))
    (reset! (:grad t) (double-array (:numel t)))))

;; ============ INDEXING ============

(defn flat->multi [flat-idx stride ndim]
  (let [result (int-array ndim)]
    (loop [f (long flat-idx) d 0]
      (when (< d ndim)
        (let [s (long (nth stride d))]
          (aset result d (int (quot f s)))
          (recur (rem f s) (inc d)))))
    result))

(defn multi->flat ^long [stride ^ints idx ndim]
  (loop [d 0 off 0]
    (if (= d ndim) off
      (recur (inc d) (+ off (* (long (aget idx d)) (long (nth stride d))))))))

;; ============ BROADCASTING ============

(defn broadcast-shapes [sa sb]
  (let [n (max (count sa) (count sb))
        pa (concat (repeat (- n (count sa)) 1) sa)
        pb (concat (repeat (- n (count sb)) 1) sb)]
    (vec (map (fn [a b]
                (cond (= a b) a (= a 1) b (= b 1) a
                      :else (throw (ex-info "Shape mismatch" {:a sa :b sb}))))
              pa pb))))

(defn broadcast-idx ^ints [^ints bcast-idx orig-shape ndim-b ndim-o]
  (let [result (int-array ndim-o)
        off (- ndim-b ndim-o)]
    (dotimes [i ndim-o]
      (aset result i (if (= 1 (int (nth orig-shape i)))
                       0 (aget bcast-idx (+ off i)))))
    result))

;; ============ BINARY OPS (broadcast) ============

(defn- bcast-binary [a b op grad-a grad-b]
  (let [os (broadcast-shapes (:shape a) (:shape b))
        nd (count os)
        on (int (reduce * os))
        ost (compute-stride os)
        od (double-array on)
        nda (count (:shape a))
        ndb (count (:shape b))]
    (dotimes [i on]
      (let [mi (flat->multi i ost nd)
            ai (broadcast-idx mi (:shape a) nd nda)
            bi (broadcast-idx mi (:shape b) nd ndb)
            va (aget ^doubles (:data a) (int (multi->flat (:stride a) ai nda)))
            vb (aget ^doubles (:data b) (int (multi->flat (:stride b) bi ndb)))]
        (aset od i (op va vb))))
    (let [out (->Tensor od os ost on (atom nil) [a b] nil)]
      (assoc out :backward
             (fn []
               (ensure-grad! a)
               (ensure-grad! b)
               (let [^doubles og @(:grad out)]
                 (dotimes [i on]
                   (let [mi (flat->multi i ost nd)
                         ai (broadcast-idx mi (:shape a) nd nda)
                         bi (broadcast-idx mi (:shape b) nd ndb)
                         ao (int (multi->flat (:stride a) ai nda))
                         bo (int (multi->flat (:stride b) bi ndb))
                         va (aget ^doubles (:data a) ao)
                         vb (aget ^doubles (:data b) bo)
                         g  (aget og i)]
                     (let [^doubles ga @(:grad a)
                           ^doubles gb @(:grad b)]
                       (aset ga ao (+ (aget ga ao) (double (grad-a va vb g))))
                       (aset gb bo (+ (aget gb bo) (double (grad-b va vb g)))))))))))))

(defn t-add [a b] (bcast-binary a b + (fn [_ _ g] g) (fn [_ _ g] g)))
(defn t-sub [a b] (bcast-binary a b - (fn [_ _ g] g) (fn [_ _ g] (- g))))
(defn t-mul [a b] (bcast-binary a b * (fn [_ vb g] (* vb g)) (fn [va _ g] (* va g))))

;; ============ UNARY OPS ============

(defn- unary-op [a op deriv]
  (let [n (int (:numel a))
        od (double-array n)]
    (dotimes [i n]
      (aset od i (op (aget ^doubles (:data a) i))))
    (let [out (->Tensor od (:shape a) (:stride a) n (atom nil) [a] nil)]
      (assoc out :backward
             (fn []
               (ensure-grad! a)
               (let [^doubles og @(:grad out)
                     ^doubles ga @(:grad a)]
                 (dotimes [i n]
                   (aset ga i (+ (aget ga i)
                                 (* (aget og i)
                                    (deriv (aget ^doubles (:data a) i))))))))))))

(defn t-sin    [a] (unary-op a #(math/sin %) #(math/cos %)))
(defn t-cos    [a] (unary-op a #(math/cos %) #(- (math/sin %))))
(defn t-tanh   [a] (unary-op a #(math/tanh %) #(- 1.0 (let [t (math/tanh %)] (* t t)))))
(defn t-relu   [a] (unary-op a #(max 0.0 %) #(if (pos? %) 1.0 0.0)))
(defn t-exp    [a] (unary-op a #(math/exp (min % 20.0)) #(math/exp (min % 20.0))))
(defn t-square [a] (unary-op a #(* % %) #(* 2.0 %)))

;; ============ MATMUL ============

(defn t-matmul [a b]
  (let [[m k] (:shape a)
        [k2 n] (:shape b)
        _ (assert (= k k2) "matmul: inner dims must match")
        mn (* m n)
        od (double-array mn)]
    (dotimes [i m]
      (dotimes [j n]
        (aset od (+ (* i n) j)
              (loop [p 0 s 0.0]
                (if (= p k) s
                  (recur (inc p)
                         (+ s (* (aget ^doubles (:data a) (+ (* i k) p))
                                 (aget ^doubles (:data b) (+ (* p n) j))))))))))
    (let [out (->Tensor od [m n] (compute-stride [m n]) mn (atom nil) [a b] nil)]
      (assoc out :backward
             (fn []
               (ensure-grad! a)
               (ensure-grad! b)
               (let [^doubles og @(:grad out)
                     ^doubles ga @(:grad a)
                     ^doubles gb @(:grad b)]
                 (dotimes [i m]
                   (dotimes [p k]
                     (let [s (loop [j 0 acc 0.0]
                               (if (= j n) acc
                                 (recur (inc j)
                                        (+ acc (* (aget og (+ (* i n) j))
                                                  (aget ^doubles (:data b) (+ (* p n) j)))))))]
                       (aset ga (+ (* i k) p) (+ (aget ga (+ (* i k) p)) s)))))
                 (dotimes [p k]
                   (dotimes [j n]
                     (let [s (loop [i 0 acc 0.0]
                               (if (= i m) acc
                                 (recur (inc i)
                                        (+ acc (* (aget ^doubles (:data a) (+ (* i k) p))
                                                  (aget og (+ (* i n) j)))))))]
                       (aset gb (+ (* p n) j) (+ (aget gb (+ (* p n) j)) s)))))))))))

;; ============ REDUCTIONS ============

(defn t-sum [a]
  (let [n (int (:numel a))
        s (loop [i 0 acc 0.0] (if (= i n) acc (recur (inc i) (+ acc (aget ^doubles (:data a) i)))))
        out (->Tensor (double-array [s]) [1] [1] 1 (atom nil) [a] nil)]
    (assoc out :backward
           (fn []
             (ensure-grad! a)
             (let [g (aget ^doubles @(:grad out) 0)
                   ^doubles ga @(:grad a)]
               (dotimes [i n] (aset ga i (+ (aget ga i) g))))))))

(defn t-mean [a]
  (let [n (int (:numel a))
        s (/ (loop [i 0 acc 0.0] (if (= i n) acc (recur (inc i) (+ acc (aget ^doubles (:data a) i))))) n)
        out (->Tensor (double-array [s]) [1] [1] 1 (atom nil) [a] nil)]
    (assoc out :backward
           (fn []
             (ensure-grad! a)
             (let [g (/ (aget ^doubles @(:grad out) 0) (double n))
                   ^doubles ga @(:grad a)]
               (dotimes [i n] (aset ga i (+ (aget ga i) g))))))))

(defn t-cat
  "Конкатенирует два 2D тензора `a` и `b` вдоль второй размерности (features).
   a: [batch, dim-a]
   b: [batch, dim-b]
   out: [batch, dim-a + dim-b]"
  [a b]
  (let [sa (:shape a)
        sb (:shape b)
        batch (first sa)
        dim-a (second sa)
        dim-b (second sb)
        out-dim (+ dim-a dim-b)
        out-data (double-array (* batch out-dim))
        ^doubles da (:data a)
        ^doubles db (:data b)]
    (dotimes [b-idx batch]
      (let [out-offset (* b-idx out-dim)
            a-offset (* b-idx dim-a)
            b-offset (* b-idx dim-b)]
        (System/arraycopy da a-offset out-data out-offset dim-a)
        (System/arraycopy db b-offset out-data (+ out-offset dim-a) dim-b)))
    (let [out (->Tensor out-data [batch out-dim] (compute-stride [batch out-dim])
                        (* batch out-dim) (atom nil) [a b] nil)]
      (assoc out :backward
             (fn []
               (ensure-grad! a)
               (ensure-grad! b)
               (let [^doubles og @(:grad out)
                     ^doubles ga @(:grad a)
                     ^doubles gb @(:grad b)]
                 (dotimes [b-idx batch]
                   (let [out-offset (* b-idx out-dim)
                         a-offset (* b-idx dim-a)
                         b-offset (* b-idx dim-b)]
                     (dotimes [i dim-a]
                       (aset ga (+ a-offset i) (+ (aget ga (+ a-offset i)) (aget og (+ out-offset i)))))
                     (dotimes [i dim-b]
                       (aset gb (+ b-offset i) (+ (aget gb (+ b-offset i)) (aget og (+ out-offset dim-a i)))))))))))))

;; ============ BACKWARD ============

(defn topo-sort [v]
  (let [visited (java.util.HashSet.)
        order   (java.util.ArrayList.)]
    (letfn [(build [node]
              (when-not (.contains visited node)
                (.add visited node)
                (doseq [p (:parents node)] (build p))
                (.add order node)))]
      (build v)
      (vec order))))

(defn backward! [v]
  (ensure-grad! v)
  (Arrays/fill ^doubles @(:grad v) 1.0)
  (doseq [node (reverse (topo-sort v))]
    (when (:backward node)
      (ensure-grad! node)
      ((:backward node)))))

;; ============ BATCHED KAN POLY ============

(defn poly-forward-batch [coeffs x-batch]
  (let [n   (int (:numel x-batch))
        deg (dec (int (:numel coeffs)))
        od  (double-array n)]
    (dotimes [j n]
      (let [xj (aget ^doubles (:data x-batch) j)]
        (aset od j (loop [i 0 acc 0.0 xp 1.0]
                     (if (> i deg) acc
                       (recur (inc i) (+ acc (* (aget ^doubles (:data coeffs) i) xp)) (* xp xj)))))))
    (let [out (->Tensor od [n] [1] n (atom nil) [coeffs x-batch] nil)]
      (assoc out :backward
             (fn []
               (ensure-grad! coeffs)
               (ensure-grad! x-batch)
               (let [^doubles og @(:grad out)
                     ^doubles gc @(:grad coeffs)
                     ^doubles gx @(:grad x-batch)]
                 (dotimes [j n]
                   (let [xj (aget ^doubles (:data x-batch) j)
                         gj (aget og j)]
                     ;; Gradient w.r.t coefficients
                     (loop [i 0 xp 1.0]
                       (when (<= i deg)
                         (aset gc i (+ (aget gc i) (* gj xp)))
                         (recur (inc i) (* xp xj))))
                     ;; Gradient w.r.t input x_j
                     (aset gx j (+ (aget gx j)
                                   (* gj (loop [i 1 acc 0.0 xp 1.0]
                                           (if (> i deg) acc
                                             (recur (inc i)
                                                    (+ acc (* (aget ^doubles (:data coeffs) i) (double i) xp))
                                                    (* xp xj)))))))))))))))

;; ============ CLASSIFICATION & SGD ============

(defn t-cross-entropy
  "Вычисляет кросс-энтропию (фьюз Softmax + LogLoss) между ненормализованными 
   логитами [N, C] и тензором индексов правильных классов targets [N, 1].
   Возвращает скалярный тензор потери.
   Стабильно к численному переполнению (вычитание max)."
  [logits targets]
  (let [N (nth (:shape logits) 0)
        C (nth (:shape logits) 1)
        ^doubles ldata (:data logits)
        ^doubles tdata (:data targets)
        pdata (double-array (* N C))
        loss (loop [i 0 total-loss 0.0]
               (if (= i N)
                 (/ total-loss (double N))
                 (let [target-class (int (aget tdata i))
                       ;; Для стабильности exp вычитаем max-logit в строке
                       max-l (loop [j 0 mx (- Double/MAX_VALUE)]
                               (if (= j C) mx (recur (inc j) (max mx (aget ldata (+ (* i C) j))))))
                       exp-sum (loop [j 0 sum 0.0]
                                 (if (= j C) sum 
                                   (let [val (Math/exp (- (aget ldata (+ (* i C) j)) max-l))]
                                     (aset pdata (+ (* i C) j) val)
                                     (recur (inc j) (+ sum val)))))
                       prob (/ (aget pdata (+ (* i C) target-class)) exp-sum)]
                   ;; Записываем нормализованные вероятности обратно в pdata для Backward
                   (dotimes [j C]
                     (aset pdata (+ (* i C) j) (/ (aget pdata (+ (* i C) j)) exp-sum)))
                   ;; Аккумулируем лог-потерю
                   (recur (inc i) (- total-loss (Math/log (max 1e-15 prob)))))))
        out (->Tensor (double-array [loss]) [1] [1] 1 (atom nil) [logits] nil)]
    (assoc out :backward
           (fn []
             (ensure-grad! logits)
             (let [g (aget ^doubles @(:grad out) 0)
                   ^doubles g-logits @(:grad logits)]
               (dotimes [i N]
                 (let [target-class (int (aget tdata i))]
                   (dotimes [j C]
                     (let [p (aget pdata (+ (* i C) j))
                           indicator (if (= j target-class) 1.0 0.0)
                           grad-val (* g (/ (- p indicator) (double N)))]
                       (aset g-logits (+ (* i C) j) 
                             (+ (aget g-logits (+ (* i C) j)) grad-val)))))))))))

(defn mse-loss [preds targets] (t-mean (t-square (t-sub preds targets))))

(defn sgd-step! [param lr max-grad]
  (let [n (int (:numel param))
        nd (double-array n)]
    (when-let [^doubles g @(:grad param)]
      (dotimes [i n]
        (aset nd i (- (aget ^doubles (:data param) i)
                      (* lr (max (- max-grad) (min max-grad (aget g i))))))))
    (tensor (vec nd) (:shape param))))

;; ============ DEMO ============

(defn demo-tensor-v2 []
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  Production Tensor Engine v2         ║")
  (println "  ╚══════════════════════════════════════╝")

  ;; 1. Broadcasting
  (println "\n  Part 1: Broadcasting")
  (let [a (tensor [1 2 3] [3 1])
        b (tensor [10 20] [1 2])
        c (t-add a b)]
    (println (format "    [3×1] + [1×2] → %s" (pr-str (:shape c))))
    (println (format "    Result: %s" (pr-str (mapv long (vec (:data c)))))))

  ;; 2. MatMul + backward
  (println "\n  Part 2: MatMul + backward")
  (let [a (tensor [1 2 3 4] [2 2])
        b (tensor [5 6 7 8] [2 2])
        c (t-matmul a b)
        loss (t-sum c)]
    (backward! loss)
    (println (format "    A@B = %s" (pr-str (mapv long (vec (:data c))))))
    (println (format "    ∂L/∂A = %s (expect [11 15 11 15])" (pr-str (mapv long (vec @(:grad a))))))
    (println (format "    ∂L/∂B = %s (expect [4 4 6 6])" (pr-str (mapv long (vec @(:grad b)))))))

  ;; 3. Gradient verification
  (println "\n  Part 3: Gradient verification (tensor)")
  (let [cd [0.5 -0.3 0.1]
        xd [0.5 1.0 1.5 2.0]
        ag (let [c (tensor cd [3]) x (tensor xd [4]) y (tensor (mapv math/sin xd) [4])]
             (backward! (mse-loss (poly-forward-batch c x) y))
             (vec @(:grad c)))
        eps 1e-5
        num (mapv (fn [i]
                    (let [c+ (assoc cd i (+ (nth cd i) eps))
                          c- (assoc cd i (- (nth cd i) eps))
                          x (tensor xd [4]) y (tensor (mapv math/sin xd) [4])
                          l+ (aget ^doubles (:data (mse-loss (poly-forward-batch (tensor c+ [3]) x) y)) 0)
                          l- (aget ^doubles (:data (mse-loss (poly-forward-batch (tensor c- [3]) x) y)) 0)]
                      (/ (- l+ l-) (* 2.0 eps))))
                  (range 3))
        err (reduce max 0.0 (map #(abs (- %1 %2)) ag num))]
    (println (format "    Max error: %.2e %s" err (if (< err 1e-4) "✅ PASS" "❌ FAIL"))))

  ;; 4. Batched KAN training
  (println "\n  Part 4: Batched training on sin(x)")
  (let [n-pts 50
        xs (mapv (fn [_] (- (* 4.0 (rand)) 2.0)) (range n-pts))
        ys (mapv math/sin xs)
        xb (tensor xs [n-pts])
        yt (tensor ys [n-pts])]
    (loop [c (tensor (repeatedly 4 #(* 0.1 (- (rand) 0.5))) [4]) ep 1]
      (if (> ep 100)
        (println (format "    Final: %s" (pr-str (mapv #(format "%.4f" %) (vec (:data c))))))
        (let [loss (mse-loss (poly-forward-batch c xb) yt)
              _ (backward! loss)
              nc (sgd-step! c 0.01 5.0)]
          (when (zero? (mod ep 25))
            (println (format "    Epoch %3d | Loss: %.6f" ep (aget ^doubles (:data loss) 0))))
          (recur nc (inc ep)))))))
