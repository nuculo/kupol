(ns kan-kat.financial-timeseries-kan
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]
            [kan-kat.data-loader :as dl]
            [kan-kat.technical-indicators :as ti]))

;; ==========================================
;; OHLCV Data Generation (GBM - Geometric Brownian Motion)
;; ==========================================

(defn generate-metal-futures
  "Генерирует симуляцию цены актива (например, фьючерса на Золото)
   в формате OHLCV (Open, High, Low, Close, Volume).
   Использует стохастический процесс Геометрического броуновского движения (GBM)."
  [days initial-price mu sigma]
  (let [dt 1.0  ;; 1 day
        ohlcv (object-array days)]
    (loop [i 0
           current-price initial-price]
      (if (= i days)
        (vec ohlcv)
        (let [;; Нормальное распределение через преобразование Бокса-Мюллера
              u1 (rand)
              u2 (rand)
              z (* (Math/sqrt (* -2.0 (Math/log (max 1e-10 u1))))
                   (Math/cos (* 2.0 Math/PI u2)))
              
              ;; Сдвиг цены по GBM: dS = S * (mu * dt + sigma * dZ)
              drift (* mu dt)
              shock (* sigma (Math/sqrt dt) z)
              next-close (* current-price (Math/exp (+ drift shock)))
              
              ;; Эмуляция волатильности внутри дня для High/Low
              noise-h (* current-price (Math/abs (* sigma 0.5 (rand))))
              noise-l (* current-price (Math/abs (* sigma 0.5 (rand))))
              
              day-open current-price
              day-close next-close
              day-high (max day-open day-close (+ (max day-open day-close) noise-h))
              day-low (min day-open day-close (- (min day-open day-close) noise-l))
              
              ;; Объем торгов коррелирует с рывками цены
              volatility-proxy (Math/abs (- day-close day-open))
              day-vol (long (* 1000 (+ 1.0 (* volatility-proxy 0.1) (rand))))]
              
          (aset ohlcv i {"open" day-open
                         "high" day-high
                         "low" day-low
                         "close" day-close
                         "volume" (double day-vol)})
          (recur (inc i) next-close))))))

;; ==========================================
;; Multivariate Sliding Windows
;; ==========================================

(defn create-multivariate-windows
  "Преобразует вектор хэш-мап (OHLCV) во фрейм данных [X, Y], где $X$ состоит из 
   `window-size` * `features` значений, а $Y$ это цель на `target-feature`."
  [ohlcv-data window-size features target-feature]
  (let [N (count ohlcv-data)
        max-start (- N window-size 1)]
    (vec (for [i (range max-start)]
           (let [window-slice (subvec ohlcv-data i (+ i window-size))
                 target-val (get (nth ohlcv-data (+ i window-size)) target-feature)
                 
                 ;; Уплощаем (flatten) окно: [o1 h1 l1 c1 v1, o2 h2 l2 ... ]
                 row (reduce (fn [acc [t day-map]]
                               (reduce (fn [acc2 feat]
                                         (assoc acc2 (str feat "_" t) (get day-map feat)))
                                       acc features))
                             {} (map-indexed vector window-slice))]
             (assoc row (str "target_" target-feature) target-val))))))

;; ==========================================
;; Financial Metrics
;; ==========================================

(defn directional-accuracy
  "Считает долю угаданных направлений рынка. 
   В трейдинге важно предсказать знак дельты P_t - P_{t-1}, а не только точную цену.
   `preds` и `targets` - тензоры v2. `prev-closes` - массив даблов с ценой закрытия предыдущего дня."
  [preds targets prev-closes]
  (let [N (nth (:shape preds) 0)
        ^doubles p-data (:data preds)
        ^doubles t-data (:data targets)
        correct (loop [i 0 c 0]
                  (if (= i N)
                    c
                    (let [prev-p (aget prev-closes i)
                          pred-p (aget p-data i)
                          true-p (aget t-data i)
                          pred-dir (if (> pred-p prev-p) 1.0 -1.0)
                          true-dir (if (> true-p prev-p) 1.0 -1.0)]
                      (if (= pred-dir true-dir)
                        (recur (inc i) (inc c))
                        (recur (inc i) c)))))]
    (/ (double correct) N)))

;; ==========================================
;; Demo 47: Metal Futures Forecasting
;; ==========================================

