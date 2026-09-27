(ns kan-kat.spline
  "B-Spline evaluation using Cox-de Boor recursion.
   B-splines form the learnable activation functions on each edge
   of the KAN network, replacing fixed activations like ReLU.")

(defn b-spline
  "Cox-de Boor recursion for B-Spline basis function N_{i,k}(x).
   
   Base case (k=0): N_{i,0}(x) = 1 if t_i <= x < t_{i+1}, else 0
   Recursion:
     N_{i,k}(x) = ((x - t_i)/(t_{i+k} - t_i)) * N_{i,k-1}(x)
                + ((t_{i+k+1} - x)/(t_{i+k+1} - t_{i+1})) * N_{i+1,k-1}(x)"
  [i k x knots]
  (if (zero? k)
    ;; Base case
    (let [t-i   (nth knots i)
          t-i1  (nth knots (inc i))]
      (if (or (and (<= t-i x) (< x t-i1))
              (and (<= t-i x) (= x t-i1)
                   (= i (- (count knots) 2))))
        1.0
        0.0))
    ;; Recursive case
    (let [t-i   (nth knots i)
          t-i1  (nth knots (inc i))
          t-ik  (nth knots (+ i k))
          t-ik1 (nth knots (+ i k 1))
          denom1 (- t-ik t-i)
          term1  (if (zero? denom1) 0.0
                   (* (/ (- x t-i) denom1)
                      (b-spline i (dec k) x knots)))
          denom2 (- t-ik1 t-i1)
          term2  (if (zero? denom2) 0.0
                   (* (/ (- t-ik1 x) denom2)
                      (b-spline (inc i) (dec k) x knots)))]
      (+ term1 term2))))

(defn eval-splines-at
  "Evaluate all B-spline basis functions at a single point x.
   Returns [N_0(x), N_1(x), ..., N_{n-1}(x)]."
  [k knots x]
  (let [num-spl (- (count knots) k 1)]
    (mapv #(b-spline % k x knots) (range num-spl))))

(defn init-knots
  "Initialize a uniform knot vector for B-splines.
   Creates (grid-size + 2*order + 1) knots with extended boundaries."
  [k grid-size [grid-min grid-max]]
  (let [step      (/ (- grid-max grid-min) (double grid-size))
        start     (- grid-min (* k step))
        num-knots (+ grid-size (* 2 k) 1)]
    (mapv #(+ start (* % step)) (range num-knots))))
