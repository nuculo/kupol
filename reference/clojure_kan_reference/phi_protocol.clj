(ns kan-kat.phi-protocol
  "Протокол PhiFunction — расширяемые функции активации для KAN.
   
   ═══════════════════════════════════════════════════
   МОТИВАЦИЯ
   ═══════════════════════════════════════════════════
   
   KAN использует обучаемые одномерные функции φ_{j,i}(x)
   на каждом ребре. До сих пор у нас был только B-spline.
   
   Протокол PhiFunction позволяет plug-and-play:
   - BSplinePhi   — Cox-de Boor B-spline (наш основной)
   - PolyPhi      — полином (быстрый, для простых задач)
   - RationalPhi  — p(x)/q(x) (вдохновлён GR-KAN, ICLR 2025)
   
   Все реализации поддерживают forward, backward, params.
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.spline :as spl]))

;; ============================================================
;; ПРОТОКОЛ
;; ============================================================

(defprotocol PhiFunction
  "Обучаемая одномерная функция φ(x) для KAN edge."
  (phi-forward  [this x]
    "Вычислить φ(x) → число.")
  (phi-backward [this x]
    "Вычислить dφ/dx → число (производная по входу).")
  (phi-grad     [this x]
    "Вычислить ∂φ/∂params → вектор (градиент по параметрам).")
  (phi-params   [this]
    "Извлечь обучаемые параметры → вектор.")
  (phi-update   [this new-params]
    "Инъекция новых параметров → новый PhiFunction."))

;; ============================================================
;; B-SPLINE PHI (основной, как в KAN v2)
;; ============================================================

