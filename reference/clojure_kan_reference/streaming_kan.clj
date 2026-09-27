(ns kan-kat.streaming-kan
  (:require [clojure.core.async :as a :refer [>! <! >!! <!! go go-loop chan close! timeout]]
            [kan-kat.financial-timeseries-kan :as ft]
            [kan-kat.kan-framework :as fw]
            [kan-kat.tensor-v2 :as t2]
            [kan-kat.data-loader :as dl]))

;; ==============================================================================
;; 1. Producer: Имитатор Живого Рынка (Market Feed)
;; ==============================================================================

(defn start-market-feed
  "Симулирует потоковое поступление рыночных тиков/свечей с задержкой (simulating real-time).
   Берет сырой датасет, ждет `delay-ms` и кладет строку данных в канал `tick-chan`."
  [raw-data delay-ms tick-chan]
  (go
    (doseq [tick raw-data]
      (>! tick-chan tick)
      (<! (timeout delay-ms)))
    ;; Когда данные заканчиваются, закрываем канал (симулируя конец торговой сессии)
    (close! tick-chan)))

;; ==============================================================================
;; 2. Buffer: Скользящее Окно в реальном времени
;; ==============================================================================

(defn sliding-window-buffer
  "Слушает тики из `tick-chan`. Накапливает список последних `window-size` свечей.
   Каждый раз, когда приходит новый тик, сдвигает окно и пушит его в `window-chan`."
  [window-size tick-chan window-chan]
  (go-loop [window-buffer []]
    (let [tick (<! tick-chan)]
      (if (nil? tick)
        ;; Если tick-chan закрыт, закрываем дальше по пайплайну
        (close! window-chan)
        (let [new-buffer (conj window-buffer tick)]
          (if (< (count new-buffer) window-size)
            ;; Окно еще не заполнено (burn-in period), ждем дальше
            (recur new-buffer)
            ;; Окно заполнено. Отправляем в следующий канал, и отрезаем самую старую свечу
            (do
              (>! window-chan new-buffer)
              (recur (vec (rest new-buffer))))))))))

;; ==============================================================================
;; 3. Consumer: KAN Inference Worker
;; ==============================================================================

(defn kan-inference-worker
  "Слушает наполненные окна из `window-chan`. На лету скалирует их,
   пропускает через KAN модель и генерирует торговый сигнал (Buy/Sell) в `signal-chan`."
  [model x-scaler y-scaler input-features target-feature window-chan signal-chan]
  (go-loop []
    (let [window (<! window-chan)]
      (if (nil? window)
        (close! signal-chan)
        (let [;; 1. Flatten the window and extract required features (e.g. "close_0", "volume_0", etc)
              row-mapped (reduce (fn [acc i]
                                   (let [tick (nth window i)]
                                     (assoc acc 
                                            (str "close_" i) (get tick "close")
                                            (str "volume_" i) (get tick "volume"))))
                                 {}
                                 (range (count window)))
              
              ;; 2. Scale using pre-trained scaler
              scaled-row (first (dl/transform-minmax x-scaler [row-mapped] input-features))
              scaled-x (mapv #(get scaled-row %) input-features)
              X-input (t2/tensor scaled-x [1 (count input-features)])
              
              ;; 3. Forward Pass
              y-pred-tensor (fw/model-forward model X-input)
              y-pred-scaled (aget ^doubles (:data y-pred-tensor) 0)
              
              ;; 4. Unscale
              min-y (first (:mins y-scaler))
              max-y (first (:maxs y-scaler))
              y-pred (+ min-y (* y-pred-scaled (- max-y min-y)))
              
              ;; Current Price (last tick in window)
              current-price (get (last window) "close")
              
              ;; 5. Generate Signal
              signal (cond
                       (> y-pred (* current-price 1.01)) :buy
                       (< y-pred (* current-price 0.99)) :sell
                       :else :hold)]
          
          ;; Отправляем сигнал в канал с метаданными
          (>! signal-chan {:price current-price :predicted y-pred :signal signal})
          (recur))))))

;; ==============================================================================
;; 4. Demo 53: Live Market Simulation Pipeline
;; ==============================================================================

(defn demo-live-streaming []
  (println "\n==========================================")
  (println " Demo 53: Real-Time Incremental KAN Inference")
  (println "==========================================\n")
  
  (let [;; Сгенерируем 150 точек исторических данных "Рынка"
        days 150
        raw-ohlcv (ft/generate-metal-futures days 1500.0 0.0001 0.02)
        window-size 3
        
        ;; Подготовим данные для обучения суррогатной модели (чтобы было что стримить)
        features ["close" "volume"]
        input-features (vec (for [t (range window-size) f features] (str f "_" t)))
        target-feature "target_close"
        
        dataset (ft/create-multivariate-windows raw-ohlcv window-size features "close")
        train-size 100
        train-data (take train-size dataset)
        
        x-scaler (dl/fit-minmax train-data input-features)
        y-scaler (dl/fit-minmax train-data [target-feature])
        
        train-x (dl/transform-minmax x-scaler train-data input-features)
        train-y (dl/transform-minmax y-scaler train-data [target-feature])
        
        X-train (dl/to-tensor train-x input-features)
        Y-train (dl/to-tensor train-y [target-feature])
        
        kan-model (fw/make-model [(count input-features) 5 1] 3)
        lr 0.05
        epochs 50]
        
    ;; Обучаем быстро
    (println "Pre-training frozen KAN model for the worker...")
    (let [final-model (loop [ep 1 currkan kan-model]
                        (if (> ep epochs)
                          currkan
                          (let [preds (fw/model-forward currkan X-train)
                                loss-t (t2/mse-loss preds Y-train)
                                loss (aget ^doubles (:data loss-t) 0)]
                            (t2/backward! loss-t)
                            (recur (inc ep) (fw/model-update currkan (mapv #(t2/sgd-step! % lr 5.0) (fw/all-params currkan)))))))]
       
      (println "Model frozen. Initializing core.async Streaming Pipeline...\n")
      
      (let [;; 1. Создание неблокирующих каналов
            tick-chan (chan 10)     ; Канал свечей
            window-chan (chan 10)   ; Канал собранных окон
            signal-chan (chan 10)   ; Канал торговых сигналов
            
            ;; 2. Сымитируем "живые" данные, взяв только Out-of-sample часть (последние 47 дней)
            live-stream-data (vec (drop train-size raw-ohlcv))
            delay-ms 100 ; Симулируем приход нового тика каждые 100 миллисекунд
            ]
        
        ;; Запускаем конвейер
        (sliding-window-buffer window-size tick-chan window-chan)
        (kan-inference-worker final-model x-scaler y-scaler input-features target-feature window-chan signal-chan)
        (start-market-feed live-stream-data delay-ms tick-chan)
        
        ;; Слушаем финальный вывод в основном потоке (блокирующий <!!)
        (loop [ticks-processed 0]
          (let [signal-meta (<!! signal-chan)]
            (when signal-meta
              (let [{:keys [price predicted signal]} signal-meta]
                (println (format "[Live Stream] Received Signal: %-4s | Current Price: %.2f => KAN Pred: %.2f" 
                                 (name signal) price predicted)))
              (recur (inc ticks-processed)))))
              
        (println "\nMarket Session Closed. Pipeline gracefully shut down.")))))
