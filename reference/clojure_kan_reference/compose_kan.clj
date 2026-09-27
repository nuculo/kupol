(ns kan-kat.compose-kan
  "Композиция автоморфных функций для KAN.
   
   ═══════════════════════════════════════════════════
   ИДЕЯ: КАЛЕЙДОСКОП
   ═══════════════════════════════════════════════════
   
   Каждое зеркало калейдоскопа = автоморфизм φ: Rⁿ → Rⁿ.
   Композиция зеркал = сложный узор из простых.
   
   Отличие от automorphic-kan (Normalizing Flow):
   - NF использует coupling layers (split/merge)
   - Здесь: элементарные автоморфизмы (поворот, масштаб,
     сдвиг, нелинейная деформация), каждый ТОЧНО обратимый
   
   Преимущества:
   - Якобиан вычисляется аналитически (бесплатно)
   - Любая композиция обратима (just reverse + invert)
   - Функции = first-class → compose/invert как map/reduce
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]))

;; ============================================================
;; АВТОМОРФИЗМ = {:forward, :inverse, :log-det-jac}
;; ============================================================

(defn make-automorphism
  "Создаёт автоморфизм — тройку (forward, inverse, log-det-jacobian).
   
   forward     : x → y
   inverse     : y → x (точная обратная)
   log-det-jac : x → log|det(∂y/∂x)| (для density estimation)"
  [forward-fn inverse-fn log-det-jac-fn name-str]
  {:forward     forward-fn
   :inverse     inverse-fn
   :log-det-jac log-det-jac-fn
   :name        name-str})

;; ============================================================
;; ЭЛЕМЕНТАРНЫЕ АВТОМОРФИЗМЫ
;; ============================================================

