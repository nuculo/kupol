(ns kan-kat.math
  "Core math operations for KAN/KAT.
   All operations work on Clojure vectors of doubles."
  (:require [clojure.math :as math]))

;; ============================================================
;; Vector operations
;; ============================================================

(defn dot-product
  "Dot product of two vectors."
  [v1 v2]
  (reduce + 0.0 (map * v1 v2)))

(defn vec-add
  "Element-wise addition of two vectors."
  [v1 v2]
  (mapv + v1 v2))

(defn vec-sub
  "Element-wise subtraction of two vectors."
  [v1 v2]
  (mapv - v1 v2))

(defn vec-scale
  "Scale a vector by a scalar."
  [s v]
  (mapv #(* s %) v))

;; ============================================================
;; Matrix operations
;; ============================================================

(defn transpose
  "Transpose a matrix (vector of vectors)."
  [m]
  (if (empty? m) []
    (apply mapv vector m)))

(defn mat-mul
  "Matrix multiplication: A [m×k] × B [k×n] → C [m×n]."
  [a b]
  (let [bt (transpose b)]
    (mapv (fn [row]
            (mapv (fn [col] (dot-product row col)) bt))
          a)))

(defn mat-add
  "Element-wise matrix addition."
  [a b]
  (mapv vec-add a b))

(defn mat-scale
  "Scale all elements of a matrix by a scalar."
  [s m]
  (mapv (fn [row] (mapv #(* s %) row)) m))

;; ============================================================
;; Activation functions
;; ============================================================

(defn sigmoid
  "Logistic sigmoid: 1 / (1 + exp(-x))."
  [x]
  (/ 1.0 (+ 1.0 (math/exp (- x)))))

(defn silu
  "SiLU (Swish): x * σ(x) = x / (1 + exp(-x))."
  [x]
  (/ x (+ 1.0 (math/exp (- x)))))

(defn gelu
  "GELU approximation: 0.5 * x * (1 + tanh(√(2/π) * (x + 0.044715 * x³)))."
  [x]
  (let [c 0.7978845608028654]  ;; sqrt(2/pi)
    (* 0.5 x (+ 1.0 (math/tanh (* c (+ x (* 0.044715 x x x))))))))

;; ============================================================
;; Utility
;; ============================================================

(defn chunks-of
  "Split a sequence into chunks of size n."
  [n coll]
  (if (empty? coll) []
    (let [[h t] (split-at n coll)]
      (cons (vec h) (lazy-seq (chunks-of n t))))))

(defn softmax-vec
  "Numerically stable softmax over a vector."
  [v]
  (let [max-v (apply max v)
        exps  (mapv #(math/exp (- % max-v)) v)
        s     (reduce + 0.0 exps)]
    (mapv #(/ % s) exps)))

(defn argmax
  "Index of the maximum element."
  [v]
  (first (apply max-key second (map-indexed vector v))))