(defn demo-financial-kan []
  (println "\n==========================================")
  (println " Demo 47: Financial TS (Metal Futures)")
  (println "==========================================\n")
  
  (let [days 300
        ;; Параметры GBM. mu=0.0001 (слабый рост), sigma=0.015 (волатильность 1.5% в день)
        raw-ohlcv (generate-metal-futures days 1500.0 0.0001 0.015)
        
        window-size 3 ;; Прогнозируем на базе 3 дней
        features ["open" "high" "low" "close" "volume"]
        target "close"
        
        dataset (create-multivariate-windows raw-ohlcv window-size features target)
        
        ;; Train/Test Split (Хронологически)
        splits (dl/train-test-split dataset 0.2 false)
        train-data (:train splits)
        test-data (:test splits)
        
        input-features (vec (for [t (range window-size) f features] (str f "_" t)))
        target-feature (str "target_" target)
        
        ;; Формируем независимые Scalers для каждого признака
        x-scaler (dl/fit-minmax train-data input-features)
        y-scaler (dl/fit-minmax train-data [target-feature])
        
        train-x (dl/transform-minmax x-scaler train-data input-features)
        train-y (dl/transform-minmax y-scaler train-data [target-feature])
        test-x (dl/transform-minmax x-scaler test-data input-features)
        test-y (dl/transform-minmax y-scaler test-data [target-feature])
        
        X-train (dl/to-tensor train-x input-features)
        Y-train (dl/to-tensor train-y [target-feature])
        X-test (dl/to-tensor test-x input-features)
        Y-test (dl/to-tensor test-y [target-feature])
        
        ;; Важно для Directional Accuracy: извлечь реальные значения предыдущего t-1
        ;; Оно лежит в поле (dec window-size)
        extract-prev-close (fn [data]
                             (let [prev-key (str "close_" (dec window-size))]
                               (double-array (map #(get % prev-key) data))))
        prev-train (extract-prev-close train-x)
        prev-test (extract-prev-close test-x)
        
        ;; Архитектура: Вход (3 * 5 = 15 признаков) -> Скрытые [6] -> Выход [1]
        model (fw/make-model [(* window-size (count features)) 6 1] 3)
        lr 0.05
        epochs 100
        
        println (fn [& args] (apply clojure.core/println args))]
        
    (println (format "Simulated %d days of Metal Futures (OHLCV). Window Size: %d" days window-size))
    (println (format "Train count: %d | Test count: %d" (count train-data) (count test-data)))
    (println "Training KAN (Multivariate Output) ...")
    
    (loop [ep 1
           curr-model model]
      (if (> ep epochs)
        curr-model
        (let [preds-train (fw/model-forward curr-model X-train)
              loss-t-train (t2/mse-loss preds-train Y-train)
              
              preds-test (fw/model-forward curr-model X-test)
              loss-t-test (t2/mse-loss preds-test Y-test)
              
              loss-train (aget ^doubles (:data loss-t-train) 0)
              loss-test (aget ^doubles (:data loss-t-test) 0)
              
              ;; Метрика трендера (Acc)
              acc-train (directional-accuracy preds-train Y-train prev-train)
              acc-test (directional-accuracy preds-test Y-test prev-test)]
              
          (t2/backward! loss-t-train)
          (let [params (fw/all-params curr-model)
                new-params (mapv #(t2/sgd-step! % lr 5.0) params)
                new-model (fw/model-update curr-model new-params)]
                
            (when (zero? (mod ep 25))
              (println (format "  Epoch %3d | Train MSE: %.5f (DirAcc: %5.1f%%) | Test MSE: %.5f (DirAcc: %5.1f%%)" 
                               ep loss-train (* 100.0 acc-train) loss-test (* 100.0 acc-test))))
            (recur (inc ep) new-model)))))))

;; ==========================================
;; Demo 48: Technical Indicators KAN
;; ==========================================

(defn demo-technical-indicators-kan []
  (println "\n==========================================")
  (println " Demo 48: Technical Indicators KAN (Metal Futures)")
  (println "==========================================\n")
  
  (let [days 350
        raw-ohlcv (generate-metal-futures days 1500.0 0.0001 0.015)
        closes (vec (map #(get % "close") raw-ohlcv))
        
        ;; Feature Engineering (Технический Анализ)
        ;; Вычисляем индикаторы для всего датасета
        rsi-14 (ti/rsi closes 14)
        ema-20 (ti/ema closes 20)
        
        ;; Функция безопасного извлечения значений индикаторов (nil -> 0.0 для стабильности)
        safe-val (fn [v default-val] (if (nil? v) default-val (double v)))
        
        ;; Мы обогащаем каждый словарь OHLCV новыми вычисленными признаками
        enriched-ohlcv (vec (for [i (range days)]
                              (let [day (nth raw-ohlcv i)]
                                (assoc day 
                                       "rsi" (safe-val (nth rsi-14 i) 50.0)
                                       "ema" (safe-val (nth ema-20 i) (get day "close"))))))
                                       
        window-size 3 ;; Снова смотрим на 3 исторических дня
        ;; Передаем расширенные фичи (7 фич: O H L C V RSI EMA)
        features ["open" "high" "low" "close" "volume" "rsi" "ema"]
        target "close"
        
        dataset (create-multivariate-windows enriched-ohlcv window-size features target)
        
        ;; Train/Test Split
        splits (dl/train-test-split dataset 0.2 false)
        train-data (:train splits)
        test-data (:test splits)
        
        input-features (vec (for [t (range window-size) f features] (str f "_" t)))
        target-feature (str "target_" target)
        
        x-scaler (dl/fit-minmax train-data input-features)
        y-scaler (dl/fit-minmax train-data [target-feature])
        
        train-x (dl/transform-minmax x-scaler train-data input-features)
        train-y (dl/transform-minmax y-scaler train-data [target-feature])
        test-x (dl/transform-minmax x-scaler test-data input-features)
        test-y (dl/transform-minmax y-scaler test-data [target-feature])
        
        X-train (dl/to-tensor train-x input-features)
        Y-train (dl/to-tensor train-y [target-feature])
        X-test (dl/to-tensor test-x input-features)
        Y-test (dl/to-tensor test-y [target-feature])
        
        extract-prev-close (fn [data]
                             (let [prev-key (str "close_" (dec window-size))]
                               (double-array (map #(get % prev-key) data))))
        prev-train (extract-prev-close train-x)
        prev-test (extract-prev-close test-x)
        
        ;; Архитектура: Вход (3 * 7 = 21 признак) -> Скрытые [10] -> Выход [1]
        model (fw/make-model [(* window-size (count features)) 10 1] 3)
        lr 0.03
        epochs 100
        
        println (fn [& args] (apply clojure.core/println args))]
        
    (println (format "Extracted Technical Indicators (RSI, EMA) for %d days." days))
    (println (format "Features per window: %d. Train count: %d | Test count: %d" 
                     (* window-size (count features)) (count train-data) (count test-data)))
    (println "Training KAN on enriched Technical Data ...")
    
    (loop [ep 1
           curr-model model]
      (if (> ep epochs)
        curr-model
        (let [preds-train (fw/model-forward curr-model X-train)
              loss-t-train (t2/mse-loss preds-train Y-train)
              
              preds-test (fw/model-forward curr-model X-test)
              loss-t-test (t2/mse-loss preds-test Y-test)
              
              loss-train (aget ^doubles (:data loss-t-train) 0)
              loss-test (aget ^doubles (:data loss-t-test) 0)
              
              acc-train (directional-accuracy preds-train Y-train prev-train)
              acc-test (directional-accuracy preds-test Y-test prev-test)]
              
          ;; Мы минимизируем кросс-энтропийный Loss в классификации и MSE в регрессии
          (t2/backward! loss-t-train)
          (let [params (fw/all-params curr-model)
                new-params (mapv #(t2/sgd-step! % lr 5.0) params)
                new-model (fw/model-update curr-model new-params)]
                
            (when (zero? (mod ep 25))
              (println (format "  Epoch %3d | Train MSE: %.5f (DirAcc: %5.1f%%) | Test MSE: %.5f (DirAcc: %5.1f%%)" 
                               ep loss-train (* 100.0 acc-train) loss-test (* 100.0 acc-test))))
            (recur (inc ep) new-model)))))))

;; ==========================================
;; Demo 49: Extended Technical Indicators (Holistic KAN)
;; ==========================================

(defn demo-extended-indicators-kan []
  (require '[kan-kat.technical-indicators :as ti])
  (println "\n==========================================")
  (println " Demo 49: Extended Indicators (Holistic Financial KAN)")
  (println "==========================================\n")
  
  (let [days 350
        raw-ohlcv (generate-metal-futures days 1500.0 0.0001 0.02)
        closes (vec (map #(get % "close") raw-ohlcv))
        
        ;; Feature Engineering (Полная Картина От Уолл-Стрит)
        rsi-14 (ti/rsi closes 14)
        ema-20 (ti/ema closes 20)
        macd-data (ti/macd closes 12 26 9)
        bb-data (ti/bollinger-bands closes 20 2.0)
        atr-14 (ti/atr raw-ohlcv 14)
        stoch-data (ti/stochastic-oscillator raw-ohlcv 14 3)
        williams-14 (ti/williams-r raw-ohlcv 14)
        roc-10 (ti/roc closes 10)
        obv-data (ti/obv raw-ohlcv)
        vwap-data (ti/vwap raw-ohlcv)
        
        safe-val (fn [v default-val] (if (nil? v) default-val (double v)))
        
        enriched-ohlcv 
        (vec (for [i (range days)]
               (let [day (nth raw-ohlcv i)
                     c (get day "close")]
                 (assoc day 
                        "rsi" (safe-val (nth rsi-14 i) 50.0)
                        "ema" (safe-val (nth ema-20 i) c)
                        "macd" (safe-val (:macd (nth macd-data i)) 0.0)
                        "bb_upper" (safe-val (:high (nth bb-data i)) c)
                        "bb_lower" (safe-val (:low (nth bb-data i)) c)
                        "atr" (safe-val (nth atr-14 i) 0.0)
                        "stoch_k" (safe-val (:k (nth stoch-data i)) 50.0)
                        "williams" (safe-val (nth williams-14 i) -50.0)
                        "roc" (safe-val (nth roc-10 i) 0.0)
                        "obv" (safe-val (nth obv-data i) 0.0)
                        "vwap" (safe-val (nth vwap-data i) c)))))
                                       
        window-size 3
        features ["open" "high" "low" "close" "volume" 
                  "rsi" "ema" "macd" "bb_upper" "bb_lower" 
                  "atr" "stoch_k" "williams" "roc" "obv" "vwap"]
        target "close"
        
        dataset (create-multivariate-windows enriched-ohlcv window-size features target)
        
        splits (dl/train-test-split dataset 0.2 false)
        train-data (:train splits)
        test-data (:test splits)
        
        input-features (vec (for [t (range window-size) f features] (str f "_" t)))
        target-feature (str "target_" target)
        
        x-scaler (dl/fit-minmax train-data input-features)
        y-scaler (dl/fit-minmax train-data [target-feature])
        
        train-x (dl/transform-minmax x-scaler train-data input-features)
        train-y (dl/transform-minmax y-scaler train-data [target-feature])
        test-x (dl/transform-minmax x-scaler test-data input-features)
        test-y (dl/transform-minmax y-scaler test-data [target-feature])
        
        X-train (dl/to-tensor train-x input-features)
        Y-train (dl/to-tensor train-y [target-feature])
        X-test (dl/to-tensor test-x input-features)
        Y-test (dl/to-tensor test-y [target-feature])
        
        extract-prev-close (fn [data]
                             (let [prev-key (str "close_" (dec window-size))]
                               (double-array (map #(get % prev-key) data))))
        prev-train (extract-prev-close train-x)
        prev-test (extract-prev-close test-x)
        
        ;; Архитектура: Вход (3 * 16 = 48 признаков) -> Скрытые [15] -> Выход [1]
        model (fw/make-model [(* window-size (count features)) 15 1] 3)
        lr 0.03
        epochs 100
        
        println (fn [& args] (apply clojure.core/println args))]
        
    (println (format "Extracted 11 Advanced Technical Indicators for %d days." days))
    (println (format "Features per window: %d. Train count: %d | Test count: %d" 
                     (* window-size (count features)) (count train-data) (count test-data)))
    (println "Training KAN on Holistic Financial Data ...")
    
    (loop [ep 1
           curr-model model]
      (if (> ep epochs)
        curr-model
        (let [preds-train (fw/model-forward curr-model X-train)
              loss-t-train (t2/mse-loss preds-train Y-train)
              
              preds-test (fw/model-forward curr-model X-test)
              loss-t-test (t2/mse-loss preds-test Y-test)
              
              loss-train (aget ^doubles (:data loss-t-train) 0)
              loss-test (aget ^doubles (:data loss-t-test) 0)
              
              acc-train (directional-accuracy preds-train Y-train prev-train)
              acc-test (directional-accuracy preds-test Y-test prev-test)]
              
          (t2/backward! loss-t-train)
          (let [params (fw/all-params curr-model)
                new-params (mapv #(t2/sgd-step! % lr 5.0) params)
                new-model (fw/model-update curr-model new-params)]
                
            (when (zero? (mod ep 25))
              (println (format "  Epoch %3d | Train MSE: %.5f (DirAcc: %5.1f%%) | Test MSE: %.5f (DirAcc: %5.1f%%)" 
                               ep loss-train (* 100.0 acc-train) loss-test (* 100.0 acc-test))))
            (recur (inc ep) new-model)))))))

;; ==========================================
;; Demo 50: Advanced Trading Analytics (Regime-Aware KAN)
;; ==========================================

(defn demo-advanced-analytics-kan []
  (require '[kan-kat.technical-indicators :as ti])
  (println "\n==========================================")
  (println " Demo 50: Advanced Trading Analytics (Smart Money KAN)")
  (println "==========================================\n")
  
  (let [days 350
        raw-ohlcv (generate-metal-futures days 1500.0 0.0001 0.02)
        closes (vec (map #(get % "close") raw-ohlcv))
        
        ;; 1. Вычисляем только "Умные" фичи из Фазы 53.4
        rsi-14 (ti/rsi closes 14)
        adx-14 (ti/adx raw-ohlcv 14)
        hma-20 (ti/hma closes 20)
        entropy-10 (ti/shannon-entropy closes 10 5)
        
        safe-val (fn [v default-val] (if (nil? v) default-val (double v)))
        
        ;; 2. Базовое обогащение для передачи в Crossover генератор
        base-enriched 
        (vec (for [i (range days)]
               (let [day (nth raw-ohlcv i)
                     c (get day "close")]
                 (assoc day 
                        "rsi" (safe-val (nth rsi-14 i) 50.0)
                        "adx" (safe-val (nth adx-14 i) 20.0)
                        "hma" (safe-val (nth hma-20 i) c)
                        "entropy" (safe-val (nth entropy-10 i) 0.0)))))
                        
        ;; 3. Генерируем сигнальные флаги (Crossovers)
        sig-enriched (ti/extract-crossover-signals base-enriched 10 20)
                                       
        window-size 3
        ;; Вместо 16 фич как в Demo 49 (Overfitting), 
        ;; мы концентрируем умный контекст в 5 "мета-фич" + Close.
        features ["close" "adx" "hma" "entropy" "signal_cross" "signal_rsi" "regime_trend"]
        target "close"
        
        dataset (create-multivariate-windows sig-enriched window-size features target)
        
        splits (dl/train-test-split dataset 0.2 false)
        train-data (:train splits)
        test-data (:test splits)
        
        input-features (vec (for [t (range window-size) f features] (str f "_" t)))
        target-feature (str "target_" target)
        
        x-scaler (dl/fit-minmax train-data input-features)
        y-scaler (dl/fit-minmax train-data [target-feature])
        
        train-x (dl/transform-minmax x-scaler train-data input-features)
        train-y (dl/transform-minmax y-scaler train-data [target-feature])
        test-x (dl/transform-minmax x-scaler test-data input-features)
        test-y (dl/transform-minmax y-scaler test-data [target-feature])
        
        X-train (dl/to-tensor train-x input-features)
        Y-train (dl/to-tensor train-y [target-feature])
        X-test (dl/to-tensor test-x input-features)
        Y-test (dl/to-tensor test-y [target-feature])
        
        extract-prev-close (fn [data]
                             (let [prev-key (str "close_" (dec window-size))]
                               (double-array (map #(get % prev-key) data))))
        prev-train (extract-prev-close train-x)
        prev-test (extract-prev-close test-x)
        
        ;; Архитектура: Вход (3 * 7 = 21 признак) -> Скрытые [8] -> Выход [1]
        model (fw/make-model [(* window-size (count features)) 8 1] 3)
        lr 0.03
        epochs 100
        
        println (fn [& args] (apply clojure.core/println args))]
        
    (println "Extracted Regime-Aware Indicators (ADX, HMA, Entropy, Crossovers).")
    (println (format "Features per window reduced to: %d. Train count: %d | Test count: %d" 
                     (* window-size (count features)) (count train-data) (count test-data)))
    (println "Training Smart KAN (Avoiding Dimension Curse) ...")
    
    (loop [ep 1
           curr-model model]
      (if (> ep epochs)
        curr-model
        (let [preds-train (fw/model-forward curr-model X-train)
              loss-t-train (t2/mse-loss preds-train Y-train)
              
              preds-test (fw/model-forward curr-model X-test)
              loss-t-test (t2/mse-loss preds-test Y-test)
              
              loss-train (aget ^doubles (:data loss-t-train) 0)
              loss-test (aget ^doubles (:data loss-t-test) 0)
              
              acc-train (directional-accuracy preds-train Y-train prev-train)
              acc-test (directional-accuracy preds-test Y-test prev-test)]
              
          (t2/backward! loss-t-train)
          (let [params (fw/all-params curr-model)
                new-params (mapv #(t2/sgd-step! % lr 5.0) params)
                new-model (fw/model-update curr-model new-params)]
                
            (when (zero? (mod ep 25))
              (println (format "  Epoch %3d | Train MSE: %.5f (DirAcc: %5.1f%%) | Test MSE: %.5f (DirAcc: %5.1f%%)" 
                               ep loss-train (* 100.0 acc-train) loss-test (* 100.0 acc-test))))
            (recur (inc ep) new-model)))))))

;; ==========================================
;; Demo 51: Multi-Head KAN with Candlestick Patterns
;; ==========================================

(defn demo-candlestick-multihead-kan []
  (println "\n==========================================")
  (println " Demo 51: Multi-Head KAN + 61 Candlesticks")
  (println "==========================================\n")
  
  (let [ohlcv-history (generate-metal-futures 400 1500.0 0.0001 0.02)
        ;; Извлекаем 61 паттерн для каждого дня (O(1) по каждому барру)
        cdl-features ((requiring-resolve 'kan-kat.candlestick-patterns/extract-features) ohlcv-history)
        
        N (count ohlcv-history)
        window-size 3
        horizon 1
        
        ;; Формируем dataset
        dataset (for [i (range (- N window-size horizon))]
                  (let [window (subvec ohlcv-history i (+ i window-size))
                        cdl-window (subvec cdl-features i (+ i window-size))
                        
                        ;; Сливаем базовые OHLCV и 61 CDL фичу
                        x-vec (vec (mapcat (fn [d cdl-vec]
                                             (concat [(get d "close") (get d "volume")]
                                                     cdl-vec))
                                           window cdl-window))
                        y-val (get (nth ohlcv-history (+ i window-size horizon -1)) "close")]
                    {:x x-vec :y [y-val]}))
                    
        train-size (int (* 0.8 (count dataset)))
        train-data (take train-size dataset)
        test-data (drop train-size dataset)
        
        input-dim (count (:x (first dataset)))
        
        ;; Нормализация
        all-xs (mapv :x train-data)
        x-mins (vec (apply mapv min all-xs))
        x-maxs (vec (apply mapv max all-xs))
        x-scaler {:mins x-mins :maxs x-maxs}
        
        y-mins [(apply min (map (comp first :y) train-data))]
        y-maxs [(apply max (map (comp first :y) train-data))]
        y-scaler {:mins y-mins :maxs y-maxs}
        
        scale-fn (fn [v scaler]
                   (mapv (fn [val min-v max-v]
                           (if (= (double min-v) (double max-v)) 0.5 (/ (- val min-v) (- max-v min-v))))
                         v (:mins scaler) (:maxs scaler)))
                         
        train-x (mapv #(scale-fn (:x %) x-scaler) train-data)
        train-y (mapv #(scale-fn (:y %) y-scaler) train-data)
        test-x (mapv #(scale-fn (:x %) x-scaler) test-data)
        test-y (mapv #(scale-fn (:y %) y-scaler) test-data)
        
        X-train (t2/tensor (vec (mapcat identity train-x)) [train-size input-dim])
        Y-train (t2/tensor (vec (mapcat identity train-y)) [train-size 1])
        X-test (t2/tensor (vec (mapcat identity test-x)) [(count test-data) input-dim])
        Y-test (t2/tensor (vec (mapcat identity test-y)) [(count test-data) 1])
        
        prev-train (double-array (mapv #(get (nth ohlcv-history (+ % window-size -1)) "close") (range train-size)))
        prev-test (double-array (mapv #(get (nth ohlcv-history (+ % train-size window-size -1)) "close") (range (count test-data))))
        
        ;; Multi-Head: 4 головы бьются над огромным вектором
        num-heads 4
        head-dim 4
        mhk-model ((requiring-resolve 'kan-kat.multi-head-kan/make-multi-head-kan) num-heads input-dim head-dim 1 3)
        
        mhk-forward (requiring-resolve 'kan-kat.multi-head-kan/mhk-forward)
        mhk-params (requiring-resolve 'kan-kat.multi-head-kan/mhk-params)
        mhk-update (requiring-resolve 'kan-kat.multi-head-kan/mhk-update)
        
        epochs 150
        lr 0.05]
        
    (println (format "Dataset: %d features/window (2 Base + 61 CDL) * %d days = %d inputs" 
                     63 window-size input-dim))
    (println (format "Multi-Head Architecture: %d Heads -> %d concat -> 1 output" num-heads (* num-heads head-dim)))
    
    (loop [ep 1
           curr-mhk mhk-model]
      (if (> ep epochs)
        curr-mhk
        (let [preds-train (mhk-forward curr-mhk X-train)
              loss-t-train (t2/mse-loss preds-train Y-train)
              
              preds-test (mhk-forward curr-mhk X-test)
              loss-t-test (t2/mse-loss preds-test Y-test)
              
              loss-train (aget ^doubles (:data loss-t-train) 0)
              loss-test (aget ^doubles (:data loss-t-test) 0)
              
              acc-train (directional-accuracy preds-train Y-train prev-train)
              acc-test (directional-accuracy preds-test Y-test prev-test)]
              
          (t2/backward! loss-t-train)
          (let [params (mhk-params curr-mhk)
                new-params (mapv #(t2/sgd-step! % lr 5.0) params)
                new-mhk (mhk-update curr-mhk new-params)]
                
            (when (zero? (mod ep 25))
              (println (format "  Epoch %3d | Train MSE: %.5f (DirAcc: %5.1f%%) | Test MSE: %.5f (DirAcc: %5.1f%%)" 
                               ep loss-train (* 100.0 acc-train) loss-test (* 100.0 acc-test))))
            (recur (inc ep) new-mhk)))))))

;; ============================================================
;; Demo 54: Phase 55 - Babylonian 60-Head KAN (A/B Test)
;; ============================================================

(defn demo-babylonian-kan []
  (println "==========================================")
  (println " Demo 54: Babylonian 60-Head KAN vs 1-Head KAN")
  (println "==========================================")
  
  (let [extract-cdl-fn (requiring-resolve 'kan-kat.candlestick-patterns/extract-features)
        all-cdl-names @(requiring-resolve 'kan-kat.candlestick-patterns/all-cdl-patterns)
        ;; 1. Подготовим ровно 60 признаков для чистоты эксперимента
        ;; Базовые (5): Open, High, Low, Close, Volume
        ;; Индикаторы (15): SMA, EMA, WMA, HMA, DEMA, TEMA, KAMA, RSI, MACD, MACD_Signal, MACD_Hist, Bollinger_U, Bollinger_L, ATR, VWAP
        ;; Свечные паттерны (40 избранных):
        base-features ["open" "high" "low" "close" "volume"]
        indicator-features ["sma" "ema" "wma" "hma" "dema" "tema" "kama" 
                            "rsi" "macd-line" "macd-signal" "macd-hist" 
                            "bb-upper" "bb-lower" "atr" "vwap"]
        cdl-features ["CDL2CROWS" "CDL3BLACKCROWS" "CDL3INSIDE" "CDL3LINESTRIKE" "CDL3OUTSIDE" "CDL3STARSINSOUTH" 
                      "CDL3WHITESOLDIERS" "CDLABANDONEDBABY" "CDLADVANCEBLOCK" "CDLBELTHOLD" "CDLBREAKAWAY" 
                      "CDLCLOSINGMARUBOZU" "CDLCONCEALBABYSWALL" "CDLCOUNTERATTACK" "CDLDARKCLOUDCOVER" "CDLDOJI" 
                      "CDLDOJISTAR" "CDLDRAGONFLYDOJI" "CDLENGULFING" "CDLEVENINGDOJISTAR" "CDLEVENINGSTAR" 
                      "CDLGAPSIDESIDEWHITE" "CDLGRAVESTONEDOJI" "CDLHAMMER" "CDLHANGINGMAN" "CDLHARAMI" 
                      "CDLHARAMICROSS" "CDLHIGHWAVE" "CDLHIKKAKE" "CDLHIKKAKEMOD" "CDLHOMINGPIGEON" 
                      "CDLIDENTICAL3CROWS" "CDLINNECK" "CDLINVERTEDHAMMER" "CDLKICKING" "CDLKICKINGBYLENGTH" 
                      "CDLLADDERBOTTOM" "CDLLONGLEGGEDDOJI" "CDLLONGLINE" "CDLMARUBOZU"]
                      
        days 300
        raw-ohlcv (generate-metal-futures days 1500.0 0.0001 0.02)
        raw-cdls (extract-cdl-fn raw-ohlcv)
        ohlcv-history (vec (for [t (range days)]
                             (let [row (nth raw-ohlcv t)
                                   h-slice (take (inc t) raw-ohlcv)
                                   h-close (vec (map #(get % "close") h-slice))
                                   cdl-map (zipmap all-cdl-names (nth raw-cdls t))]
                               (merge row
                                      cdl-map
                                      {"sma" (last (kan-kat.technical-indicators/sma h-close 10))
                                       "ema" (last (kan-kat.technical-indicators/ema h-close 10))
                                       "wma" (last (kan-kat.technical-indicators/wma h-close 10))
                                       "hma" (last (kan-kat.technical-indicators/hma h-close 10))
                                       "dema" (last (kan-kat.technical-indicators/dema h-close 10))
                                       "tema" (last (kan-kat.technical-indicators/tema h-close 10))
                                       "kama" (last (kan-kat.technical-indicators/kama h-close 10))
                                       "rsi" (last (kan-kat.technical-indicators/rsi h-close 14))}
                                       (let [m (last (kan-kat.technical-indicators/macd h-close 12 26 9))]
                                         (if m {"macd-line" (:macd m) "macd-signal" (:signal m) "macd-hist" (:hist m)}
                                               {"macd-line" nil "macd-signal" nil "macd-hist" nil}))
                                       (let [bb (last (kan-kat.technical-indicators/bollinger-bands h-close 20 2.0))]
                                         (if bb {"bb-upper" (:high bb) "bb-lower" (:low bb)}
                                                {"bb-upper" nil "bb-lower" nil}))
                                      {"atr" (last (kan-kat.technical-indicators/atr h-slice 14))
                                       "vwap" (last (kan-kat.technical-indicators/vwap h-slice))}))))
        
        ;; Отбрасываем первые 35 дней, чтобы прогрелись индикаторы
        cleaned-data (vec (take 10 (drop 35 ohlcv-history)))
        
        ;; Собираем все 60 фичей
        all-60-features (vec (concat base-features indicator-features cdl-features))
        _ (assert (= 60 (count all-60-features)) "Must be exactly 60 features for the Babylonian architecture.")
        
        window-size 1
        dataset (create-multivariate-windows cleaned-data window-size all-60-features "close")
        
        splits (dl/train-test-split dataset 0.2 false)
        train-data (:train splits)
        test-data (:test splits)
        train-size (count train-data)
        
        input-features (vec (for [t (range window-size) f all-60-features] (str f "_" t)))
        
        x-scaler (dl/fit-minmax train-data input-features)
        y-scaler (dl/fit-minmax train-data ["target_close"])
        
        train-x (dl/transform-minmax x-scaler train-data input-features)
        train-y (dl/transform-minmax y-scaler train-data ["target_close"])
        test-x (dl/transform-minmax x-scaler test-data input-features)
        test-y (dl/transform-minmax y-scaler test-data ["target_close"])
        
        input-dim 60
        X-train (dl/to-tensor train-x input-features)
        Y-train (dl/to-tensor train-y ["target_close"])
        X-test (dl/to-tensor test-x input-features)
        Y-test (dl/to-tensor test-y ["target_close"])
        
        prev-train (double-array (mapv #(get (nth cleaned-data %) "close") (range train-size)))
        prev-test (double-array (mapv #(get (nth cleaned-data (+ % train-size)) "close") (range (count test-data))))
        
        ;; Dynamic resolving to avoid cyclic deps if any
        mhk-make (requiring-resolve 'kan-kat.multi-head-kan/make-multi-head-kan)
        mhk-forward (requiring-resolve 'kan-kat.multi-head-kan/mhk-forward)
        mhk-params (requiring-resolve 'kan-kat.multi-head-kan/mhk-params)
        mhk-update (requiring-resolve 'kan-kat.multi-head-kan/mhk-update)]
        
    (println (format "Dataset: %d rows. Training on %d features (no windowing)." (count cleaned-data) input-dim))
    (println "Starting A/B Comparative Training (50 epochs each)...\n")
    
    (let [babylonian-forward (fn [mhk X]
                               (let [heads (:heads mhk)
                                     N (count heads)]
                                 (if (= N 1)
                                   (mhk-forward mhk X)
                                   (let [batch-size (first (:shape X))
                                         ^doubles data (:data X)
                                         x-cols (vec (for [i (range N)]
                                                       (let [col-data (double-array batch-size)]
                                                         (dotimes [b batch-size]
                                                           (aset col-data b (aget data (+ (* b N) i))))
                                                         (t2/tensor col-data [batch-size 1]))))
                                         head-outputs (mapv (fn [head col-x] (fw/model-forward head col-x)) heads x-cols)
                                         cat-output (reduce t2/t-cat head-outputs)]
                                     (fw/model-forward (:o-kan mhk) cat-output)))))
          ;; MODEL A: 1 Широкая Голова (60 входов -> 180 скрытых -> 1 выход)
          model-a (mhk-make 1 60 180 1 3)
          ;; MODEL B: 60 Узких Голов (по 1 входу -> 3 скрытых -> 1 выход для каждой)
          model-b (mhk-make 60 1 3 1 3)
          lr 0.05
          epochs 2]
          
      (println "[Model A] 1 Wide Head (In: 60, Hidden: 180)")
      (let [start-a (System/nanoTime)
            final-a (loop [ep 1 curr model-a]
                      (if (> ep epochs)
                        curr
                        (let [preds (mhk-forward curr X-train)
                              loss-t (t2/mse-loss preds Y-train)
                              loss (aget ^doubles (:data loss-t) 0)]
                          (t2/backward! loss-t)
                          (let [new-mhk (mhk-update curr (mapv #(t2/sgd-step! % lr 5.0) (mhk-params curr)))]
                            (when true
                              (let [test-loss (aget ^doubles (:data (t2/mse-loss (mhk-forward curr X-test) Y-test)) 0)]
                                (println (format "  Ep %2d | Train Loss: %.4f | Test Loss: %.4f" ep loss test-loss))))
                            (recur (inc ep) new-mhk)))))
            end-a (System/nanoTime)
            time-a (/ (- end-a start-a) 1e9)]
            
        (println (format "Model A finished in %.2f seconds (%.3f s/epoch)" time-a (/ time-a epochs)))
        
        (println "\n[Model B] 60 Narrow Heads (In: 1, Hidden: 3 per head)")
        (let [start-b (System/nanoTime)
              final-b (loop [ep 1 curr model-b]
                        (if (> ep epochs)
                          curr
                          (let [preds (babylonian-forward curr X-train)
                                loss-t (t2/mse-loss preds Y-train)
                                loss (aget ^doubles (:data loss-t) 0)]
                            (t2/backward! loss-t)
                            (let [new-mhk (mhk-update curr (mapv #(t2/sgd-step! % lr 5.0) (mhk-params curr)))]
                              (when true
                                (let [test-loss (aget ^doubles (:data (t2/mse-loss (babylonian-forward curr X-test) Y-test)) 0)]
                                  (println (format "  Ep %2d | Train Loss: %.4f | Test Loss: %.4f" ep loss test-loss))))
                              (recur (inc ep) new-mhk)))))
              end-b (System/nanoTime)
              time-b (/ (- end-b start-b) 1e9)]
              
          (println (format "Model B finished in %.2f seconds (%.3f s/epoch)" time-b (/ time-b epochs)))
          
          (println "\n--- A/B Comparison ---")
          (println (format "Model A Time: %.2fs" time-a))
          (println (format "Model B Time: %.2fs (%.1fx Speedup!)" time-b (/ time-a time-b)))
          (println "==========================================\n"))))))

(defn demo-benchmark-ml []
  (println "\n==========================================")
  (println " Demo 55: KAN vs Random Forest Benchmark")
  (println "==========================================\n")
  
  (let [extract-cdl-fn (requiring-resolve 'kan-kat.candlestick-patterns/extract-features)
        all-cdl-names @(requiring-resolve 'kan-kat.candlestick-patterns/all-cdl-patterns)
        ;; Набор как в Babylonian: 60 фичей
        base-features ["open" "high" "low" "close" "volume"]
        indicator-features ["sma" "ema" "wma" "hma" "dema" "tema" "kama" 
                            "rsi" "macd-line" "macd-signal" "macd-hist" 
                            "bb-upper" "bb-lower" "atr" "vwap"]
        cdl-features ["CDL2CROWS" "CDL3BLACKCROWS" "CDL3INSIDE" "CDL3LINESTRIKE" "CDL3OUTSIDE" "CDL3STARSINSOUTH" 
                      "CDL3WHITESOLDIERS" "CDLABANDONEDBABY" "CDLADVANCEBLOCK" "CDLBELTHOLD" "CDLBREAKAWAY" 
                      "CDLCLOSINGMARUBOZU" "CDLCONCEALBABYSWALL" "CDLCOUNTERATTACK" "CDLDARKCLOUDCOVER" "CDLDOJI" 
                      "CDLDOJISTAR" "CDLDRAGONFLYDOJI" "CDLENGULFING" "CDLEVENINGDOJISTAR" "CDLEVENINGSTAR" 
                      "CDLGAPSIDESIDEWHITE" "CDLGRAVESTONEDOJI" "CDLHAMMER" "CDLHANGINGMAN" "CDLHARAMI" 
                      "CDLHARAMICROSS" "CDLHIGHWAVE" "CDLHIKKAKE" "CDLHIKKAKEMOD" "CDLHOMINGPIGEON" 
                      "CDLIDENTICAL3CROWS" "CDLINNECK" "CDLINVERTEDHAMMER" "CDLKICKING" "CDLKICKINGBYLENGTH" 
                      "CDLLADDERBOTTOM" "CDLLONGLEGGEDDOJI" "CDLLONGLINE" "CDLMARUBOZU"]
                      
        days 300
        raw-ohlcv (generate-metal-futures days 1500.0 0.0001 0.02)
        raw-cdls (extract-cdl-fn raw-ohlcv)
        ohlcv-history (vec (for [t (range days)]
                             (let [row (nth raw-ohlcv t)
                                   h-slice (take (inc t) raw-ohlcv)
                                   h-close (vec (map #(get % "close") h-slice))
                                   cdl-map (zipmap all-cdl-names (nth raw-cdls t))]
                               (merge row
                                      cdl-map
                                      {"sma" (last (kan-kat.technical-indicators/sma h-close 10))
                                       "ema" (last (kan-kat.technical-indicators/ema h-close 10))
                                       "wma" (last (kan-kat.technical-indicators/wma h-close 10))
                                       "hma" (last (kan-kat.technical-indicators/hma h-close 10))
                                       "dema" (last (kan-kat.technical-indicators/dema h-close 10))
                                       "tema" (last (kan-kat.technical-indicators/tema h-close 10))
                                       "kama" (last (kan-kat.technical-indicators/kama h-close 10))
                                       "rsi" (last (kan-kat.technical-indicators/rsi h-close 14))}
                                       (let [m (last (kan-kat.technical-indicators/macd h-close 12 26 9))]
                                         (if m {"macd-line" (:macd m) "macd-signal" (:signal m) "macd-hist" (:hist m)}
                                               {"macd-line" nil "macd-signal" nil "macd-hist" nil}))
                                       (let [bb (last (kan-kat.technical-indicators/bollinger-bands h-close 20 2.0))]
                                         (if bb {"bb-upper" (:high bb) "bb-lower" (:low bb)}
                                                {"bb-upper" nil "bb-lower" nil}))
                                      {"atr" (last (kan-kat.technical-indicators/atr h-slice 14))
                                       "vwap" (last (kan-kat.technical-indicators/vwap h-slice))}))))
        
        cleaned-data (vec (drop 35 ohlcv-history))
        all-60-features (vec (concat base-features indicator-features cdl-features))
        
        dataset (create-multivariate-windows cleaned-data 1 all-60-features "close")
        splits (dl/train-test-split dataset 0.2 false)
        train-data (:train splits)
        test-data (:test splits)
        train-size (count train-data)
        
        input-features (vec (for [t (range 1) f all-60-features] (str f "_" t)))
        
        ;; KAN Pipelines
        x-scaler (dl/fit-minmax train-data input-features)
        y-scaler (dl/fit-minmax train-data ["target_close"])
        
        train-x (dl/transform-minmax x-scaler train-data input-features)
        train-y (dl/transform-minmax y-scaler train-data ["target_close"])
        test-x (dl/transform-minmax x-scaler test-data input-features)
        test-y (dl/transform-minmax y-scaler test-data ["target_close"])
        
        input-dim 60
        X-train (dl/to-tensor train-x input-features)
        Y-train (dl/to-tensor train-y ["target_close"])
        X-test (dl/to-tensor test-x input-features)
        Y-test (dl/to-tensor test-y ["target_close"])
        
        prev-train (double-array (mapv #(get (nth cleaned-data %) "close") (range train-size)))
        prev-test (double-array (mapv #(get (nth cleaned-data (+ % train-size)) "close") (range (count test-data))))
        
        mhk-make (requiring-resolve 'kan-kat.multi-head-kan/make-multi-head-kan)
        mhk-forward (requiring-resolve 'kan-kat.multi-head-kan/mhk-forward)
        mhk-params (requiring-resolve 'kan-kat.multi-head-kan/mhk-params)
        mhk-update (requiring-resolve 'kan-kat.multi-head-kan/mhk-update)
        
        babylonian-forward (fn [mhk X]
                               (let [heads (:heads mhk)
                                     N (count heads)]
                                 (if (= N 1)
                                   (mhk-forward mhk X)
                                   (let [batch-size (first (:shape X))
                                         ^doubles data (:data X)
                                         x-cols (vec (for [i (range N)]
                                                       (let [col-data (double-array batch-size)]
                                                         (dotimes [b batch-size]
                                                           (aset col-data b (aget data (+ (* b N) i))))
                                                         (t2/tensor col-data [batch-size 1]))))
                                         head-outputs (mapv (fn [head col-x] (fw/model-forward head col-x)) heads x-cols)
                                         cat-output (reduce t2/t-cat head-outputs)]
                                     (fw/model-forward (:o-kan mhk) cat-output)))))]
                                     
    (println (format "Dataset: %d rows (%d train / %d test) on %d Features" 
                     (+ (count train-data) (count test-data))
                     (count train-data)
                     (count test-data)
                     input-dim))

    ;; 1. Run KAN (Babylonian 60-Head)
    (println "\n[Training KAN] 60 Narrow Heads...")
    (let [model-kan (mhk-make 60 1 3 1 3)
          lr 0.05
          epochs 20
          kan-start (System/nanoTime)
          final-kan (loop [ep 1 curr model-kan]
                      (if (> ep epochs)
                        curr
                        (let [preds (babylonian-forward curr X-train)
                              loss-t (t2/mse-loss preds Y-train)
                              loss (aget ^doubles (:data loss-t) 0)]
                          (t2/backward! loss-t)
                          (let [new-mhk (mhk-update curr (mapv #(t2/sgd-step! % lr 5.0) (mhk-params curr)))]
                            (recur (inc ep) new-mhk)))))
          kan-time (/ (- (System/nanoTime) kan-start) 1e9)
          
          preds-train (babylonian-forward final-kan X-train)
          preds-test (babylonian-forward final-kan X-test)
          kan-train-mse (aget ^doubles (:data (t2/mse-loss preds-train Y-train)) 0)
          kan-test-mse (aget ^doubles (:data (t2/mse-loss preds-test Y-test)) 0)
          kan-acc-test (* 100.0 (directional-accuracy preds-test Y-test prev-test))]
          
      (println (format "KAN Finished in %.2fs" kan-time))
      (println (format "KAN Train MSE: %.4f | Test MSE: %.4f" kan-train-mse kan-test-mse))
      (println (format "KAN Test Directional Acc: %.1f%%" kan-acc-test))

      ;; 2. Run scicloj.ml (Random Forest)
      (println "\n[Training Baseline] Scicloj.ml Random Forest (Smile)...")
      (let [prep-fn (requiring-resolve 'kan-kat.benchmark-ml/prep-techml-dataset)
            train-fn (requiring-resolve 'kan-kat.benchmark-ml/train-random-forest)
            predict-fn (requiring-resolve 'kan-kat.benchmark-ml/predict-ml)
            
            ;; Подготавливаем X в виде векторов (списков)
            x-train-vecs (mapv (fn [row] (mapv #(get row % 0.0) input-features)) train-x)
            y-train-vecs (mapv #(get % "target_close" 0.0) train-y)
            x-test-vecs (mapv (fn [row] (mapv #(get row % 0.0) input-features)) test-x)
            y-test-vecs (mapv #(get % "target_close" 0.0) test-y)
            
            train-ds (prep-fn x-train-vecs y-train-vecs input-features)
            test-ds (prep-fn x-test-vecs y-test-vecs input-features)
            
            rf-start (System/nanoTime)
            rf-model (train-fn train-ds)
            rf-time (/ (- (System/nanoTime) rf-start) 1e9)
            
            rf-preds-train (predict-fn rf-model train-ds)
            rf-preds-test (predict-fn rf-model test-ds)
            
            ;; Считаем метрики (Scicloj выдает вектор doubles)
            t-rf-train-preds (t2/tensor (double-array rf-preds-train) [(count rf-preds-train) 1])
            t-rf-test-preds (t2/tensor (double-array rf-preds-test) [(count rf-preds-test) 1])
            
            _ (println "t-rf-train-preds shape:" (:shape t-rf-train-preds) " Y-train shape:" (:shape Y-train))
            _ (println "t-rf-test-preds shape:" (:shape t-rf-test-preds) " Y-test shape:" (:shape Y-test))
            
            rf-train-mse (aget ^doubles (:data (t2/mse-loss t-rf-train-preds Y-train)) 0)
            rf-test-mse (aget ^doubles (:data (t2/mse-loss t-rf-test-preds Y-test)) 0)
            rf-acc-test (* 100.0 (directional-accuracy t-rf-test-preds Y-test prev-test))]
            
        (println (format "RF Finished in %.2fs" rf-time))
        (println (format "RF Train MSE: %.4f | Test MSE: %.4f" rf-train-mse rf-test-mse))
        (println (format "RF Test Directional Acc: %.1f%%" rf-acc-test))
        
        (println "\n==========================================")
        (println " BENCHMARK SUMMARY")
        (println "==========================================")
        (println (format "| %-15s | %-12s | %-12s | %-12s |" "Model" "Train MSE" "Test MSE" "Test DirAcc"))
        (println "-------------------------------------------------------------")
        (println (format "| %-15s | %-12.4f | %-12.4f | %-11.1f%% |" "60-Head KAN" kan-train-mse kan-test-mse kan-acc-test))
        (println (format "| %-15s | %-12.4f | %-12.4f | %-11.1f%% |" "Random Forest" rf-train-mse rf-test-mse rf-acc-test))
        (println "==========================================\n")))))
