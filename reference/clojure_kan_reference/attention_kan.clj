(ns kan-kat.attention-kan
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]))

(defn make-kan-attention
  "Создает слой Multi-Head Attention, где проекции Q, K, V и O заменены на KAN-слои.
   В обычном Transformer: Q = X * Wq.
   В KAN-Transformer v2: Q = KAN_q(X).
   `dim-in`: размерность входного токена.
   `dim-k`: размерность Query и Key.
   `dim-v`: размерность Value.
   `degree`: степень полинома B-spline (по умолчанию 3)."
  [dim-in dim-k dim-v degree]
  {:q-kan (fw/make-model [dim-in 8 dim-k] degree)
   :k-kan (fw/make-model [dim-in 8 dim-k] degree)
   :v-kan (fw/make-model [dim-in 8 dim-v] degree)
   :o-kan (fw/make-model [dim-v 8 dim-in] degree)
   :scale (/ 1.0 (Math/sqrt dim-k))})

(defn kan-attention-forward
  "Выполняет проход Attention.
   `attn` - конфигурация из `make-kan-attention`.
   `x` - входной тензор последовательности размерности [SeqLen, dim-in].
   (Для упрощения пока работаем с batch=1 внутри одного Attention блока, 
    или batch x seq-len если `x` плоский).
   
   Внимание вычисляется так:
   Q = KAN_q(X), K = KAN_k(X), V = KAN_v(X)
   Scores = Q * K^T * scale
   Probs = Softmax(Scores)
   Out = Probs * V
   O = KAN_o(Out)"
  [attn x]
  (let [{:keys [q-kan k-kan v-kan o-kan scale]} attn
        
        ;; 1. Проецируем вход X в Q, K, V через KAN
        Q (fw/model-forward q-kan x)
        K (fw/model-forward k-kan x)
        V (fw/model-forward v-kan x)
        
        ;; 2. Scaled Dot-Product: Scores = Q * K^T
        ;; Кастомный Транспоуз для матрицы K [SeqL, dim-k] -> [dim-k, SeqL]
        k-rows (nth (:shape K) 0)
        k-cols (nth (:shape K) 1)
        k-t-data (double-array (* k-rows k-cols))
        ^doubles kd (:data K)
        ;; Перекладываем данные: K^T[c, r] = K[r, c]
        _ (dotimes [r k-rows]
            (dotimes [c k-cols]
              (aset k-t-data (+ (* c k-rows) r) 
                    (aget kd (+ (* r k-cols) c)))))
        K-t (t2/->Tensor k-t-data [k-cols k-rows] (t2/compute-stride [k-cols k-rows])
                         (* k-rows k-cols) (atom nil) [K] nil)
                         
        ;; Backward для Транспонирования
        _ (assoc K-t :backward
                 (fn []
                   (t2/ensure-grad! K)
                   (let [^doubles gk @(:grad K)
                         ^doubles gkt @(:grad K-t)]
                     (dotimes [r k-rows]
                       (dotimes [c k-cols]
                         (let [idx-k (+ (* r k-cols) c)
                               idx-kt (+ (* c k-rows) r)]
                           (aset gk idx-k (+ (aget gk idx-k) (aget gkt idx-kt)))))))))
        
        scores-raw (t2/t-matmul Q K-t)
        
        ;; Умножаем на scale: 1 / sqrt(dk)
        ;; Мы можем сделать это через broadcasting scale, но пока 
        ;; напишем кастомную обертку для скалирования с backward
        numel (:numel scores-raw)
        scaled-data (double-array numel)
        ^doubles sr-data (:data scores-raw)
        _ (dotimes [i numel]
            (aset scaled-data i (* (aget sr-data i) scale)))
            
        scores (t2/->Tensor scaled-data (:shape scores-raw) (:stride scores-raw)
                            numel (atom nil) [scores-raw] nil)
        
        ;; Привязываем backward для скалирования
        _ (assoc scores :backward
                 (fn []
                   (t2/ensure-grad! scores-raw)
                   (let [^doubles gs @(:grad scores)
                         ^doubles gsr @(:grad scores-raw)]
                     (dotimes [i numel]
                       (aset gsr i (+ (aget gsr i) (* (aget gs i) scale)))))))
        
        ;; 3. Softmax по последней оси (axis=1) - строкам матрицы Scores
        rows (nth (:shape scores) 0)
        cols (nth (:shape scores) 1)
        probs-data (double-array numel)
        ^doubles sc-data (:data scores)
        
        _ (dotimes [r rows]
            (let [start (* r cols)
                  ;; Найти максимум для стабильности
                  max-val (loop [c 0 m -1e9]
                            (if (= c cols) m
                                (let [v (aget sc-data (+ start c))]
                                  (recur (inc c) (if (> v m) v m)))))
                  ;; Экспоненты и сумма
                  sum-exp (loop [c 0 s 0.0]
                            (if (= c cols) s
                                (let [v (Math/exp (- (aget sc-data (+ start c)) max-val))]
                                  (aset probs-data (+ start c) v)
                                  (recur (inc c) (+ s v)))))]
              ;; Нормализация
              (dotimes [c cols]
                (aset probs-data (+ start c) (/ (aget probs-data (+ start c)) sum-exp)))))
                
        probs (t2/->Tensor probs-data (:shape scores) (:stride scores)
                           numel (atom nil) [scores] nil)
                           
        ;; Backward Softmax
        _ (assoc probs :backward
                 (fn []
                   (t2/ensure-grad! scores)
                   (let [^doubles gp @(:grad probs)
                         ^doubles gs @(:grad scores)
                         ^doubles pd (:data probs)]
                     (dotimes [r rows]
                       (let [start (* r cols)
                             ;; sum(gp * probs)
                             dp-sum (loop [c 0 s 0.0]
                                      (if (= c cols) s
                                          (recur (inc c) (+ s (* (aget gp (+ start c))
                                                                 (aget pd (+ start c)))))))]
                         (dotimes [c cols]
                           (let [idx (+ start c)
                                 p (aget pd idx)
                                 ;; ds = p * (dp - sum(dp * p))
                                 g (* p (- (aget gp idx) dp-sum))]
                             (aset gs idx (+ (aget gs idx) g)))))))))
        
        ;; 4. Умножаем Probs * V
        attention-out (t2/t-matmul probs V)
        
        ;; 5. Финальная проекция через O-KAN
        out (fw/model-forward o-kan attention-out)]
    out))

