(ns kan-kat.gradient-accumulation
  "Фаза 42: Gradient Accumulation.
   Позволяет эмулировать большие батчи через накопление градиентов
   за несколько микро-шагов перед применением оптимизатора."
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]))

;; ============================================================
;; GRADIENT ACCUMULATION LOGIC
;; ============================================================

(defn accumulate-gradients!
  "Один шаг накопления градиента.
   Вызывается для каждого микро-батча.
   Возвращает (накопленный loss) / accum-steps, чтобы 
   градиенты были усреднены как в едином большом батче."
  [model x-micro y-micro accum-steps]
  (let [pred (fw/model-forward model x-micro)
        ;; Вычисляем loss для микро-батча
        mseloss (t2/mse-loss pred y-micro)
        ;; Масштабируем loss (делим на accum-steps)
        scale-tensor (t2/tensor [(/ 1.0 accum-steps)] [1])
        scaled-loss (t2/t-mul mseloss scale-tensor)]
    ;; Backward (градиенты накапливаются в узлах model, так как мы их не очищаем)
    (t2/backward! scaled-loss)
    ;; Возвращаем сырой лосс для статистики (немасштабированный)
    (aget ^doubles (:data mseloss) 0)))

(defn train-accumulate-step
  "Выполняет один эффективный (большой) шаг оптимизатора путём накопления. 
   X-batch и Y-batch считаются 'большими' (effective-batch-size).
   Они бьются на `accum-steps` микро-батчей.
   Возвращает [new-model, avg-loss]."
  [model x-batch y-batch accum-steps lr]
  (let [bs (first (:shape x-batch))
        _ (assert (zero? (mod bs accum-steps)) 
                  "Batch size must be divisible by accum-steps for this demo")
        micro-bs (quot bs accum-steps)
        in-dim (last (:shape x-batch))
        out-dim (last (:shape y-batch))
        
        ^doubles xd (:data x-batch)
        ^doubles yd (:data y-batch)]
    
    (let [total-loss 
          (loop [step 0 total 0.0]
            (if (= step accum-steps)
              total
              (let [;; Срез (slice) микро-батча
                    xm-data (double-array (* micro-bs in-dim))
                    ym-data (double-array (* micro-bs out-dim))]
                ;; Копируем кусок данных
                (System/arraycopy xd (* step micro-bs in-dim) xm-data 0 (* micro-bs in-dim))
                (System/arraycopy yd (* step micro-bs out-dim) ym-data 0 (* micro-bs out-dim))
                
                (let [xm (t2/tensor (vec xm-data) [micro-bs in-dim])
                      ym (t2/tensor (vec ym-data) [micro-bs out-dim])
                      loss-step (accumulate-gradients! model xm ym accum-steps)]
                  (recur (inc step) (+ total loss-step))))))]
      ;; Все градиенты накоплены. Вызываем SGD.
      (let [flat-params (fw/all-params model)
            new-params (mapv (fn [p] (t2/sgd-step! p lr 5.0)) flat-params)
            new-model (fw/model-update model new-params)]
        [new-model (/ total-loss accum-steps)]))))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-gradient-accumulation []
  (println "==========================================")
  (println " Demo 35: Gradient Accumulation")
  (println "    (B=100) vs (b=10 ✕ 10 accum-steps)    ")
  (println "==========================================\n")
  
  (let [;; Создаем данные: y = 0.5 * x
        bs 100
        x-raw (mapv #(vector (float (/ % bs))) (range (- bs) bs))
        y-raw (mapv #(vector (float (* 0.5 (first %)))) x-raw)
        
        ;; Важно! Делаем шафл, чтобы микро-батчи были репрезентативными
        ;; (для чистоты эксперимента будем подавать их в том же псевдорандомном порядке)
        xy-pairs (shuffle (map vector x-raw y-raw))
        xs (mapv first xy-pairs)
        ys (mapv second xy-pairs)
        
        x (t2/tensor (flatten xs) [bs 1])
        y (t2/tensor (flatten ys) [bs 1])
        
        ;; Две идентичные модели
        model-standard (fw/make-model [1 1] 3)
        model-accum    (fw/make-model [1 1] 3)
        
        ;; Синхронизируем веса, чтобы стартовать с абсолютно идентичной точки
        params (fw/all-params model-standard)
        param-copies (mapv (fn [p]
                             (let [d (double-array (:numel p))]
                               (System/arraycopy (:data p) 0 d 0 (:numel p))
                               (t2/tensor (vec d) (:shape p))))
                           params)
        model-accum (fw/model-update model-accum param-copies)
        
        lr 0.1
        epochs 15]
    
    (println "Model 1: Single Large Batch (B=100)")
    (loop [ep 1
           m model-standard]
      (if (<= ep epochs)
        (let [pred (fw/model-forward m x)
              loss-raw (t2/mse-loss pred y)
              loss-val (aget ^doubles (:data loss-raw) 0)]
          (t2/backward! loss-raw)
          (let [new-p (mapv #(t2/sgd-step! % lr 5.0) (fw/all-params m))
                new-m (fw/model-update m new-p)]
            (when (zero? (mod ep 3))
              (println (format "  Epoch %2d | Loss: %8.6f" ep loss-val)))
            (recur (inc ep) new-m)))
        (println "  Done.\n")))
    
    (println "Model 2: Gradient Accumulation (b=10 ✕ 10 steps = 100)")
    (loop [ep 1
           m model-accum]
      (if (<= ep epochs)
        (let [[new-m avg-loss] (train-accumulate-step m x y 10 lr)]
          (when (zero? (mod ep 3))
            (println (format "  Epoch %2d | Loss: %8.6f" ep avg-loss)))
          (recur (inc ep) new-m))
        (println "  Done.\n")))
    
    (println "Notice: The loss trajectories should be perfectly identical!")
    (println "But Model 2 uses 10x less memory for intermediate tensors.")))
