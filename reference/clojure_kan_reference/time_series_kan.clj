(ns kan-kat.time-series-kan
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]
            [kan-kat.data-loader :as dl]))

;; ==========================================
;; Data Generation
;; ==========================================

(defn generate-time-series
  "Генерирует сложный синтетический временной ряд (Time Series).
   Включает: 
   1. Низкочастотную синусоиду (основной цикл).
   2. Высокочастотную синусоиду (сезонные колебания).
   3. Линейный тренд.
   4. Гауссов белый шум (опционально)."
  [N noise-level]
  (let [omega1 0.1
        omega2 0.5
        trend-k 0.05
        seq-data (double-array N)]
    (dotimes [t N]
      (let [val (+ (* 2.0 (Math/sin (* omega1 t)))
                   (* 0.5 (Math/sin (* omega2 t)))
                   (* trend-k t)
                   (* noise-level (- (rand) 0.5)))]
        (aset seq-data t val)))
    (vec seq-data)))

(defn create-sliding-windows
  "Разбивает одномерный вектор временного ряда на пары [X, Y].
   X - вектор из `window-size` шагов.
   Y - вектор из `horizon` шагов в будущем."
  [time-series window-size horizon]
  (let [N (count time-series)
        max-start (- N window-size horizon)
        dataset (vec (for [i (range max-start)]
                       (let [x-patch (subvec time-series i (+ i window-size))
                             y-patch (subvec time-series (+ i window-size) (+ i window-size horizon))
                             row (into {} (map-indexed (fn [idx val] [(str "x" idx) val]) x-patch))]
                         (into row (map-indexed (fn [idx val] [(str "y" idx) val]) y-patch)))))]
    dataset))

;; ==========================================
;; Demo 46: Time Series Forecasting
;; ==========================================

(defn demo-time-series-kan []
  (println "\n==========================================")
  (println " Demo 46: Time Series Forecasting (KAN)")
  (println "==========================================\n")
  
  (let [N 500
        window-size 10  ;; Смотрим на 10 шагов назад
        horizon 3       ;; Предсказываем 3 шага вперед
        epochs 150
        lr 0.05
        
        println (fn [& args] (apply clojure.core/println args))]
        
    (println (format "Generating Time Series (N=%d, Window=%d, Horizon=%d)..." N window-size horizon))
    (let [raw-series (generate-time-series N 0.2)
          dataset (create-sliding-windows raw-series window-size horizon)]
          
      ;; Не перемешиваем, чтобы хронология Train-Test была реалистичной.
      ;; Первые 80% времени - Train (Прошлое), последние 20% - Test (Будущее).
      (println "Splitting Past (Train) and Future (Test)...")
      (let [splits (dl/train-test-split dataset 0.2 false)
            train-data (:train splits)
            test-data (:test splits)
            
            x-features (mapv #(str "x" %) (range window-size))
            y-targets (mapv #(str "y" %) (range horizon))
            
            ;; Скалируем X и Y только по статистике Train, чтобы предотвратить заглядывание в будущее
            x-scaler (dl/fit-minmax train-data x-features)
            y-scaler (dl/fit-minmax train-data y-targets)
            
            train-x (dl/transform-minmax x-scaler train-data x-features)
            train-y (dl/transform-minmax y-scaler train-data y-targets)
            test-x (dl/transform-minmax x-scaler test-data x-features)
            test-y (dl/transform-minmax y-scaler test-data y-targets)
            
            X-train (dl/to-tensor train-x x-features)
            Y-train (dl/to-tensor train-y y-targets)
            X-test (dl/to-tensor test-x x-features)
            Y-test (dl/to-tensor test-y y-targets)
            
            ;; Архитектура: Вход [10] -> Скрытые [5] -> Выход [3]
            model (fw/make-model [window-size 5 horizon] 3)]
            
        (println (format "Train window count: %d | Test window count: %d" (count train-data) (count test-data)))
        (println "Training KAN...")
        
        (loop [ep 1
               curr-model model]
          (if (> ep epochs)
            curr-model
            (let [;; Форвард проход
                  preds-train (fw/model-forward curr-model X-train)
                  loss-t-train (t2/mse-loss preds-train Y-train)
                  
                  preds-test (fw/model-forward curr-model X-test)
                  loss-t-test (t2/mse-loss preds-test Y-test)
                  
                  loss-val-train (aget ^doubles (:data loss-t-train) 0)
                  loss-val-test (aget ^doubles (:data loss-t-test) 0)]
                  
              ;; Backprop только по прошлому (Train)
              (t2/backward! loss-t-train)
              (let [params (fw/all-params curr-model)
                    new-params (mapv #(t2/sgd-step! % lr 5.0) params)
                    new-model (fw/model-update curr-model new-params)]
                    
                (when (zero? (mod ep 25))
                  (println (format "  Epoch %3d | Train MSE: %.5f | Test MSE (Future): %.5f" 
                                   ep loss-val-train loss-val-test)))
                (recur (inc ep) new-model)))))))))
