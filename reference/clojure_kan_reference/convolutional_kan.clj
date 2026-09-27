(ns kan-kat.convolutional-kan
  "Фаза 46: Convolutional KAN (1D Convolution).
   Разбивает временной ряд на окна (unfold) и пропускает их через KAN слои."
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]))

(defn unfold-1d
  "Извлекает скользящие окна из [B, L, C_in].
   Возвращает матрицу [B * L_out, K * C_in] и функцию backward."
  [x k-size stride]
  (let [shape (:shape x)
        B (int (nth shape 0))
        L (int (nth shape 1))
        C (int (nth shape 2))
        L-out (int (inc (quot (- L k-size) stride)))
        out-rows (* B L-out)
        out-cols (* k-size C)
        n (* out-rows out-cols)
        out-data (double-array n)
        x-data ^doubles (:data x)]
    
    ;; Forward
    (dotimes [i n]
      ;; i = out-idx = row * out-cols + col
      (let [row (quot i out-cols)
            col (rem i out-cols)
            ;; row = b * L-out + l
            b (quot row L-out)
            l (rem row L-out)
            ;; col = k * C + c
            k (quot col C)
            c (rem col C)
            
            x-start (* l stride)
            x-idx (+ (* b L C) (* (+ x-start k) C) c)]
        (aset out-data i (aget x-data x-idx))))
    
    (let [out (t2/->Tensor out-data [out-rows out-cols] 
                           (t2/compute-stride [out-rows out-cols])
                           n
                           (atom nil) [x] nil)]
      (assoc out :backward
             (fn []
               (t2/ensure-grad! x)
               (let [^doubles og @(:grad out)
                     ^doubles gx @(:grad x)]
                 (dotimes [i n]
                   ;; Recover indices from flat index `i` = out_idx
                   ;; i = row * out-cols + col
                   (let [row (quot i out-cols)
                         col (rem i out-cols)
                         ;; row = b * L-out + l
                         b (quot row L-out)
                         l (rem row L-out)
                         ;; col = k * C + c
                         k (quot col C)
                         c (rem col C)
                         
                         x-start (* l stride)
                         x-idx (+ (* b L C) (* (+ x-start k) C) c)]
                     (aset gx x-idx (+ (aget gx x-idx) (aget og i)))))))))))

(defn make-conv1d-kan
  "Создает сверточный KAN слой. 
   in-channels: C_in, out-channels: C_out, kernel-size: K.
   Внутри это KAN модель, принимающая [K * C_in] и отдающая [C_out]."
  [in-channels out-channels kernel-size stride num-knots]
  {:type :conv1d-kan
   :kernel-size kernel-size
   :stride stride
   :out-channels out-channels
   ;; Инкапсулируем полносвязный KAN как фильтр
   :filter (fw/make-model [(* kernel-size in-channels) out-channels] num-knots)})

(defn conv1d-forward
  "Пропускает [B, L, C_in] через Conv1D-KAN слой.
   Возвращает тензор [B, L_out, C_out]."
  [conv-layer x]
  (let [k (:kernel-size conv-layer)
        s (:stride conv-layer)
        B (nth (:shape x) 0)
        L (nth (:shape x) 1)
        L-out (inc (quot (- L k) s))
        C-out (:out-channels conv-layer)
        
        ;; 1. Extract patches: [B * L_out, K * C_in]
        patches (unfold-1d x k s)
        
        ;; 2. Apply KAN filter: [B * L_out, C_out]
        filtered (fw/model-forward (:filter conv-layer) patches)
        
        ;; 2.5 Apply SiLU to convolutional results to prevent linearity lock
        activated (fw/silu-forward filtered)
        
        ;; 3. Reshape back to [B, L_out, C_out]
        numel (:numel activated)
        out-shape [B L-out C-out]]
    
    (let [out (t2/->Tensor (:data activated) out-shape 
                           (t2/compute-stride out-shape)
                           numel
                           (atom nil) [activated] nil)]
      ;; Простой проброс градиентов от reshape
      (assoc out :backward
             (fn []
               (t2/ensure-grad! activated)
               (let [^doubles og @(:grad out)
                     ^doubles ga @(:grad activated)]
                 (dotimes [i numel]
                   (aset ga i (+ (aget ga i) (aget og i))))))))))

