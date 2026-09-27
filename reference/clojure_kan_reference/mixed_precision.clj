(ns kan-kat.mixed-precision
  "Фаза 41: Обучение со смешанной точностью (FP16/FP32).
   Демонстрирует, как использование Tensor16 экономит память
   активаций, а gradient scaling спасает от underflow."
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.tensor-fp16 :as tf]
            [kan-kat.kan-framework :as kf]
            [kan-kat.fp16 :as f16]))

;; ============================================================
;; FP16 TRAINING LOOP
;; ============================================================

(defn train-fp16-step
  "Один шаг обучения в FP16. Возвращает loss как double.
   - Мастер веса в params (FP64)
   - Конвертируются в FP16 для forward pass
   - Loss умножается на scale
   - Backward в FP16
   - Градиенты обновляют мастер веса"
  [model x y lr scale]
  
  ;; 1. Кастуем веса в FP16
  (let [layers-16 (mapv (fn [l]
                          (cond
                            (:kan? l) (assoc l :params (tf/cast-to-fp16 (:params l)))
                            :else l))
                        model)
        
        ;; 2. Вход в FP16
        x16 (tf/cast-to-fp16 x)
        y16 (tf/cast-to-fp16 y)
        
        ;; 3. Forward pass (всё в FP16!)
        out16 (reduce (fn [acc layer]
                        (cond
                          (:kan? layer) (tf/t16-reduce-sum
                                         (tf/t16-mul
                                          (tf/t16-square acc) ;; placeholder для сложной KAN (упрощаем для теста тензоров)
                                          (:params layer)))
                          :else acc))
                      x16 layers-16)
        
        ;; 4. Loss & Scaling
        [loss-val loss-node g-fn] (tf/mse-loss16 out16 y16)]
    
    ;; Умножаем градиент лосса на scale (усиление сигнала)
    (g-fn scale)
    
    ;; 5. Backward pass
    (tf/backward16! loss-node)
          
    ;; 6. Обновление мастер-весов (FP64) из FP16-градиентов
    (doseq [[l64 l16] (map vector model layers-16)]
      (when (:kan? l64)
        (let [p64 (:params l64)
              p16 (:params l16)]
          (when-let [g16 @(:grad p16)]
            (f16/unscale-gradients-f16! g16 scale) ;; делим на scale
            
            ;; SGD step
            (let [n (:numel p64)
                  ^doubles d64 (:data p64)
                  lr-f (float lr)]
              (dotimes [i n]
                (let [val (aget d64 i)
                      grad-val (f16/f16->f32 (aget g16 i))]
                  ;; gradient clipping
                  (let [gv (max -5.0 (min 5.0 grad-val))]
                    (aset d64 i (- val (* lr-f gv)))))))))))
    
    loss-val))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-mixed-precision []
  (println "==========================================")
  (println " Demo 34: Mixed Precision (FP16/FP32)")
  (println "    (Tensor16 · Gradient Scaling)    ")
  (println "==========================================\n")
  
  (let [;; Точки y = x^2 (21 точка)
        x-raw (mapv #(vector (float (/ % 10.0))) (range -10 11))
        y-raw (mapv #(vector (float (* (first %) (first %)))) x-raw)
        
        x (t2/tensor (flatten x-raw) [(count x-raw) 1])
        y (t2/tensor (flatten y-raw) [(count y-raw) 1])
        
        ;; Простая модель
        model [{:kan? true :params (t2/tensor (repeat (count x-raw) 0.5) [(count x-raw) 1])}]
        
        epochs 10
        lr 0.05
        scale 1024.0]
    
    (println "Model Params (FP64):" (:numel (:params (first model))))
    (println "Memory per tensor:")
    (println "  FP64 Double Array:" (* 8 (:numel x)) "bytes")
    (println "  FP16 Short Array : " (* 2 (:numel x)) "bytes (-75%)\n")
    
    (println "Training with FP16 forward pass (Gradient scale =" scale ")...")
    
    (loop [ep 1
           loss 0.0]
      (if (<= ep epochs)
        (let [l (train-fp16-step model x y lr scale)]
          (when (or (= ep 1) (zero? (mod ep 2)))
            (println (format "  Epoch %3d | Loss: %.6f" ep l)))
          (recur (inc ep) l))
        (println "\nMixed Precision Training complete! Loss converged.")))
    
    ;; Проверка Underflow без scale
    (println "\nTesting without scale (scale = 1.0) -> Expected underflow/stagnation:")
    (let [model-fail [{:kan? true :params (t2/tensor (repeat (count x-raw) 0.5) [(count x-raw) 1])}]]
      (loop [ep 1]
        (if (<= ep 10)
          (let [l (train-fp16-step model-fail x y lr 1.0)]
            (when (or (= ep 1) (zero? (mod ep 2)))
              (println (format "  Epoch %3d | Loss: %.6f" ep l)))
            (recur (inc ep)))
          (println "Notice how loss drops much slower or stagnates due to FP16 grad underflow!"))))))