(defrecord BSplinePhi [coeffs knots order wb ws]
  PhiFunction
  (phi-forward [this x]
    (let [;; SiLU base
          sig   (/ 1.0 (+ 1.0 (math/exp (- x))))
          base  (* x sig)
          ;; B-spline
          basis (spl/eval-splines-at order knots x)
          spline (reduce + 0.0 (map * coeffs basis))]
      (+ (* wb base) (* ws spline))))
  
  (phi-backward [this x]
    (let [;; d(SiLU)/dx = σ(x) + x·σ(x)·(1-σ(x))
          sig    (/ 1.0 (+ 1.0 (math/exp (- x))))
          d-silu (+ sig (* x sig (- 1.0 sig)))
          ;; B-spline derivative (numerical, ε=1e-5)
          eps    1e-5
          sp+    (reduce + 0.0 (map * coeffs (spl/eval-splines-at order knots (+ x eps))))
          sp-    (reduce + 0.0 (map * coeffs (spl/eval-splines-at order knots (- x eps))))
          d-spl  (/ (- sp+ sp-) (* 2.0 eps))]
      (+ (* wb d-silu) (* ws d-spl))))
  
  (phi-grad [this x]
    (let [sig   (/ 1.0 (+ 1.0 (math/exp (- x))))
          base  (* x sig)
          basis (spl/eval-splines-at order knots x)
          spline (reduce + 0.0 (map * coeffs basis))]
      ;; ∂φ/∂wb = SiLU(x), ∂φ/∂ws = spline(x), ∂φ/∂c_i = ws·B_i(x)
      (vec (concat [base spline]
                   (mapv #(* ws %) basis)))))
  
  (phi-params [this]
    (vec (concat [wb ws] coeffs)))
  
  (phi-update [this new-params]
    (let [wb'     (nth new-params 0)
          ws'     (nth new-params 1)
          coeffs' (subvec new-params 2)]
      (->BSplinePhi coeffs' knots order wb' ws'))))

(defn make-bspline-phi
  "Создание BSplinePhi с случайной инициализацией."
  [order grid-size grid-range]
  (let [knots    (spl/init-knots order grid-size grid-range)
        n-coeffs (- (count knots) order 1)
        coeffs   (vec (repeatedly n-coeffs #(* 0.1 (- (rand) 0.5))))
        wb       (- (rand) 0.5)
        ws       (* 0.1 (- (rand) 0.5))]
    (->BSplinePhi coeffs knots order wb ws)))

;; ============================================================
;; POLYNOMIAL PHI (быстрый, для простых задач)
;; ============================================================

(defrecord PolyPhi [coeffs]
  PhiFunction
  (phi-forward [this x]
    ;; φ(x) = c₀ + c₁·x + c₂·x² + ... (Horner)
    (reduce (fn [acc c] (+ c (* acc x)))
            0.0
            (reverse coeffs)))
  
  (phi-backward [this x]
    ;; dφ/dx = c₁ + 2c₂·x + 3c₃·x² + ...
    (reduce (fn [acc [i c]] (+ acc (* i c (math/pow x (dec i)))))
            0.0
            (map-indexed vector (rest coeffs))))
  
  (phi-grad [this x]
    ;; ∂φ/∂c_i = x^i
    (mapv #(math/pow x %) (range (count coeffs))))
  
  (phi-params [this] coeffs)
  
  (phi-update [this new-params]
    (->PolyPhi (vec new-params))))

(defn make-poly-phi
  "Создание PolyPhi степени degree."
  [degree]
  (->PolyPhi (vec (repeatedly (inc degree) #(* 0.1 (- (rand) 0.5))))))

;; ============================================================
;; RATIONAL PHI (вдохновлён GR-KAN / Padé)
;; ============================================================

(defrecord RationalPhi [p-coeffs q-coeffs]
  PhiFunction
  (phi-forward [this x]
    ;; φ(x) = P(x) / (1 + |Q(x)|), Q нормализован для стабильности
    (let [px (reduce (fn [acc c] (+ c (* acc x))) 0.0 (reverse p-coeffs))
          qx (reduce (fn [acc c] (+ c (* acc x))) 0.0 (reverse q-coeffs))]
      (/ px (+ 1.0 (abs qx)))))
  
  (phi-backward [this x]
    ;; Numerical derivative (стабильнее чем аналитический для rational)
    (let [eps 1e-5]
      (/ (- (phi-forward this (+ x eps))
            (phi-forward this (- x eps)))
         (* 2.0 eps))))
  
  (phi-grad [this x]
    ;; Numerical param gradients
    (let [params (phi-params this)
          eps    1e-5
          f0     (phi-forward this x)]
      (mapv (fn [i]
              (let [p+ (assoc params i (+ (nth params i) eps))
                    p- (assoc params i (- (nth params i) eps))
                    f+ (phi-forward (phi-update this p+) x)
                    f- (phi-forward (phi-update this p-) x)]
                (/ (- f+ f-) (* 2.0 eps))))
            (range (count params)))))
  
  (phi-params [this]
    (vec (concat p-coeffs q-coeffs)))
  
  (phi-update [this new-params]
    (let [np (count p-coeffs)]
      (->RationalPhi (subvec new-params 0 np)
                     (subvec new-params np)))))

(defn make-rational-phi
  "Создание RationalPhi: P(x)/Q(x) степени p-deg / q-deg."
  [p-deg q-deg]
  (->RationalPhi (vec (repeatedly (inc p-deg) #(* 0.1 (- (rand) 0.5))))
                 (vec (repeatedly (inc q-deg) #(* 0.1 (- (rand) 0.5))))))

;; ============================================================
;; ФАБРИКА
;; ============================================================

(defn make-phi
  "Фабрика: создаёт PhiFunction по типу.
   
   :bspline  {:order 3 :grid-size 5 :grid-range [-1 1]}
   :poly     {:degree 3}
   :rational {:p-deg 3 :q-deg 2}"
  [phi-type opts]
  (case phi-type
    :bspline  (make-bspline-phi
                (get opts :order 3)
                (get opts :grid-size 5)
                (get opts :grid-range [-1.0 1.0]))
    :poly     (make-poly-phi (get opts :degree 3))
    :rational (make-rational-phi
                (get opts :p-deg 3)
                (get opts :q-deg 2))
    (throw (ex-info (str "Unknown phi type: " phi-type) {:type phi-type}))))
