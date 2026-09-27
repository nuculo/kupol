(ns kan-kat.candlestick-patterns
  "Извлечение японских свечных паттернов для KAN через clj-ta-lib (C-binding)."
  (:require [clj-ta-lib.core :as ta])
  (:import [com.tictactec.ta.lib.meta PriceHolder]))

(def all-cdl-patterns
  ["CDL2CROWS" "CDL3BLACKCROWS" "CDL3INSIDE" "CDL3LINESTRIKE"
   "CDL3OUTSIDE" "CDL3STARSINSOUTH" "CDL3WHITESOLDIERS"
   "CDLABANDONEDBABY" "CDLADVANCEBLOCK" "CDLBELTHOLD"
   "CDLBREAKAWAY" "CDLCLOSINGMARUBOZU" "CDLCONCEALBABYSWALL"
   "CDLCOUNTERATTACK" "CDLDARKCLOUDCOVER" "CDLDOJI"
   "CDLDOJISTAR" "CDLDRAGONFLYDOJI" "CDLENGULFING"
   "CDLEVENINGDOJISTAR" "CDLEVENINGSTAR" "CDLGAPSIDESIDEWHITE"
   "CDLGRAVESTONEDOJI" "CDLHAMMER" "CDLHANGINGMAN"
   "CDLHARAMI" "CDLHARAMICROSS" "CDLHIGHWAVE"
   "CDLHIKKAKE" "CDLHIKKAKEMOD" "CDLHOMINGPIGEON"
   "CDLIDENTICAL3CROWS" "CDLINNECK" "CDLINVERTEDHAMMER"
   "CDLKICKING" "CDLKICKINGBYLENGTH" "CDLLADDERBOTTOM"
   "CDLLONGLEGGEDDOJI" "CDLLONGLINE" "CDLMARUBOZU"
   "CDLMATCHINGLOW" "CDLMATHOLD" "CDLMORNINGDOJISTAR"
   "CDLMORNINGSTAR" "CDLONNECK" "CDLPIERCING"
   "CDLRICKSHAWMAN" "CDLRISEFALL3METHODS" "CDLSEPARATINGLINES"
   "CDLSHOOTINGSTAR" "CDLSHORTLINE" "CDLSPINNINGTOP"
   "CDLSTALLEDPATTERN" "CDLSTICKSANDWICH" "CDLTAKURI"
   "CDLTASUKIGAP" "CDLTHRUSTING" "CDLTRISTAR" "CDLUNIQUE3RIVER"
   "CDLUPSIDEGAP2CROWS" "CDLXSIDEGAP3METHODS"])

(defn ta-call-with-defaults
  "Вызывает TA-Lib, подставляя 0.3 для всех optInputs (это default для penetration)."
  [func-name ph]
  (let [func (com.tictactec.ta.lib.meta.CoreMetaData/getInstance func-name)
        nbOptInputs (-> func .getFuncInfo .nbOptInput)
        ;; Все японские свечи, которым нужна опция, требуют optInPenetration (default = 0.3)
        options (vec (repeat nbOptInputs 0.3))]
    (apply ta/ta func-name [ph] options)))

(defn extract-features
  "Принимает список OHLCV-мап и возвращает матрицу: для каждого дня вектор из 61 CDL-флага (-1, 0, 1)."
  [ohlcv-data]
  (let [N (count ohlcv-data)
        o (double-array (map #(get % "open" (get % "close")) ohlcv-data))
        h (double-array (map #(get % "high") ohlcv-data))
        l (double-array (map #(get % "low") ohlcv-data))
        c (double-array (map #(get % "close") ohlcv-data))
        v (double-array (map #(get % "volume") ohlcv-data))
        i-arr (double-array N)
        ph (PriceHolder. o h l c v i-arr)]
    
    ;; Вызываем все паттерны (они вернут int array)
    ;; Мы разделим значения на 100.0, чтобы нормализовать в [-1.0, 0.0, 1.0] для KAN
    (let [pattern-arrays (mapv (fn [pat-name]
                                 (let [res (ta-call-with-defaults pat-name ph)
                                       int-arr (first res)
                                       ;; meta contains :begIndex which says where it starts
                                       meta-info (meta res)
                                       beg (:begIndex meta-info)
                                       nb (:nbElements meta-info)
                                       ;; int-arr is length nb
                                       arr (object-array N)]
                                   ;; Заполняем нулями до begIndex
                                   (dotimes [i beg]
                                     (aset arr i 0.0))
                                   ;; Заполняем значениями CDL от begIndex
                                   (dotimes [i nb]
                                     (aset arr (+ beg i) (/ (double (aget ^ints int-arr i)) 100.0)))
                                   (vec arr)))
                               all-cdl-patterns)]
      ;; Транспонируем, чтобы получить [Day -> [61 Patterns]]
      ;; (apply mapv vector pattern-arrays)
      (vec (apply mapv vector pattern-arrays)))))