;; --- 1. Масштабирование (Scale) ---
(defn scale-auto
  "Масштабирование: y = s·x, s > 0.
   Jacobian = diag(s), log|det| = Σ log|s_i|."
  [s-vec]
  (let [log-det (reduce + 0.0 (map #(math/log (abs %)) s-vec))]
    (make-automorphism
      (fn [x] (mapv * x s-vec))
      (fn [y] (mapv / y s-vec))
      (fn [_x] log-det)
      (format "Scale(%s)" (pr-str (mapv #(format "%.2f" %) s-vec))))))

;; --- 2. Сдвиг (Translation) ---
(defn translate-auto
  "Сдвиг: y = x + b. Jacobian = I, log|det| = 0."
  [b-vec]
  (make-automorphism
    (fn [x] (mapv + x b-vec))
    (fn [y] (mapv - y b-vec))
    (fn [_x] 0.0)
    (format "Translate(%s)" (pr-str (mapv #(format "%.2f" %) b-vec)))))

;; --- 3. Поворот 2D (Rotation) ---
(defn rotate-2d-auto
  "Поворот на θ в плоскости (i,j). Jacobian = 1 (det=1 для ортог.)."
  [theta i j dim]
  (let [c (math/cos theta)
        s (math/sin theta)]
    (make-automorphism
      (fn [x]
        (let [xi (nth x i)
              xj (nth x j)]
          (-> x
              (assoc i (- (* c xi) (* s xj)))
              (assoc j (+ (* s xi) (* c xj))))))
      (fn [y]
        (let [yi (nth y i)
              yj (nth y j)]
          (-> y
              (assoc i (+ (* c yi) (* s yj)))
              (assoc j (+ (- (* s yi)) (* c yj))))))
      (fn [_x] 0.0) ; det(rotation) = 1, log = 0
      (format "Rotate(%.2f, %d, %d)" theta i j))))

;; --- 4. LeakyReLU (поэлементная, обратимая) ---
(defn leaky-relu-auto
  "LeakyReLU: y = x if x>0, y = α·x if x≤0. Обратимо при α≠0."
  [alpha]
  (make-automorphism
    (fn [x] (mapv #(if (pos? %) % (* alpha %)) x))
    (fn [y] (mapv #(if (pos? %) % (/ % alpha)) y))
    (fn [x]
      ;; log|det| = Σ log(1 if x>0, α if x≤0)
      (reduce + 0.0
        (map #(if (pos? %) 0.0 (math/log (abs alpha))) x)))
    (format "LeakyReLU(α=%.2f)" alpha)))

;; --- 5. Exp/Log (поэлементная) ---
(defn exp-auto
  "Exp: y = exp(x). Inverse: x = log(y). log|det| = Σ x_i."
  []
  (make-automorphism
    (fn [x] (mapv #(math/exp (min % 10.0)) x))
    (fn [y] (mapv #(math/log (max % 1e-10)) y))
    (fn [x] (reduce + 0.0 (map #(min % 10.0) x)))
    "Exp"))

;; --- 6. Tanh (обратимая на (-1,1)) ---
(defn tanh-auto
  "Tanh: y = tanh(x). Inverse: x = arctanh(y).
   log|det| = Σ log(1 - tanh²(x))."
  []
  (make-automorphism
    (fn [x] (mapv math/tanh x))
    (fn [y] (mapv #(* 0.5 (math/log (/ (+ 1.0 %) (max 1e-10 (- 1.0 %))))) y))
    (fn [x]
      (reduce + 0.0
        (map #(let [t (math/tanh %)]
                (math/log (max 1e-10 (- 1.0 (* t t)))))
             x)))
    "Tanh"))

;; ============================================================
;; КОМПОЗИЦИЯ (калейдоскоп)
;; ============================================================

(defn compose
  "Композиция автоморфизмов: (f₃ ∘ f₂ ∘ f₁)(x).
   Forward: применяем слева направо.
   Inverse: применяем справа налево обратные."
  [& autos]
  (let [auto-vec (vec autos)]
    (make-automorphism
      ;; Forward: f₁ → f₂ → f₃
      (fn [x]
        (reduce (fn [state a] ((:forward a) state))
                x auto-vec))
      ;; Inverse: f₃⁻¹ → f₂⁻¹ → f₁⁻¹
      (fn [y]
        (reduce (fn [state a] ((:inverse a) state))
                y (reverse auto-vec)))
      ;; Log-det: Σ log|det J_i|
      (fn [x]
        (let [[_ total-ld]
              (reduce (fn [[state ld] a]
                        [((:forward a) state)
                         (+ ld ((:log-det-jac a) state))])
                      [x 0.0]
                      auto-vec)]
          total-ld))
      (str "Compose(" (clojure.string/join " ∘ " (map :name auto-vec)) ")"))))

;; ============================================================
;; ВЕРИФИКАЦИЯ ОБРАТИМОСТИ
;; ============================================================

(defn verify-invertibility
  "Проверяет: f⁻¹(f(x)) ≈ x."
  [auto x]
  (let [y         ((:forward auto) x)
        x-back    ((:inverse auto) y)
        err       (reduce + 0.0 (map #(abs (- %1 %2)) x x-back))
        log-det   ((:log-det-jac auto) x)]
    {:name       (:name auto)
     :input      x
     :output     y
     :roundtrip  x-back
     :error      err
     :log-det    log-det
     :exact?     (< err 1e-10)}))

;; ============================================================
;; KAN-КАЛЕЙДОСКОП: параметрический автоморфный слой
;; ============================================================

(defn make-kaleidoscope
  "Создаёт калейдоскоп: настраиваемая композиция автоморфизмов.
   
   dim    — размерность пространства
   params — {:scales, :shifts, :angles, :alpha}
   
   Архитектура: Scale → Rotate → LeakyReLU → Translate → Tanh"
  [dim params]
  (let [{:keys [scales shifts angles alpha]
         :or   {scales (vec (repeat dim 1.0))
                shifts (vec (repeat dim 0.0))
                angles []
                alpha  0.1}} params
        ;; Собираем калейдоскоп
        layers (concat
                 [(scale-auto scales)]
                 (for [[theta i j] angles]
                   (rotate-2d-auto theta i j dim))
                 [(leaky-relu-auto alpha)
                  (translate-auto shifts)
                  (tanh-auto)])]
    (apply compose layers)))

;; ============================================================
;; ДЕМО-УТИЛИТЫ
;; ============================================================

(defn demo-kaleidoscope
  "Демо: построить калейдоскоп, проверить обратимость."
  [dim]
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  Kaleidoscope: automorphic compose   ║")
  (println "  ╚══════════════════════════════════════╝")
  
  ;; 1. Элементарные автоморфизмы
  (println (format "\n  Dimension: %d" dim))
  (println "  Testing elementary automorphisms:\n")
  
  (let [test-x (mapv (fn [i] (* 0.5 (inc i))) (range dim))
        autos  [(scale-auto (vec (repeat dim 2.0)))
                (translate-auto (vec (repeat dim 0.3)))
                (leaky-relu-auto 0.1)
                (tanh-auto)]]
    ;; Test each
    (doseq [a autos]
      (let [r (verify-invertibility a test-x)]
        (println (format "    %-20s | err: %.2e | log|det|: %7.4f | %s"
                         (:name r) (:error r) (:log-det r)
                         (if (:exact? r) "✅" "⚠️")))))
    
    ;; 2. Compose all
    (println "\n  Composing all into kaleidoscope:")
    (let [kaleidoscope (apply compose autos)
          r (verify-invertibility kaleidoscope test-x)]
      (println (format "    %s" (:name r)))
      (println (format "    Input:     %s" (pr-str (mapv #(format "%.4f" %) (:input r)))))
      (println (format "    Output:    %s" (pr-str (mapv #(format "%.4f" %) (:output r)))))
      (println (format "    Roundtrip: %s" (pr-str (mapv #(format "%.4f" %) (:roundtrip r)))))
      (println (format "    Error:     %.2e" (:error r)))
      (println (format "    Log|det|:  %.4f" (:log-det r)))
      (println (format "    Exact:     %s" (if (:exact? r) "✅ YES" "⚠️ approx")))
      r)))
