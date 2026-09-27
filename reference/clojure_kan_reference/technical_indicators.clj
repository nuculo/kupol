(ns kan-kat.technical-indicators
  "Библиотека классических технических индикаторов (Technical Analysis) 
   для извлечения признаков (Feature Engineering) из OHLCV графиков.")

;; ==========================================
;; Moving Averages (Тренд)
;; ==========================================

(defn sma
  "Simple Moving Average (SMA) для серии значений за период (период).
   Возвращает вектор такой же длины. Первые (period-1) элементы равны nil."
  [series period]
  (let [N (count series)
        res (object-array N)]
    (loop [i (dec period)
           sum (reduce + (take period series))]
      (when (< i N)
        (aset res i (/ (double sum) period))
        (when (< (inc i) N)
          (recur (inc i) (+ sum (nth series (inc i)) (- (nth series (- (inc i) period))))))))
    (vec res)))

(defn ema
  "Exponential Moving Average (EMA). Отдает больший вес последним данным.
   multiplier = [2 / (period + 1)]. Возвращает вектор."
  [series period]
  (let [N (count series)
        res (object-array N)
        k (/ 2.0 (inc period))]
    (when (>= N period)
      ;; SMA для первой точки EMA
      (let [initial-sma (/ (double (reduce + (take period series))) period)]
        (aset res (dec period) initial-sma)
        (loop [i period
               prev-ema initial-sma]
          (when (< i N)
            (let [curr-ema (+ (* (- (nth series i) prev-ema) k) prev-ema)]
              (aset res i curr-ema)
              (recur (inc i) curr-ema))))))
    (vec res)))

;; ==========================================
;; Momentum Oscillators (Осцилляторы Импульса)
;; ==========================================

(defn rsi
  "Relative Strength Index (RSI). Классическая формула Дж. Уэллса Уайлдера (Wilder's Smoothing).
   Измеряет скорость и изменение ценовых движений [0, 100]."
  [series period]
  (let [N (count series)
        res (object-array N)]
    (when (> N period)
      ;; Вычисляем начальные Average Gain и Average Loss (SMA первой свечки)
      (let [changes (map - (rest series) series)
            initial-changes (take period changes)
            initial-gain (/ (reduce + (map #(max % 0.0) initial-changes)) period)
            initial-loss (/ (reduce + (map #(Math/abs (min % 0.0)) initial-changes)) period)]
            
        (let [initial-rs (if (zero? initial-loss) 100000.0 (/ initial-gain initial-loss))]
          (aset res period (- 100.0 (/ 100.0 (inc initial-rs)))))
          
        (loop [i (inc period)
               prev-gain initial-gain
               prev-loss initial-loss]
          (when (< i N)
            (let [change (- (nth series i) (nth series (dec i)))
                  gain-val (max change 0.0)
                  loss-val (Math/abs (min change 0.0))
                  
                  ;; Wilder's Smoothing Method
                  avg-gain (/ (+ (* prev-gain (dec period)) gain-val) period)
                  avg-loss (/ (+ (* prev-loss (dec period)) loss-val) period)
                  
                  rs (if (zero? avg-loss) 100000.0 (/ avg-gain avg-loss))
                  rsi-val (- 100.0 (/ 100.0 (inc rs)))]
                  
              (aset res i rsi-val)
              (recur (inc i) avg-gain avg-loss))))))
    (vec res)))

(defn macd
  "Moving Average Convergence Divergence (MACD). 
   Возвращает вектор мап: {:macd :signal :histogram}"
  [series fast-period slow-period signal-period]
  (let [fast-ema (ema series fast-period)
        slow-ema (ema series slow-period)
        macd-line (vec (for [i (range (count series))]
                         (if (and (get fast-ema i) (get slow-ema i))
                           (- (get fast-ema i) (get slow-ema i))
                           nil)))
        ;; Мы считаем EMA только по non-nil значениям MACD
        valid-macd (remove nil? macd-line)
        offset (- (count series) (count valid-macd))
        signal-line-valid (ema valid-macd signal-period)
        
        signal-padded (into (vec (repeat offset nil)) signal-line-valid)]
        
    (vec (for [i (range (count series))]
           (let [m (nth macd-line i)
                 s (nth signal-padded i)]
             (if (and m s)
               {:macd m :signal s :hist (- m s)}
               {:macd nil :signal nil :hist nil}))))))

;; ==========================================
;; Volatility Bands (Волатильность)
;; ==========================================

(defn bollinger-bands
  "Bollinger Bands (Ленты Боллинджера). Возвращает вектор {:high :mid :low}
   Использует SMA и Стандартное Отклонение (SD) на базе period, умноженное на mult."
  [series period mult]
  (let [N (count series)
        mid-line (sma series period)
        res (object-array N)]
    (dotimes [i N]
      (if-let [m (nth mid-line i)]
        (let [window (subvec series (- (inc i) period) (inc i))
              variance (/ (reduce + (map #(Math/pow (- % m) 2) window)) period)
              sd (Math/sqrt variance)]
          (aset res i {:high (+ m (* sd mult))
                       :mid m
                       :low (- m (* sd mult))}))
        (aset res i nil)))
    (vec res)))

(defn true-range
  "Вычисляет True Range (TR) для списка мап с ключами \"high\", \"low\", \"close\".
   ATR использует это как базу."
  [ohlcv-data]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (loop [i 0]
      (when (< i N)
        (let [curr (nth ohlcv-data i)
              h (get curr "high")
              l (get curr "low")]
          (if (zero? i)
            (aset res i (- h l))
            (let [prev-c (get (nth ohlcv-data (dec i)) "close")
                  tr (max (- h l)
                          (Math/abs (- h prev-c))
                          (Math/abs (- l prev-c)))]
              (aset res i tr))))
        (recur (inc i))))
    (vec res)))

(defn atr
  "Average True Range (ATR). SMA или эквивалент от TR."
  [ohlcv-data period]
  (let [tr-series (true-range ohlcv-data)]
    (sma tr-series period)))

(defn keltner-channels
  "Каналы Кельтнера (Keltner Channels). 
   Используют EMA для средней линии и ATR для ширины (mult * ATR)."
  [ohlcv-data period atr-mult]
  (let [closes (vec (map #(get % "close") ohlcv-data))
        mid-line (ema closes period)
        atr-line (atr ohlcv-data period)
        N (count ohlcv-data)
        res (object-array N)]
    (dotimes [i N]
      (let [m (nth mid-line i)
            a (nth atr-line i)]
        (if (and m a)
          (aset res i {:high (+ m (* atr-mult a))
                       :mid m
                       :low (- m (* atr-mult a))})
          (aset res i nil))))
    (vec res)))

;; ==========================================
;; Extended Oscillators & Momentum
;; ==========================================

(defn stochastic-oscillator
  "Stochastic Oscillator. Возвращает вектор мап: {:k %K :d %D}.
   Formula %K = (Current Close - Lowest Low) / (Highest High - Lowest Low) * 100.
   Formula %D = 3-day SMA of %K."
  [ohlcv-data k-period d-period]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (dotimes [i N]
      (if (>= i (dec k-period))
        (let [window (subvec ohlcv-data (- (inc i) k-period) (inc i))
              curr-c (get (nth ohlcv-data i) "close")
              lows (map #(get % "low") window)
              highs (map #(get % "high") window)
              lowest-low (apply min lows)
              highest-high (apply max highs)
              denom (- highest-high lowest-low)]
          (if (zero? denom)
            (aset res i {:k 50.0 :d nil}) ;; Защита от деления на ноль во флэте
            (aset res i {:k (* (/ (- curr-c lowest-low) denom) 100.0) :d nil})))
        (aset res i {:k nil :d nil})))
    
    ;; Вычисляем %D (SMA от %K)
    (let [k-series (map :k res)
          valid-k (remove nil? k-series)
          offset (- N (count valid-k))
          d-valid (sma valid-k d-period)
          d-line (into (vec (repeat offset nil)) d-valid)]
      (vec (map-indexed (fn [idx m] (assoc m :d (nth d-line idx))) res)))))

(defn williams-r
  "Williams %R. Очень похож на Стохастик, но инвертирован (от 0 до -100).
   Formula %R = (Highest High - Current Close) / (Highest High - Lowest Low) * -100."
  [ohlcv-data period]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (dotimes [i N]
      (if (>= i (dec period))
        (let [window (subvec ohlcv-data (- (inc i) period) (inc i))
              curr-c (get (nth ohlcv-data i) "close")
              lows (map #(get % "low") window)
              highs (map #(get % "high") window)
              lowest-low (apply min lows)
              highest-high (apply max highs)
              denom (- highest-high lowest-low)]
          (if (zero? denom)
            (aset res i -50.0)
            (aset res i (* (/ (- highest-high curr-c) denom) -100.0))))
        (aset res i nil)))
    (vec res)))

(defn roc
  "Rate of Change (ROC). Измеряет процентное изменение цены за N периодов.
   Formula: ((Current - Previous_N) / Previous_N) * 100"
  [series period]
  (let [N (count series)
        res (object-array N)]
    (dotimes [i N]
      (if (>= i period)
        (let [curr (nth series i)
              prev (nth series (- i period))]
          (if (zero? prev)
            (aset res i 0.0)
            (aset res i (* (/ (- curr prev) prev) 100.0))))
        (aset res i nil)))
    (vec res)))

;; ==========================================
;; Volume-Based Indicators
;; ==========================================

(defn obv
  "On-Balance Volume (OBV). Кумулятивный индикатор давления покупателей/продавцов."
  [ohlcv-data]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (when (> N 0)
      (aset res 0 (get (nth ohlcv-data 0) "volume"))
      (loop [i 1]
        (when (< i N)
          (let [curr-c (get (nth ohlcv-data i) "close")
                prev-c (get (nth ohlcv-data (dec i)) "close")
                curr-v (get (nth ohlcv-data i) "volume")
                prev-obv (aget res (dec i))]
            (cond
              (> curr-c prev-c) (aset res i (+ prev-obv curr-v))
              (< curr-c prev-c) (aset res i (- prev-obv curr-v))
              :else (aset res i prev-obv))
            (recur (inc i))))))
    (vec res)))

(defn vwap
  "Volume Weighted Average Price (VWAP).
   Рассчитывается обычно внутри дня, но здесь мы считаем кумулятивный VWAP для симуляции."
  [ohlcv-data]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (loop [i 0
           cum-vol 0.0
           cum-pv 0.0]
      (when (< i N)
        (let [curr (nth ohlcv-data i)
              typical-price (/ (+ (get curr "high") (get curr "low") (get curr "close")) 3.0)
              vol (get curr "volume")
              new-cum-vol (+ cum-vol vol)
              new-cum-pv (+ cum-pv (* typical-price vol))]
          (if (zero? new-cum-vol)
            (aset res i (double typical-price))
            (aset res i (/ (double new-cum-pv) new-cum-vol)))
          (recur (inc i) new-cum-vol new-cum-pv))))
    (vec res)))

;; ==========================================
;; Phase 53.4: Advanced Trading Analytics (Smart Money)
;; ==========================================

;; --- 1. ADX (Average Directional Index) ---
;; Измеряет силу тренда, независимо от направления [0, 100].
;; Требует вычисления +DM, -DM, TR, +DI, -DI перед финальным ADX.
(defn adx
  "Average Directional Index (ADX) для оценки силы тренда."
  [ohlcv-data period]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (if (<= N period)
      (vec (repeat N nil))
      (let [tr-series (true-range ohlcv-data)
            plus-dm (object-array N)
            minus-dm (object-array N)]
        ;; Вычисляем сырые +DM и -DM
        (dotimes [i N]
          (if (zero? i)
            (do (aset plus-dm i 0.0)
                (aset minus-dm i 0.0))
            (let [curr (nth ohlcv-data i)
                  prev (nth ohlcv-data (dec i))
                  up-move (- (get curr "high") (get prev "high"))
                  down-move (- (get prev "low") (get curr "low"))]
              (if (and (> up-move down-move) (> up-move 0))
                (aset plus-dm i up-move)
                (aset plus-dm i 0.0))
              (if (and (> down-move up-move) (> down-move 0))
                (aset minus-dm i down-move)
                (aset minus-dm i 0.0)))))
                
        ;; Wilder's Smoothing (эквивалентно экспоненциальному сглаживанию)
        (let [smooth (fn [series]
                       (let [s-res (object-array N)
                             initial-sum (reduce + (take period (drop 1 series)))]
                         (aset s-res period initial-sum)
                         (loop [i (inc period)
                                prev-val initial-sum]
                           (when (< i N)
                             (let [curr-val (+ (- prev-val (/ prev-val period)) (nth series i))]
                               (aset s-res i curr-val)
                               (recur (inc i) curr-val))))
                         s-res))
              smooth-tr (smooth tr-series)
              smooth-plus (smooth plus-dm)
              smooth-minus (smooth minus-dm)
              dx (object-array N)]
          
          ;; Вычисляем DX
          (dotimes [i N]
            (if (<= i period)
              (aset dx i 0.0)
              (let [tr-val (aget smooth-tr i)]
                (if (zero? tr-val)
                  (aset dx i 0.0)
                  (let [plus-di (* (/ (aget smooth-plus i) tr-val) 100.0)
                        minus-di (* (/ (aget smooth-minus i) tr-val) 100.0)
                        di-diff (Math/abs (- plus-di minus-di))
                        di-sum (+ plus-di minus-di)]
                    (aset dx i (if (zero? di-sum) 0.0 (* (/ di-diff di-sum) 100.0))))))))
                    
          ;; ADX = smoothed DX
          (let [adx-res (object-array N)
                adx-start (* period 2)
                initial-adx (/ (reduce + (take period (drop (inc period) dx))) period)]
            (if (< N adx-start)
              (vec (repeat N nil))
              (do
                (aset adx-res adx-start initial-adx)
                (loop [i (inc adx-start)
                       prev-adx initial-adx]
                  (when (< i N)
                    (let [curr-adx (/ (+ (* prev-adx (dec period)) (aget dx i)) period)]
                      (aset adx-res i curr-adx)
                      (recur (inc i) curr-adx))))
                (vec (map-indexed (fn [idx v] (if (< idx adx-start) nil v)) (vec adx-res)))))))))))

;; --- 2. HMA (Hull Moving Average) ---
;; HMA(n) = WMA(2 * WMA(n/2) - WMA(n), sqrt(n))
(defn wma
  "Weighted Moving Average (WMA)."
  [series period]
  (let [N (count series)
        res (object-array N)
        weight-sum (/ (* period (inc period)) 2.0)]
    (dotimes [i N]
      (if (>= i (dec period))
        (let [window (subvec series (- (inc i) period) (inc i))
              weighted-sum (reduce + (map-indexed (fn [idx val] (* val (inc idx))) window))]
          (aset res i (/ weighted-sum weight-sum)))
        (aset res i nil)))
    (vec res)))

(defn hma
  "Hull Moving Average (HMA). Экстремально гладкая и быстрая средняя с нулевым запаздыванием."
  [series period]
  (let [half-period (int (/ period 2.0))
        sqrt-period (int (Math/sqrt period))
        wma-half (wma series half-period)
        wma-full (wma series period)
        N (count series)
        raw-hma (vec (for [i (range N)]
                       (let [h (nth wma-half i)
                             f (nth wma-full i)]
                         (if (and h f)
                           (- (* 2.0 h) f)
                           nil))))
        ;; Нам нужно посчитать WMA по raw-hma. Убираем nil, считаем, возвращаем nil-смещение
        valid-raw (remove nil? raw-hma)
        valid-hma (wma (vec valid-raw) sqrt-period)
        offset (- N (count valid-raw))]
    (into (vec (repeat offset nil)) valid-hma)))

;; --- 3. Shannon Entropy ---
(defn shannon-entropy
  "Оценивает рыночный хаос/структурированность за окно.
   Рассчитывает энтропию дискретизированных логарифмических доходностей цены."
  [series period bins]
  (let [N (count series)
        res (object-array N)
        ;; Лог-доходности
        returns (vec (for [i (range 1 N)]
                       (let [curr (nth series i)
                             prev (nth series (dec i))]
                         (if (zero? prev) 0.0 (Math/log (/ curr prev))))))
        ret-offset (into [0.0] returns)]
    (dotimes [i N]
      (if (>= i period)
        (let [window (subvec ret-offset (- (inc i) period) (inc i))
              min-val (apply min window)
              max-val (apply max window)
              range-val (- max-val min-val)]
          (if (zero? range-val)
            (aset res i 0.0)
            (let [hist (frequencies 
                        (map (fn [v] 
                               (int (* (- bins 1) (/ (- v min-val) range-val)))) 
                             window))
                  total (double period)
                  entropy (reduce (fn [acc [_ count]]
                                    (let [p (/ count total)]
                                      (- acc (* p (/ (Math/log p) (Math/log 2.0))))))
                                  0.0 hist)]
              (aset res i entropy))))
        (aset res i nil)))
    (vec res)))

;; --- 4. Signal Feature Engineering ---
(defn extract-crossover-signals
  "Преобразует непрерывные индикаторы в разреженные категоричные бинарные паттерны:
   +1 (Бычий Сигнал), -1 (Медвежий Сигнал), 0 (Нет Сигнала / Нейтрально).
   Входные словари OHLCV ожидают ключи: rsi, ema, close."
  [enriched-ohlcv short-ema-period long-ema-period]
  (let [N (count enriched-ohlcv)
        closes (vec (map #(get % "close") enriched-ohlcv))
        short-ema (ema closes short-ema-period)
        long-ema (ema closes long-ema-period)]
    (vec (for [i (range N)]
           (let [day (nth enriched-ohlcv i)
                 rsi-val (get day "rsi")
                 
                 ;; Индикатор режима: если ADX существует, используем его, иначе флэт=0
                 adx-val (get day "adx" 20.0)
                 is-trend? (> adx-val 25.0)
                 
                 s-ema (nth short-ema i)
                 l-ema (nth long-ema i)
                 prev-s-ema (if (> i 0) (nth short-ema (dec i)) nil)
                 prev-l-ema (if (> i 0) (nth long-ema (dec i)) nil)
                 
                 ;; 1. Поиск Золотых / Мертвых крестов
                 cross-signal (if (and s-ema l-ema prev-s-ema prev-l-ema)
                                (cond
                                  (and (> s-ema l-ema) (<= prev-s-ema prev-l-ema)) 1.0  ;; Golden Cross
                                  (and (< s-ema l-ema) (>= prev-s-ema prev-l-ema)) -1.0 ;; Death Cross
                                  :else 0.0)
                                0.0)
                 
                 ;; 2. Поиск экстремумов RSI
                 rsi-signal (cond
                              (nil? rsi-val) 0.0
                              (< rsi-val 30.0) 1.0  ;; Buy signal
                              (> rsi-val 70.0) -1.0 ;; Sell signal
                              :else 0.0)]
             ;; Комбинируем или возвращаем оба
             (assoc day 
                    "signal_cross" cross-signal
                    "signal_rsi" rsi-signal
                    "regime_trend" (if is-trend? 1.0 0.0)))))))

;; ==========================================
;; Phase 53.5: Comprehensive Indicator Library
;; ==========================================

;; --- Additional Moving Averages ---

(defn dema
  "Double Exponential Moving Average. DEMA = 2×EMA(n) − EMA(EMA(n))."
  [series period]
  (let [e1 (ema series period)
        valid-e1 (vec (remove nil? e1))
        e2 (ema valid-e1 period)
        offset (- (count e1) (count valid-e1))
        offset2 (- (count valid-e1) (count (remove nil? e2)))
        total-offset (+ offset offset2)]
    (vec (concat (repeat total-offset nil)
                 (map (fn [i]
                        (let [e1-idx (+ i offset offset2)
                              e1-val (nth e1 e1-idx)
                              e2-val (nth e2 (+ i offset2))]
                          (if (and e1-val e2-val)
                            (- (* 2.0 e1-val) e2-val)
                            nil)))
                      (range (- (count series) total-offset)))))))

(defn tema
  "Triple Exponential Moving Average. TEMA = 3×EMA − 3×EMA(EMA) + EMA(EMA(EMA))."
  [series period]
  (let [e1 (ema series period)
        valid-e1 (vec (remove nil? e1))
        e2 (ema valid-e1 period)
        valid-e2 (vec (remove nil? e2))
        e3 (ema valid-e2 period)
        N (count series)
        o1 (- N (count valid-e1))
        o2 (- (count valid-e1) (count valid-e2))
        o3 (- (count valid-e2) (count (remove nil? e3)))
        total-offset (+ o1 o2 o3)]
    (vec (concat (repeat total-offset nil)
                 (map (fn [i]
                        (let [i1 (+ i o1 o2 o3)
                              i2 (+ i o2 o3)
                              i3 (+ i o3)]
                          (let [v1 (nth e1 i1 nil)
                                v2 (nth e2 i2 nil)
                                v3 (nth e3 i3 nil)]
                            (if (and v1 v2 v3)
                              (+ (- (* 3.0 v1) (* 3.0 v2)) v3)
                              nil))))
                      (range (- N total-offset)))))))

(defn smma
  "Smoothed Moving Average (SMMA / RMA / Wilder's MA). 
   SMMA(i) = (SMMA(i-1)×(N-1) + Close(i)) / N."
  [series period]
  (let [N (count series)
        res (object-array N)]
    (when (>= N period)
      (let [init (/ (double (reduce + (take period series))) period)]
        (aset res (dec period) init)
        (loop [i period
               prev init]
          (when (< i N)
            (let [curr (/ (+ (* prev (dec period)) (nth series i)) (double period))]
              (aset res i curr)
              (recur (inc i) curr))))))
    (vec res)))

(defn vwma
  "Volume-Weighted Moving Average. SMA, но вес каждого Close = Volume."
  [ohlcv-data period]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (dotimes [i N]
      (if (>= i (dec period))
        (let [window (subvec ohlcv-data (- (inc i) period) (inc i))
              sum-cv (reduce + (map #(* (get % "close") (get % "volume")) window))
              sum-v (reduce + (map #(get % "volume") window))]
          (aset res i (if (zero? sum-v) (get (nth ohlcv-data i) "close") (/ sum-cv sum-v))))
        (aset res i nil)))
    (vec res)))

(defn tma
  "Triangular Moving Average (TMA). SMA от SMA — двойное сглаживание."
  [series period]
  (let [half (int (Math/ceil (/ (inc period) 2.0)))
        first-sma (sma series half)
        valid (vec (remove nil? first-sma))
        second-sma (sma valid half)
        offset (- (count series) (count valid))
        offset2 (- (count valid) (count (remove nil? second-sma)))
        total (+ offset offset2)]
    (into (vec (repeat total nil)) (remove nil? second-sma))))

(defn kama
  "Kaufman Adaptive Moving Average. Адаптивная по Direction/Volatility."
  [series period]
  (let [N (count series)
        fast-sc (/ 2.0 3.0)  ;; 2/(2+1)
        slow-sc (/ 2.0 31.0) ;; 2/(30+1)
        res (object-array N)]
    (when (>= N period)
      (aset res (dec period) (nth series (dec period)))
      (loop [i period
             prev-kama (double (nth series (dec period)))]
        (when (< i N)
          (let [direction (Math/abs (- (double (nth series i)) (double (nth series (- i period)))))
                volatility (reduce + (map (fn [j] (Math/abs (- (double (nth series (inc j))) (double (nth series j)))))
                                         (range (- i period) i)))
                er (if (zero? volatility) 1.0 (/ direction volatility))
                sc (let [raw (+ (* er (- fast-sc slow-sc)) slow-sc)] (* raw raw))
                curr (+ prev-kama (* sc (- (double (nth series i)) prev-kama)))]
            (aset res i curr)
            (recur (inc i) curr)))))
    (vec res)))

;; --- Additional Oscillators ---

(defn momentum
  "Чистый Momentum. M = Close(t) − Close(t−n)."
  [series period]
  (let [N (count series)
        res (object-array N)]
    (dotimes [i N]
      (if (>= i period)
        (aset res i (- (double (nth series i)) (double (nth series (- i period)))))
        (aset res i nil)))
    (vec res)))

(defn cci
  "Commodity Channel Index. CCI = (TP − SMA(TP)) / (0.015 × MAD)."
  [ohlcv-data period]
  (let [N (count ohlcv-data)
        tp (vec (map #(/ (+ (get % "high") (get % "low") (get % "close")) 3.0) ohlcv-data))
        tp-sma (sma tp period)
        res (object-array N)]
    (dotimes [i N]
      (if (and (>= i (dec period)) (nth tp-sma i))
        (let [window (subvec tp (- (inc i) period) (inc i))
              mean (nth tp-sma i)
              mad (/ (reduce + (map #(Math/abs (- % mean)) window)) (double period))]
          (aset res i (if (zero? mad) 0.0 (/ (- (nth tp i) mean) (* 0.015 mad)))))
        (aset res i nil)))
    (vec res)))

(defn mfi
  "Money Flow Index. RSI с учётом объёма ('Volume-weighted RSI')."
  [ohlcv-data period]
  (let [N (count ohlcv-data)
        tp (vec (map #(/ (+ (get % "high") (get % "low") (get % "close")) 3.0) ohlcv-data))
        raw-mf (vec (map-indexed (fn [i _] (* (nth tp i) (get (nth ohlcv-data i) "volume"))) (range N)))
        res (object-array N)]
    (dotimes [i N]
      (if (>= i period)
        (let [window-range (range (- (inc i) period) (inc i))
              pos-flow (reduce + (map (fn [j] (if (and (> j 0) (> (nth tp j) (nth tp (dec j)))) (nth raw-mf j) 0.0)) window-range))
              neg-flow (reduce + (map (fn [j] (if (and (> j 0) (<= (nth tp j) (nth tp (dec j)))) (nth raw-mf j) 0.0)) window-range))]
          (aset res i (if (zero? neg-flow) 100.0 (- 100.0 (/ 100.0 (inc (/ pos-flow neg-flow)))))))
        (aset res i nil)))
    (vec res)))

(defn cmo
  "Chande Momentum Oscillator. Более резкий RSI: (sumUp − sumDown) / (sumUp + sumDown) × 100."
  [series period]
  (let [N (count series)
        res (object-array N)]
    (dotimes [i N]
      (if (>= i period)
        (let [window (map (fn [j] (- (double (nth series (inc j))) (double (nth series j))))
                          (range (- i period) i))
              sum-up (reduce + (map #(if (> % 0) % 0.0) window))
              sum-down (reduce + (map #(if (< % 0) (Math/abs %) 0.0) window))
              total (+ sum-up sum-down)]
          (aset res i (if (zero? total) 0.0 (* (/ (- sum-up sum-down) total) 100.0))))
        (aset res i nil)))
    (vec res)))

(defn tsi
  "True Strength Index. Двойное EMA-сглаживание импульса: EMA(EMA(Δ)) / EMA(EMA(|Δ|)) × 100."
  [series long-period short-period]
  (let [N (count series)
        deltas (vec (cons 0.0 (map (fn [i] (- (double (nth series i)) (double (nth series (dec i))))) (range 1 N))))
        abs-deltas (vec (map #(Math/abs %) deltas))
        e1 (ema deltas long-period)
        e1a (ema abs-deltas long-period)
        valid-e1 (vec (remove nil? e1))
        valid-e1a (vec (remove nil? e1a))
        e2 (ema valid-e1 short-period)
        e2a (ema valid-e1a short-period)
        offset (- N (count valid-e1))
        offset2 (- (count valid-e1) (count (remove nil? e2)))
        total (+ offset offset2)
        res (object-array N)]
    (dotimes [i N]
      (if (>= i total)
        (let [num (nth e2 (- i total) nil)
              den (nth e2a (- i total) nil)]
          (aset res i (if (or (nil? num) (nil? den) (zero? den)) 0.0 (* (/ num den) 100.0))))
        (aset res i nil)))
    (vec res)))

;; --- Trend Indicators ---

(defn aroon
  "Aroon Up/Down. Возвращает {:up :down :oscillator}."
  [ohlcv-data period]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (dotimes [i N]
      (if (>= i period)
        (let [window (subvec ohlcv-data (- (inc i) period) (inc i))
              highs (map-indexed (fn [idx d] [idx (get d "high")]) window)
              lows (map-indexed (fn [idx d] [idx (get d "low")]) window)
              max-idx (first (apply max-key second highs))
              min-idx (first (apply min-key second lows))
              up (* (/ (double max-idx) period) 100.0)
              down (* (/ (double min-idx) period) 100.0)]
          (aset res i {:up up :down down :oscillator (- up down)}))
        (aset res i nil)))
    (vec res)))

(defn parabolic-sar
  "Parabolic SAR (Stop And Reverse). Упрощённая реализация с AF."
  [ohlcv-data af-step af-max]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (when (> N 1)
      (let [init-high (get (nth ohlcv-data 0) "high")
            init-low (get (nth ohlcv-data 0) "low")]
        (aset res 0 init-low)
        (loop [i 1
               is-long true
               sar (double init-low)
               ep (double init-high)
               af (double af-step)]
          (when (< i N)
            (let [curr (nth ohlcv-data i)
                  high (get curr "high")
                  low (get curr "low")
                  new-sar (+ sar (* af (- ep sar)))]
              (if is-long
                (if (< low new-sar)
                  ;; Разворот в Short
                  (do (aset res i ep)
                      (recur (inc i) false (double ep) (double low) (double af-step)))
                  (let [new-ep (max ep high)
                        new-af (if (> high ep) (min (+ af af-step) af-max) af)]
                    (aset res i new-sar)
                    (recur (inc i) true (double new-sar) (double new-ep) (double new-af))))
                (if (> high new-sar)
                  ;; Разворот в Long
                  (do (aset res i ep)
                      (recur (inc i) true (double ep) (double high) (double af-step)))
                  (let [new-ep (min ep low)
                        new-af (if (< low ep) (min (+ af af-step) af-max) af)]
                    (aset res i new-sar)
                    (recur (inc i) false (double new-sar) (double new-ep) (double new-af))))))))))
    (vec res)))

(defn supertrend
  "Supertrend indicator. ATR-based тренд (simplified)."
  [ohlcv-data period multiplier]
  (let [N (count ohlcv-data)
        atr-vals (atr ohlcv-data period)
        res (object-array N)]
    (dotimes [i N]
      (let [curr (nth ohlcv-data i)
            hl2 (/ (+ (get curr "high") (get curr "low")) 2.0)
            atr-v (nth atr-vals i)]
        (if (and atr-v (not (nil? atr-v)))
          (aset res i {:upper (+ hl2 (* multiplier atr-v))
                       :lower (- hl2 (* multiplier atr-v))})
          (aset res i nil))))
    (vec res)))

(defn dpo
  "Detrended Price Oscillator. DPO = Close − SMA(N/2 + 1 bars ago)."
  [series period]
  (let [shift (inc (int (/ period 2)))
        sma-vals (sma series period)
        N (count series)
        res (object-array N)]
    (dotimes [i N]
      (let [sma-idx (- i shift)]
        (if (and (>= sma-idx 0) (nth sma-vals sma-idx))
          (aset res i (- (double (nth series i)) (double (nth sma-vals sma-idx))))
          (aset res i nil))))
    (vec res)))

;; --- Volatility Extensions ---

(defn bollinger-pct-b
  "Bollinger %B. Позиция цены в лентах: (Close − Lower) / (Upper − Lower)."
  [series period mult]
  (let [bb (bollinger-bands series period mult)
        N (count series)
        res (object-array N)]
    (dotimes [i N]
      (let [b (nth bb i)]
        (if (and b (:high b) (:low b))
          (let [range-val (- (:high b) (:low b))]
            (aset res i (if (zero? range-val) 0.5 (/ (- (double (nth series i)) (:low b)) range-val))))
          (aset res i nil))))
    (vec res)))

(defn bollinger-bandwidth
  "Bollinger BandWidth. (Upper − Lower) / Middle. Сжатие = предвестник прорыва."
  [series period mult]
  (let [bb (bollinger-bands series period mult)
        N (count series)
        res (object-array N)]
    (dotimes [i N]
      (let [b (nth bb i)]
        (if (and b (:high b) (:mid b) (not (zero? (:mid b))))
          (aset res i (/ (- (:high b) (:low b)) (:mid b)))
          (aset res i nil))))
    (vec res)))

(defn donchian-channels
  "Donchian Channels ('Turtle Trading'). High(N), Low(N) за период."
  [ohlcv-data period]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (dotimes [i N]
      (if (>= i (dec period))
        (let [window (subvec ohlcv-data (- (inc i) period) (inc i))
              high-val (apply max (map #(get % "high") window))
              low-val (apply min (map #(get % "low") window))]
          (aset res i {:high high-val :low low-val :mid (/ (+ high-val low-val) 2.0)}))
        (aset res i nil)))
    (vec res)))

(defn standard-deviation
  "Standard Deviation σ(Close, N) как отдельная волатильность-фича."
  [series period]
  (let [N (count series)
        sma-vals (sma series period)
        res (object-array N)]
    (dotimes [i N]
      (if (and (>= i (dec period)) (nth sma-vals i))
        (let [window (subvec series (- (inc i) period) (inc i))
              mean (nth sma-vals i)
              variance (/ (reduce + (map #(let [d (- % mean)] (* d d)) window)) (double period))]
          (aset res i (Math/sqrt variance)))
        (aset res i nil)))
    (vec res)))

;; --- Volume Extensions ---

(defn accumulation-distribution
  "Accumulation/Distribution Line. A/D = CLV × Volume (cumulative)."
  [ohlcv-data]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (loop [i 0
           cum 0.0]
      (when (< i N)
        (let [d (nth ohlcv-data i)
              h (get d "high") l (get d "low") c (get d "close") v (get d "volume")
              hl (- h l)
              clv (if (zero? hl) 0.0 (/ (- (- c l) (- h c)) hl))
              new-cum (+ cum (* clv v))]
          (aset res i new-cum)
          (recur (inc i) new-cum))))
    (vec res)))

(defn cmf
  "Chaikin Money Flow. Среднее A/D за период."
  [ohlcv-data period]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (dotimes [i N]
      (if (>= i (dec period))
        (let [window (subvec ohlcv-data (- (inc i) period) (inc i))
              sum-mfv (reduce + (map (fn [d]
                                       (let [h (get d "high") l (get d "low") c (get d "close") v (get d "volume")
                                             hl (- h l)
                                             clv (if (zero? hl) 0.0 (/ (- (- c l) (- h c)) hl))]
                                         (* clv v)))
                                     window))
              sum-vol (reduce + (map #(get % "volume") window))]
          (aset res i (if (zero? sum-vol) 0.0 (/ sum-mfv sum-vol))))
        (aset res i nil)))
    (vec res)))

(defn force-index
  "Force Index. FI = (Close − PrevClose) × Volume."
  [ohlcv-data]
  (let [N (count ohlcv-data)
        res (object-array N)]
    (aset res 0 0.0)
    (loop [i 1]
      (when (< i N)
        (let [curr (nth ohlcv-data i)
              prev (nth ohlcv-data (dec i))]
          (aset res i (* (- (get curr "close") (get prev "close")) (get curr "volume")))
          (recur (inc i)))))
    (vec res)))
