(ns kan-kat.training-monitor
  "Runtime Training Monitor для KAN.
   
   ═══════════════════════════════════════════════════
   Фаза 37: Визуализация и отладка обучения
   
   - ASCII loss curves (терминальные графики)
   - Gradient norm отслеживание (vanishing/exploding)
   - LR schedule visualization
   - Timing per epoch
   - Dashboard: всё в одном отчёте
   ═══════════════════════════════════════════════════"
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as kf]
            [clojure.math :as math]))

;; ============================================================
;; ASCII CHART ENGINE
;; ============================================================

(defn ascii-chart
  "Рисует ASCII график. values = [double ...].
   Опции: {:width 60 :height 15 :title \"Loss\" :x-label \"epoch\"}"
  [values & [{:keys [width height title x-label y-label]
              :or {width 60 height 12 title "Chart"
                   x-label "x" y-label "y"}}]]
  (when (seq values)
    (let [n (count values)
          vmin (apply min values)
          vmax (apply max values)
          range-v (max 1e-10 (- vmax vmin))
          ;; Resample to fit width
          step (max 1 (quot n width))
          sampled (vec (for [i (range 0 n step)] (nth values i)))
          w (count sampled)
          ;; Build grid
          grid (vec (for [_r (range height)] (vec (repeat w \space))))]
      ;; Title
      (println (format "    ┌─ %s ─%s┐" title (apply str (repeat (max 0 (- width (count title) 4)) "─"))))
      ;; Plot area
      (doseq [r (range height)]
        (let [y-val (+ vmin (* range-v (/ (- (dec height) r) (dec height))))
              row-chars (mapv (fn [col-idx]
                                (let [v (nth sampled col-idx)
                                      row-for-v (int (Math/round (* (- 1.0 (/ (- v vmin) range-v))
                                                                    (dec height))))]
                                  (if (= r row-for-v) \● \space)))
                              (range w))]
          (println (format "    │%7.4f│%s│" y-val (apply str row-chars)))))
      ;; Bottom axis
      (println (format "    └───────┴%s┘" (apply str (repeat w "─"))))
      (println (format "     %s: 0%s%d" x-label
                       (apply str (repeat (max 0 (- w 4)) " ")) (dec n))))))

(defn ascii-bar-chart
  "ASCII bar chart для сравнения значений.
   items = [{:label \"name\" :value 0.5} ...]"
  [items & [{:keys [width title]
             :or {width 40 title "Comparison"}}]]
  (let [max-v (apply max (map :value items))
        max-label (apply max (map #(count (:label %)) items))]
    (println (format "    ── %s ──" title))
    (doseq [{:keys [label value]} items]
      (let [bar-len (int (* width (/ value (max 1e-10 max-v))))
            bar (apply str (repeat bar-len "█"))
            padded (apply str label (repeat (max 0 (- max-label (count label))) " "))]
        (println (format "    %s │%s %.4f" padded bar value))))))

;; ============================================================
;; GRADIENT MONITOR
;; ============================================================

(defn gradient-norms
  "Вычисляет L2 norm градиентов модели.
   Возвращает [{:layer n :norm double :status :ok/:vanishing/:exploding}]."
  [model]
  (let [params (kf/all-params model)]
    (vec (map-indexed
           (fn [idx param]
             (let [grad-val @(:grad param)  ;; atom → deref!
                   n (if grad-val (alength ^doubles grad-val) 0)
                   norm (if (and grad-val (pos? n))
                          (math/sqrt (loop [i 0 acc 0.0]
                                       (if (= i n) acc
                                         (let [gi (aget ^doubles grad-val i)]
                                           (recur (inc i) (+ acc (* gi gi)))))))
                          0.0)
                   status (cond
                            (< norm 1e-7) :vanishing
                            (> norm 100.0) :exploding
                            :else :ok)]
               {:layer idx :norm norm :status status :n-params (:numel param)}))
           params))))

(defn format-gradient-report
  "Форматирует отчёт о градиентах."
  [grad-norms]
  (println "    ┌───────┬──────────┬──────────┬──────────┐")
  (println "    │ Layer │  Params  │   Norm   │  Status  │")
  (println "    ├───────┼──────────┼──────────┼──────────┤")
  (doseq [{:keys [layer norm status n-params]} grad-norms]
    (let [status-str (case status
                       :ok "✅ ok"
                       :vanishing "⚠️ vanish"
                       :exploding "🔥 explode")]
      (println (format "    │  %3d  │  %6d  │ %8.2e │ %-8s │" layer n-params norm status-str))))
  (println "    └───────┴──────────┴──────────┴──────────┘"))

;; ============================================================
;; LR SCHEDULE VISUALIZER
;; ============================================================

(defn cosine-lr-schedule
  "Генерирует cosine annealing schedule."
  [lr-max lr-min total-epochs]
  (vec (for [t (range total-epochs)]
         (+ lr-min (* 0.5 (- lr-max lr-min)
                      (+ 1.0 (math/cos (* Math/PI (/ t total-epochs)))))))))

(defn step-lr-schedule
  "Step decay: lr *= gamma каждые step-size эпох."
  [lr-init gamma step-size total-epochs]
  (vec (for [t (range total-epochs)]
         (* lr-init (math/pow gamma (quot t step-size))))))

(defn warmup-cosine-schedule
  "Warmup + cosine decay."
  [lr-max lr-min warmup-epochs total-epochs]
  (vec (for [t (range total-epochs)]
         (if (< t warmup-epochs)
           (* lr-max (/ (inc t) warmup-epochs))
           (+ lr-min (* 0.5 (- lr-max lr-min)
                        (+ 1.0 (math/cos (* Math/PI
                                            (/ (- t warmup-epochs)
                                               (- total-epochs warmup-epochs)))))))))))

;; ============================================================
;; EPOCH TIMER
;; ============================================================

(defn make-timer
  "Создаёт timer для отслеживания epoch timing."
  []
  (atom {:timings [] :t0 nil}))

(defn timer-start! [timer]
  (swap! timer assoc :t0 (System/nanoTime)))

(defn timer-stop! [timer]
  (let [t0 (:t0 @timer)
        ms (/ (- (System/nanoTime) t0) 1e6)]
    (swap! timer update :timings conj ms)
    ms))

(defn timer-stats [timer]
  (let [ts (:timings @timer)
        n (count ts)]
    (when (pos? n)
      {:count n
       :mean (/ (reduce + ts) n)
       :min (apply min ts)
       :max (apply max ts)
       :last (last ts)
       :total (reduce + ts)})))

;; ============================================================
;; TRAINING DASHBOARD
;; ============================================================

(defn print-dashboard
  "Печатает полный dashboard обучения."
  [epoch loss-history grad-norms lr timer-stats]
  (println "\n    ╔════════════════════════════════════════════╗")
  (println (format "    ║  Training Monitor — Epoch %4d              ║" epoch))
  (println "    ╠════════════════════════════════════════════╣")
  ;; Loss
  (println (format "    ║  Loss:     %.6f" (last loss-history)))
  (when (> (count loss-history) 1)
    (let [prev (nth loss-history (- (count loss-history) 2))
          delta (- (last loss-history) prev)]
      (println (format "    ║  Δloss:    %+.6f (%s)"
                       delta (if (neg? delta) "↓ improving" "↑ worsening")))))
  ;; LR
  (println (format "    ║  LR:       %.6f" lr))
  ;; Timing
  (when timer-stats
    (println (format "    ║  Epoch:    %.1f ms (avg: %.1f ms)" (:last timer-stats) (:mean timer-stats)))
    (println (format "    ║  ETA:      %.0f ms" (* (:mean timer-stats) (- 100 epoch)))))
  ;; Gradients
  (println "    ║  Gradients:")
  (doseq [{:keys [layer norm status]} grad-norms]
    (let [bar-len (min 20 (int (* 20 (min 1.0 (/ (math/log (+ norm 1e-10)) 5.0)))))]
      (println (format "    ║    L%d: %s %.2e %s"
                       layer
                       (apply str (repeat (max 0 bar-len) "▓"))
                       norm
                       (case status :ok "✅" :vanishing "⚠️" :exploding "🔥")))))
  (println "    ╚════════════════════════════════════════════╝"))

;; ============================================================
;; MONITORED TRAINING
;; ============================================================

(defn train-monitored
  "Training loop с полным мониторингом.
   Использует train-step из kan_framework."
  [model data epochs lr & [{:keys [print-every dashboard-every]
                             :or {print-every 10 dashboard-every 50}}]]
  (let [timer (make-timer)
        x (:x data) y (:y data)]
    (loop [m model ep 0 history []
           grad-history [] timing-history []]
      (if (= ep epochs)
        {:model m :loss-history history
         :grad-history grad-history
         :timing (timer-stats timer)}
        (do
          (timer-start! timer)
          (let [[m2 loss] (kf/train-step m x y lr)
                ms (timer-stop! timer)
                grads (gradient-norms m2)
                history2 (conj history loss)
                gh2 (conj grad-history grads)]
            ;; Print progress
            (when (zero? (mod (inc ep) print-every))
              (println (format "    Epoch %3d | Loss: %.6f | %.1f ms | grad-norm: %.2e"
                               (inc ep) loss ms
                               (reduce + (map :norm grads)))))
            ;; Dashboard
            (when (and (pos? dashboard-every)
                       (pos? ep)
                       (zero? (mod (inc ep) dashboard-every)))
              (print-dashboard (inc ep) history2 grads lr (timer-stats timer)))
            (recur m2 (inc ep) history2 gh2
                   (conj timing-history ms))))))))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-training-monitor []
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  Training Monitor                    ║")
  (println "  ║  ASCII plots · grads · timing        ║")
  (println "  ╚══════════════════════════════════════╝")

  ;; Part 1: ASCII loss curve
  (println "\n  Part 1: ASCII loss curve")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        model (kf/make-model [1 1] 4)
        result (train-monitored model
                 {:x (t2/tensor xs [n 1]) :y (t2/tensor ys [n])}
                 60 0.01
                 {:print-every 15 :dashboard-every 0})]
    (ascii-chart (:loss-history result)
                 {:title "Loss Curve" :height 10 :width 50
                  :x-label "epoch" :y-label "loss"})
    (println (format "    Final loss: %.6f" (last (:loss-history result)))))

  ;; Part 2: LR schedule comparison
  (println "\n  Part 2: LR schedule comparison")
  (let [epochs 100
        cosine (cosine-lr-schedule 0.01 0.0001 epochs)
        step-decay (step-lr-schedule 0.01 0.5 30 epochs)
        warmup (warmup-cosine-schedule 0.01 0.0001 10 epochs)]
    (println "\n    Cosine Annealing:")
    (ascii-chart cosine {:title "Cosine LR" :height 8 :width 50
                         :x-label "epoch" :y-label "lr"})
    (println "\n    Step Decay (γ=0.5 every 30):")
    (ascii-chart step-decay {:title "Step Decay LR" :height 8 :width 50
                              :x-label "epoch" :y-label "lr"})
    (println "\n    Warmup + Cosine:")
    (ascii-chart warmup {:title "Warmup+Cosine LR" :height 8 :width 50
                          :x-label "epoch" :y-label "lr"}))

  ;; Part 3: Gradient monitoring
  (println "\n  Part 3: Gradient norms per layer")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv #(+ (math/sin %) (* % %)) xs)
        model (kf/make-model [1 4 1] 4)
        ;; Train a few steps to get gradients
        [m2 _] (kf/train-step model
                  (t2/tensor xs [n 1])
                  (t2/tensor ys [n])
                  0.01)
        grads (gradient-norms m2)]
    (format-gradient-report grads))

  ;; Part 4: Epoch timing
  (println "\n  Part 4: Epoch timing analysis")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        model (kf/make-model [1 1] 4)
        result (train-monitored model
                 {:x (t2/tensor xs [n 1]) :y (t2/tensor ys [n])}
                 30 0.01
                 {:print-every 100 :dashboard-every 0})
        stats (:timing result)]
    (println (format "    Epochs:   %d" (:count stats)))
    (println (format "    Mean:     %.2f ms/epoch" (:mean stats)))
    (println (format "    Min:      %.2f ms" (:min stats)))
    (println (format "    Max:      %.2f ms" (:max stats)))
    (println (format "    Total:    %.0f ms" (:total stats))))

  ;; Part 5: Full dashboard
  (println "\n  Part 5: Training with dashboard (every 25 epochs)")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        model (kf/make-model [1 1] 4)
        result (train-monitored model
                 {:x (t2/tensor xs [n 1]) :y (t2/tensor ys [n])}
                 50 0.01
                 {:print-every 200 :dashboard-every 25})]
    (println (format "\n    Training complete: %.6f → %.6f"
                     (first (:loss-history result))
                     (last (:loss-history result)))))

  ;; Part 6: Bar chart comparison
  (println "\n  Part 6: φ-type performance comparison")
  (ascii-bar-chart [{:label "PolyPhi"     :value 0.254}
                    {:label "BSplinePhi"  :value 0.218}
                    {:label "BSpline+Grid" :value 0.206}
                    {:label "RationalPhi" :value 0.088}]
                   {:title "Final Loss by φ-type" :width 30})

  ;; Part 7: Summary
  (println "\n  Part 7: Monitor capabilities")
  (println "    ✅ ASCII loss curves (terminal-friendly)")
  (println "    ✅ Gradient norms per layer (vanish/explode detection)")
  (println "    ✅ LR schedule visualization (cosine, step, warmup)")
  (println "    ✅ Epoch timing (mean, min, max, ETA)")
  (println "    ✅ Training dashboard (all-in-one report)")
  (println "    ✅ Bar charts (comparison visualization)"))
