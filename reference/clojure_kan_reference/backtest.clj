(ns kan-kat.backtest
  (:require [clojure.java.io :as io]
            [kan-kat.financial-timeseries-kan :as ft]
            [kan-kat.kan-framework :as fw]
            [kan-kat.tensor-v2 :as t2]
            [kan-kat.data-loader :as dl]
            [clojure-backtesting.data :as bd]
            [clojure-backtesting.data-management :as bdm]
            [clojure-backtesting.portfolio :as bp]
            [clojure-backtesting.counter :as bc]
            [clojure-backtesting.order :as bo]
            [clojure-backtesting.evaluate :as be]
            [clojure-backtesting.direct :as bdir]
            [clojure-backtesting.parameters :as bpar]))

;; ==============================================================
;; 1. Adapter: Convert KAN Dataset to clojure-backtesting format
;; ==============================================================

(defn format-date [day-idx]
  ;; Простой формат YYYY-MM-DD начиная с 2020-01-01
  (let [start-date (java.time.LocalDate/of 2020 1 1)
        curr-date (.plusDays start-date day-idx)]
    (.toString curr-date)))

(defn encode-filename [date-str]
  (let [list-str (pr-str (list date-str))
        encoder (java.util.Base64/getUrlEncoder)]
    (.encodeToString encoder (.getBytes list-str))))

(defn export-dataset-to-backtester! [ohlcv-history dir-path permno]
  "Export generated OHLCV to clojure-backtesting structure (grouped folder)."
  (let [dir (io/file dir-path)
        grouped-dir (io/file dir "grouped")]
    (when (.exists dir)
      (doseq [f (file-seq dir)]
        (when (.isFile f) (.delete f))))
    (.mkdirs grouped-dir)
    
    ;; header (must be a vector of strings or keywords)
    (let [header ["date" "PERMNO" "PRC" "RET" "VOL"]]
      (spit (io/file dir "header") (pr-str header)))
    
    ;; generate daily files
    (doseq [i (range (count ohlcv-history))]
      (let [row (nth ohlcv-history i)
            prev-row (if (> i 0) (nth ohlcv-history (dec i)) row)
            date-str (format-date i)
            encoded-name (encode-filename date-str)
            prc (get row "close")
            prev-prc (get prev-row "close")
            ret (if (zero? prev-prc) 0.0 (/ (- prc prev-prc) prev-prc))
            vol (get row "volume")
            data-vector [date-str permno prc ret vol]]
        (spit (io/file grouped-dir encoded-name) (str (pr-str data-vector) "\n"))))))

;; ==============================================================
;; Demo 52: End-to-End Institutional Backtest using KAN
;; ==============================================================