(defn kan-attn-params [attn]
  (concat (fw/all-params (:q-kan attn))
          (fw/all-params (:k-kan attn))
          (fw/all-params (:v-kan attn))
          (fw/all-params (:o-kan attn))))

(defn kan-attn-update [attn new-params]
  (let [pq (count (fw/all-params (:q-kan attn)))
        pk (count (fw/all-params (:k-kan attn)))
        pv (count (fw/all-params (:v-kan attn)))
        po (count (fw/all-params (:o-kan attn)))
        
        nq (vec (take pq new-params))
        rem-k (drop pq new-params)
        nk (vec (take pk rem-k))
        rem-v (drop pk rem-k)
        nv (vec (take pv rem-v))
        rem-o (drop pv rem-v)
        no (vec (take po rem-o))]
    (assoc attn
           :q-kan (fw/model-update (:q-kan attn) nq)
           :k-kan (fw/model-update (:k-kan attn) nk)
           :v-kan (fw/model-update (:v-kan attn) nv)
           :o-kan (fw/model-update (:o-kan attn) no))))

;; ==========================================
;; Demo 41: Attention-KAN (KAT v2) Mapping
;; ==========================================

(defn demo-attention-kan []
  (println "\n==========================================")
  (println " Demo 41: Attention-KAN (KAT v2)")
  (println "==========================================\n")
  
  (let [SeqL 4
        DimIn 3
        DimK 5
        DimV 4
        
        ;; Датасет: перевести набор 4 токенов из одной размерности в другую, используя контекст
        x-data (double-array [1.0 0.0 0.0,  0.0 1.0 0.0,  0.0 0.0 1.0,  1.0 1.0 1.0])
        X-train (t2/tensor (vec x-data) [SeqL DimIn])
        
        y-data (double-array [1.0 1.0 1.0,  1.0 0.0 0.0,  0.0 1.0 0.0,  0.0 0.0 1.0])
        Y-train (t2/tensor (vec y-data) [SeqL DimIn])
        
        attn-init (make-kan-attention DimIn DimK DimV 3)]
        
    (println "Training KAN-based Self-Attention...")
    (println (format "Seq: %d | DimIn: %d | DimK: %d" SeqL DimIn DimK))
    
    (loop [ep 1
           attn attn-init]
      (if (> ep 40)
        (do
          (println "Training finished.")
          attn)
        (let [pred (kan-attention-forward attn X-train)
              loss-t (t2/mse-loss pred Y-train)
              loss-val (aget ^doubles (:data loss-t) 0)]
              
          (t2/backward! loss-t)
          
          (let [p-attn (kan-attn-params attn)
                ;; Clip at 2.0 to prevent explosive KAN spline scaling
                new-p (mapv #(t2/sgd-step! % 0.01 2.0) p-attn)
                new-attn (kan-attn-update attn new-p)]
                
            (when (zero? (mod ep 10))
              (println (format "  Epoch %3d | Loss: %8.4f" ep loss-val)))
            (recur (inc ep) new-attn)))))))
