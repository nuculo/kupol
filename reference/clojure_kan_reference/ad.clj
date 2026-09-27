(ns kan-kat.ad
  "Forward-mode Automatic Differentiation via Dual Numbers.
   
   A Dual number (a + a'ε) carries both a value and its derivative,
   where ε² = 0. By evaluating f(Dual(x,1)), we get Dual(f(x), f'(x)).
   
   This enables automatic gradient computation for KAN training:
   one forward pass per parameter gives the partial derivative.
   
   Dual numbers are represented as maps {:primal p :tangent t}
   and all arithmetic is done through helper functions (d-add, d-mul, etc.)
   since Clojure doesn't have operator overloading like Haskell."
  (:require [clojure.math :as math]))

;; ============================================================
;; Dual number constructors
;; ============================================================

(defn dual
  "Create a Dual number."
  [primal tangent]
  {:primal primal :tangent tangent})

(defn const-d
  "Lift a constant to Dual (zero derivative)."
  [x]
  {:primal (double x) :tangent 0.0})

(defn var-d
  "Create a variable Dual (derivative = 1)."
  [x]
  {:primal (double x) :tangent 1.0})

;; ============================================================
;; Type checking
;; ============================================================

(defn dual?
  "Check if x is a Dual number."
  [x]
  (and (map? x) (contains? x :primal)))

(defn ensure-dual
  "Convert a plain number to Dual if needed."
  [x]
  (if (dual? x) x (const-d x)))

;; ============================================================
;; Arithmetic operations (Dual number aware)
;; ============================================================

(defn d-add
  "Addition: (a + a'ε) + (b + b'ε) = (a+b) + (a'+b')ε"
  [x y]
  (let [x (ensure-dual x) y (ensure-dual y)]
    (dual (+ (:primal x) (:primal y))
          (+ (:tangent x) (:tangent y)))))

(defn d-sub
  "Subtraction."
  [x y]
  (let [x (ensure-dual x) y (ensure-dual y)]
    (dual (- (:primal x) (:primal y))
          (- (:tangent x) (:tangent y)))))

(defn d-mul
  "Multiplication: (a + a'ε)(b + b'ε) = ab + (a'b + ab')ε"
  [x y]
  (let [x (ensure-dual x) y (ensure-dual y)]
    (dual (* (:primal x) (:primal y))
          (+ (* (:tangent x) (:primal y))
             (* (:primal x) (:tangent y))))))

(defn d-div
  "Division: (a + a'ε) / (b + b'ε) = a/b + (a'b - ab')/(b²)ε"
  [x y]
  (let [x (ensure-dual x) y (ensure-dual y)
        a (:primal x) a' (:tangent x)
        b (:primal y) b' (:tangent y)]
    (dual (/ a b)
          (/ (- (* a' b) (* a b')) (* b b)))))

(defn d-negate
  "Negation."
  [x]
  (let [x (ensure-dual x)]
    (dual (- (:primal x)) (- (:tangent x)))))

;; ============================================================
;; Math functions (Dual number aware) — using clojure.math
;; ============================================================

(defn d-exp
  "exp(a + a'ε) = exp(a) + a'*exp(a)ε"
  [x]
  (let [x (ensure-dual x)
        ea (math/exp (:primal x))]
    (dual ea (* (:tangent x) ea))))

(defn d-log
  "log(a + a'ε) = log(a) + (a'/a)ε"
  [x]
  (let [x (ensure-dual x)]
    (dual (math/log (:primal x))
          (/ (:tangent x) (:primal x)))))

(defn d-sqrt
  "sqrt(a + a'ε) = sqrt(a) + a'/(2*sqrt(a))ε"
  [x]
  (let [x (ensure-dual x)
        sa (math/sqrt (:primal x))]
    (dual sa (/ (:tangent x) (* 2.0 sa)))))

(defn d-sin
  [x]
  (let [x (ensure-dual x)]
    (dual (math/sin (:primal x))
          (* (:tangent x) (math/cos (:primal x))))))

(defn d-cos
  [x]
  (let [x (ensure-dual x)]
    (dual (math/cos (:primal x))
          (* (- (:tangent x)) (math/sin (:primal x))))))

(defn d-tanh
  [x]
  (let [x (ensure-dual x)
        t (math/tanh (:primal x))]
    (dual t (* (:tangent x) (- 1.0 (* t t))))))

(defn d-abs
  [x]
  (let [x (ensure-dual x)
        a (:primal x)]
    (dual (abs a) (* (:tangent x) (Math/signum a)))))

;; ============================================================
;; Activation functions (Dual aware)
;; ============================================================

(defn d-silu
  "SiLU: x / (1 + exp(-x))"
  [x]
  (d-div x (d-add (const-d 1.0) (d-exp (d-negate x)))))

;; ============================================================
;; Comparison (on primal only, for B-spline conditionals)
;; ============================================================

(defn d-val
  "Extract primal value."
  [x]
  (if (dual? x) (:primal x) (double x)))

(defn d-le [x y] (<= (d-val x) (d-val y)))
(defn d-lt [x y] (< (d-val x) (d-val y)))
(defn d-eq [x y] (== (d-val x) (d-val y)))

;; ============================================================
;; Dual-aware vector operations
;; ============================================================

(defn d-dot-product
  "Dot product of two vectors of Duals."
  [v1 v2]
  (reduce d-add (const-d 0.0) (map d-mul v1 v2)))

(defn d-vec-add
  [v1 v2]
  (mapv d-add v1 v2))

(defn d-sum
  "Sum of a vector of Duals."
  [vs]
  (reduce d-add (const-d 0.0) vs))

;; ============================================================
;; Gradient computation
;; ============================================================

(defn compute-gradient
  "Compute gradient of f : R^n → R using forward-mode AD.
   For each parameter i, set its tangent to 1 and all others to 0,
   then read the tangent of the output. Requires n forward passes."
  [f params]
  (let [n (count params)]
    (mapv (fn [i]
            (let [seed (mapv (fn [j]
                              (if (= i j)
                                (var-d (nth params j))
                                (const-d (nth params j))))
                            (range n))]
              (:tangent (f seed))))
          (range n))))