(defn demo-institutional-backtest []
  (println "\n==========================================")
  (println " Demo 52: Institutional Backtesting with KAN")
  (println "==========================================\n")
  
  (let [days 200
        raw-ohlcv (ft/generate-metal-futures days 1500.0 0.0001 0.02)
        permno "XAUUSD"
        data-dir ".backtest-data"]
        
    (println "Exporting continuous dataset to backtesting engine format...")
    (export-dataset-to-backtester! raw-ohlcv data-dir permno)
    
    (println "Loading dataset into clojure-backtesting engine...")
    ;; load dataset populates internal atoms in `clojure-backtesting`
    (bd/load-dataset data-dir "main" bd/add-aprc)
    
    ;; Let's simulate a pre-trained KAN logic. 
    ;; Normally we load a frozen model. Here we'll use a fast 1-epoch surrogate
    ;; trained on the first 80% dynamically, and trade on the 20% test set.
    
    (let [window-size 3
          dataset (ft/create-multivariate-windows raw-ohlcv window-size ["close" "volume"] "close")
          train-size (int (* 0.8 (count dataset)))
          train-data (take train-size dataset)
          test-data (drop train-size dataset)
          
          features ["close" "volume"]
          input-features (vec (for [t (range window-size) f features] (str f "_" t)))
          target-feature "target_close"
          
          x-scaler (dl/fit-minmax train-data input-features)
          y-scaler (dl/fit-minmax train-data [target-feature])
          
          train-x (dl/transform-minmax x-scaler train-data input-features)
          train-y (dl/transform-minmax y-scaler train-data [target-feature])
          
          X-train (dl/to-tensor train-x input-features)
          Y-train (dl/to-tensor train-y [target-feature])

          
          kan-model (fw/make-model [(count input-features) 5 1] 3)
          lr 0.05
          epochs 50]
      
      (println "Fast-training surrogate KAN model for signals...")
      (loop [ep 1 curr-model kan-model]
        (if (> ep epochs)
          (do
            (println "KAN model 'frozen'. Starting Backtesting over out-of-sample data...")
            (let [start-test-date (format-date (+ window-size train-size))
                  end-test-date (format-date (dec days))]
              
              ;; Setup Portfolio
              (bp/init-portfolio (format-date 0) 100000.0)
              
              ;; Move time forward manually to the test set
              (bpar/CHANGE-CACHE-SIZE 100) ; prevent cache evictions out of bound
              
              (let [test-days (count test-data)
                    num-of-days (atom test-days)]
                
                ;; Jump over train dates simply by overriding the current date?
                ;; clojure-backtesting depends on `(get-date)` which is derived from the files.
                ;; The safest way is to loop `next-date` until we hit the test start
                (while (not= (bc/get-date) start-test-date)
                  (bo/next-date))
                
                (loop [test-idx 0]
                  (when (pos? @num-of-days)
                    (let [curr-date (bc/get-date)
                          ;; Predict next price
                          raw-row (nth test-data test-idx)
                          scaled-row (first (dl/transform-minmax x-scaler [raw-row] input-features))
                          scaled-x (mapv #(get scaled-row %) input-features)
                          X-input (t2/tensor scaled-x [1 (count input-features)])
                          
                          y-pred-tensor (fw/model-forward curr-model X-input)
                          y-pred-scaled (aget ^doubles (:data y-pred-tensor) 0)
                          ;; Unscale y
                          min-y (first (:mins y-scaler))
                          max-y (first (:maxs y-scaler))
                          y-pred (+ min-y (* y-pred-scaled (- max-y min-y)))
                          
                          current-price (bdm/get-permno-price curr-date permno)]
                      
                      ;; Strategy Logic
                      (when current-price
                        (let [price (double current-price)]
                           (cond
                             ;; Buy Signal: KAN predicts a price > 1.0% jump
                             (> y-pred (* price 1.01))
                             (do
                               (bo/order permno 10 :print false)
                               (println (format "[%s] BUY 10 units @ %.2f (Pred: %.2f)" curr-date price y-pred)))
                             
                             ;; Sell Signal: KAN predicts a price drop > 1.0%
                             (< y-pred (* price 0.99))
                             (do
                               (bo/order permno -10 :print false)
                               (println (format "[%s] SELL 10 units @ %.2f (Pred: %.2f)" curr-date price y-pred)))
                             
                             :else nil)))
                      
                      (be/update-eval-report)
                      (bo/next-date)
                      (swap! num-of-days dec)
                      (recur (inc test-idx)))))
                
                ;; Tear Sheet
                (println "\n=========== TEAR SHEET ===========")
                (bdir/print-portfolio)
                (bdir/print-portfolio-record 5) ; last 5 days
                (println (format "Final Sharpe Ratio: %.4f" (be/sharpe-ratio)))
                (println (format "Max Drawdown: %.4f%%" (* 100.0 (be/max-drawdown))))
                (println "=================================="))))
          
          ;; Training step
          (let [preds (fw/model-forward curr-model X-train)
                loss-t (t2/mse-loss preds Y-train)
                loss (aget ^doubles (:data loss-t) 0)]
            (t2/backward! loss-t)
            (recur (inc ep) (fw/model-update curr-model (mapv #(t2/sgd-step! % lr 5.0) (fw/all-params curr-model))))))))))
