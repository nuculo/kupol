(ns kan-kat.dynamic-kan
  "KAN как динамическая система.
   
   ═══════════════════════════════════════════════════
   ФИЗИЧЕСКАЯ АНАЛОГИЯ
   ═══════════════════════════════════════════════════
   
   Loss = ЭНЕРГИЯ (потенциальная)
   Параметры = КООРДИНАТЫ частицы
   Градиент = СИЛА, действующая на частицу
   Momentum = ИНЕРЦИЯ (масса × скорость)
   Friction = диссипация (чтобы система останавливалась)
   
   Обучение = решение ODE:
     dv/dt = -∇E(θ) - γ·v     (сила - трение)
     dθ/dt = v                  (скорость)
   
   Clojure: `iterate` → ленивая бесконечная траектория.
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.phi-protocol :as phi]))

;; ============================================================
;; СОСТОЯНИЕ СИСТЕМЫ
;; ============================================================

(defn make-particle
  "Создаёт «частицу» в пространстве параметров.
   
   phi      — PhiFunction (текущая позиция = параметры)
   target   — целевая функция f(x)
   data     — обучающие точки [[x₁ y₁] ...]"
  [a-phi target-fn data]
  (let [params (phi/phi-params a-phi)
        n      (count params)]
    {:phi       a-phi
     :params    params
     :velocity  (vec (repeat n 0.0))   ; начальная скорость = 0
     :energy    nil                     ; вычислим на первом шаге
     :time      0.0
     :step      0
     :target-fn target-fn
     :data      data}))

;; ============================================================
;; ЭНЕРГИЯ (= Loss)
;; ============================================================

(defn compute-energy
  "Потенциальная энергия = MSE на данных."
  [state]
  (let [f    (fn [x] (phi/phi-forward (:phi state) x))
        data (:data state)]
    (/ (reduce + 0.0
         (map (fn [[x y]]
                (let [err (- (f x) y)]
                  (* err err)))
              data))
       (count data))))

;; ============================================================
;; СИЛА (= -∇Energy)
;; ============================================================

(defn compute-force
  "Сила = отрицательный градиент энергии по параметрам.
   F = -∂E/∂θ"
  [state]
  (let [params (:params state)
        eps    1e-4
        e0     (compute-energy state)]
    (mapv (fn [i]
            (let [p+ (assoc params i (+ (nth params i) eps))
                  s+ (assoc state :phi (phi/phi-update (:phi state) p+)
                                  :params p+)
                  e+ (compute-energy s+)]
              ;; F = -dE/dθ
              (- (/ (- e+ e0) eps))))
          (range (count params)))))

;; ============================================================
;; ИНТЕГРАТОР (Velocity Verlet / Leapfrog)
;; ============================================================

(defn verlet-step
  "Один шаг Velocity Verlet интегратора.
   
   dt      — шаг по времени
   friction — коэффициент трения (0.0-1.0)
   
   Уравнения:
     v(t+dt/2) = v(t) + (dt/2)·F(t)/m - γ·v(t)·dt
     θ(t+dt)   = θ(t) + dt·v(t+dt/2)
     F(t+dt)   = force at θ(t+dt)
     v(t+dt)   = v(t+dt/2) + (dt/2)·F(t+dt)/m"
  [state dt friction]
  (let [force    (compute-force state)
        ;; Half-step velocity
        v-half   (mapv (fn [vi fi]
                         (+ vi (* 0.5 dt fi) (* (- friction) vi dt)))
                       (:velocity state) force)
        ;; Full-step position
        new-params (mapv (fn [pi vi] (+ pi (* dt vi)))
                         (:params state) v-half)
        new-phi  (phi/phi-update (:phi state) new-params)
        ;; Force at new position
        new-state (assoc state :phi new-phi :params new-params)
        new-force (compute-force new-state)
        ;; Full-step velocity
        v-full   (mapv (fn [vi fi] (+ vi (* 0.5 dt fi)))
                       v-half new-force)
        ;; Kinetic energy
        kinetic  (* 0.5 (reduce + 0.0 (map #(* % %) v-full)))
        potential (compute-energy new-state)]
    (assoc new-state
           :velocity  v-full
           :energy    potential
           :kinetic   kinetic
           :total-energy (+ potential kinetic)
           :time      (+ (:time state) dt)
           :step      (inc (:step state)))))

;; ============================================================
;; ЛЕНИВАЯ ТРАЕКТОРИЯ (iterate)
;; ============================================================

(defn trajectory
  "Бесконечная ленивая последовательность состояний.
   
   Как наблюдать за маятником: каждый элемент = snapshot системы.
   
   Использование:
     (take 100 (trajectory particle 0.01 0.1))"
  [initial-state dt friction]
  (iterate #(verlet-step % dt friction) initial-state))

;; ============================================================
;; ОБУЧЕНИЕ КАК ФИЗИЧЕСКИЙ ПРОЦЕСС
;; ============================================================

(defn train-dynamic
  "Обучает φ как физическую систему.
   
   Частица скатывается в минимум энергии (loss).
   Трение обеспечивает сходимость (без трения — вечное качание).
   
   phi-type — тип φ
   opts     — опции
   target   — целевая f(x)
   domain   — [lo hi]
   n-points — количество точек
   steps    — шагов интегрирования
   dt       — шаг по времени
   friction — коэффициент"
  [phi-type opts target-fn domain n-points steps dt friction]
  (let [data (mapv (fn [_]
                     (let [x (+ (first domain)
                                (* (rand) (- (second domain) (first domain))))]
                       [x (target-fn x)]))
                   (range n-points))
        a-phi    (phi/make-phi phi-type opts)
        particle (make-particle a-phi target-fn data)
        ;; Берём trajectory, наблюдаем
        states  (take (inc steps) (trajectory particle dt friction))]
    (println "  ╔══════════════════════════════════════╗")
    (println "  ║  Dynamic System: φ → минимум энергии ║")
    (println "  ╚══════════════════════════════════════╝")
    (println (format "  Type: %s | Params: %d | dt=%.3f | γ=%.2f"
                     (name phi-type) (count (phi/phi-params a-phi)) dt friction))
    (println "  ────────────────────────────────────────")
    (println "  Step  | t      | Energy   | Kinetic  | Total")
    (println "  ──────|────────|──────────|──────────|──────────")
    (let [final
          (reduce (fn [_ s]
                    (when (and (pos? (:step s))
                               (zero? (mod (:step s) (max 1 (quot steps 8)))))
                      (println (format "  %5d | %6.3f | %8.6f | %8.6f | %8.6f"
                                       (:step s) (:time s)
                                       (or (:energy s) 0.0)
                                       (or (:kinetic s) 0.0)
                                       (or (:total-energy s) 0.0))))
                    s)
                  nil
                  states)]
      (println "  ────────────────────────────────────────")
      (println (format "  Final energy (loss): %.6f" (compute-energy final)))
      (println (format "  Speed |v|: %.6f"
                       (math/sqrt (reduce + 0.0
                                    (map #(* % %) (:velocity final))))))
      final)))
