(ns kan-kat.numerical-gradient
  "Numerical gradient computation via finite differences — DEBUG UTILITY.
   Verifies correctness of the AD engine by comparing with numerical derivatives."
  (:require [clojure.math :as math]
            [kan-kat.kan-layer :as kan]
            [kan-kat.math :as m]
            [kan-kat.training :as tr]))

(defn numerical-gradient
  "Compute gradient numerically via central finite differences."
  ([f params] (numerical-gradient f params 1e-5))
  ([f params eps]
   (let [n (count params)]
     (mapv (fn [i]
             (let [params+  (assoc params i (+ (nth params i) eps))
                   params-  (assoc params i (- (nth params i) eps))
                   loss+    (f params+)
                   loss-    (f params-)]
               (/ (- loss+ loss-) (* 2.0 eps))))
           (range n)))))

(defn kan-loss-fn
  "Create a loss function for a KAN layer given input and target.
   Returns f: flat-params → MSE loss (Double)."
  [layer input target]
  (fn [params]
    (let [layer' (kan/inject-params layer params)
          pred   (kan/forward-layer layer' input)]
      (tr/mse-loss-double pred target))))

(defn verify-ad-gradient
  "Compare AD gradient with numerical gradient.
   Prints per-parameter comparison and max absolute error.
   Returns true if all errors < tolerance."
  ([layer input target] (verify-ad-gradient layer input target 1e-3))
  ([layer input target tolerance]
   (let [ad-grad  (tr/kan-gradient layer input target)
         loss-fn  (kan-loss-fn layer input target)
         num-grad (numerical-gradient loss-fn (kan/extract-params layer))
         errors   (mapv (fn [a n] (abs (- a n))) ad-grad num-grad)
         max-err  (if (empty? errors) 0.0 (apply max errors))
         n-params (count ad-grad)]
     (println "╔══════════════════════════════════════════╗")
     (println "║  AD vs Numerical Gradient Verification   ║")
     (println "╚══════════════════════════════════════════╝")
     (println (str "  Parameters: " n-params
                   " (wb + ws + coeffs + ln-gamma + ln-beta)"))
     (println)
     (println "  Param# |     AD grad  |    Num grad  |    Error")
     (println "  -------|--------------|--------------|----------")
     (doseq [i (range (min 20 n-params))]
       (println (format "  %5d  | %12.6f | %12.6f | %.2e"
                        i (nth ad-grad i) (nth num-grad i) (nth errors i))))
     (when (> n-params 20)
       (println (str "  ... (" (- n-params 20) " more parameters)")))
     (println)
     (println (str "  Max absolute error: " (format "%.2e" max-err)))
     (println (str "  Tolerance:          " (format "%.2e" tolerance)))
     (println (str "  Result:             " (if (<= max-err tolerance) "✅ PASS" "❌ FAIL")))
     (println)
     (<= max-err tolerance))))