(defn conv1d-params
  "Извлекает параметры KAN-фильтра."
  [conv-layer]
  (fw/all-params (:filter conv-layer)))

(defn conv1d-update
  "Обновляет параметры KAN-фильтра."
  [conv-layer new-params]
  (assoc conv-layer :filter (fw/model-update (:filter conv-layer) new-params)))

;; ============================================================
;; DEMO 39: TIME SERIES PREDICTION
;; ============================================================

(defn demo-conv-kan []
  (println "==========================================")
  (println " Demo 39: Convolutional KAN (1D)")
  (println "==========================================\n")
  
  (let [;; Датасет: [B=30, L=15, C=1]
        ;; Predict next value based on sequence of length 15
        B 30
        L 15
        C-in 1
        x-raw (double-array (* B L C-in))
        y-raw (double-array B)
        
        _ (dotimes [b B]
            (let [t0 (* b 0.5)]
              (dotimes [l L]
                (let [t (+ t0 (* l 0.1))
                      val (+ (Math/sin t) (* 0.5 (Math/cos (* 3 t))))]
                  (aset x-raw (+ (* b L) l) val)))
              ;; target is value at t + 0.1
              (let [t-target (+ t0 (* L 0.1))
                    target (+ (Math/sin t-target) (* 0.5 (Math/cos (* 3 t-target))))]
                (aset y-raw b target))))
        
        X-train (t2/tensor (vec x-raw) [B L C-in])
        Y-train (t2/tensor (vec y-raw) [B 1])
        
        ;; Model: Conv1D (k=5, s=2) -> Flatten -> Linear KAN
        ;; Output of Conv1D: L_out = (15 - 5)/2 + 1 = 6. Shape: [B, 6, C_out]
        C-out 4
        hidden-kan 8
        conv-layer (make-conv1d-kan C-in C-out 5 2 3) ;; K=5, S=2
        linear-layer (fw/make-model [(* 6 C-out) hidden-kan 1] 3)
        
        lr 0.05
        epochs 80]
        
    (println "Training Conv1D KAN on Time Series data...")
    (println "Dataset: [100 samples, length 20]")
    (println "Conv1D Filter: kernel=5, stride=2, channels=4")
    
    (loop [ep 1
           conv conv-layer
           lin linear-layer]
      (if (<= ep epochs)
        (let [;; Forward
              conv-out-3d (conv1d-forward conv X-train)
              ;; Flatten [B, L_out, C_out] -> [B, L_out * C_out]
              B-out (nth (:shape conv-out-3d) 0)
              L-out (nth (:shape conv-out-3d) 1)
              conv-flat (t2/->Tensor (:data conv-out-3d) [B-out (* L-out C-out)] 
                                     (t2/compute-stride [B-out (* L-out C-out)])
                                     (:numel conv-out-3d)
                                     (atom nil) [conv-out-3d] nil)
              
              ;; Привязка gradient-routing к flat-тензору
              conv-flat (assoc conv-flat :backward 
                               (fn []
                                 (t2/ensure-grad! conv-out-3d)
                                 (let [^doubles og @(:grad conv-flat)
                                       ^doubles gc3d @(:grad conv-out-3d)
                                       n (:numel conv-out-3d)]
                                   (dotimes [i n]
                                     (aset gc3d i (+ (aget gc3d i) (aget og i)))))))
                                     
              pred (fw/model-forward lin conv-flat)
              loss-raw (t2/mse-loss pred Y-train)
              loss-val (aget ^doubles (:data loss-raw) 0)]
              
          (t2/backward! loss-raw)
          
          (let [lin-p (fw/all-params lin)
                conv-p (conv1d-params conv)
                
                ;; Сбор всех параметров и общий шаг оптимизатора
                all-p (concat lin-p conv-p)
                new-all-p (mapv #(t2/sgd-step! % lr 5.0) all-p)
                
                ;; Разделение параметров обратно
                new-lin-p (vec (take (count lin-p) new-all-p))
                new-conv-p (vec (drop (count lin-p) new-all-p))
                
                new-lin (fw/model-update lin new-lin-p)
                new-conv (conv1d-update conv new-conv-p)]
                
            (when (zero? (mod ep 30))
              (println (format "  Epoch %3d | Loss: %8.4f" ep loss-val)))
            (recur (inc ep) new-conv new-lin)))
        (println "  Done.\n")))))
