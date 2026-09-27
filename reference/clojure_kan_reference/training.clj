(ns kan-kat.training
  "Модуль обучения KAN v2 — полный набор инструментов.
   
   ═══════════════════════════════════════════════════════════
   АРХИТЕКТУРА ОБУЧЕНИЯ
   ═══════════════════════════════════════════════════════════
   
   1. LOSS-ФУНКЦИИ
      • MSE (Mean Squared Error) — для регрессии
      • MSE-batch — пакетная версия для мини-батчей
      Обе версии: Dual-aware (для AD) и Double (для мониторинга)
   
   2. ВЫЧИСЛЕНИЕ ГРАДИЕНТА
      • Forward-mode AD (дуальные числа) — точный градиент
      • Один forward pass на параметр → O(n_params) проходов
      • Сетки (grids) НЕ дифференцируются (обновляются отдельно)
   
   3. ОПТИМИЗАТОРЫ
      • SGD — базовый стохастический градиентный спуск
      • SGD с momentum — экспоненциальное сглаживание градиентов
        v_t = μ·v_{t-1} + g_t, θ_t = θ_{t-1} - lr·v_t
      • Gradient clipping — обрезка по max-norm для стабильности
   
   4. ОБУЧАЮЩИЙ ЦИКЛ (train-kan)
      • Мини-батчи: shuffle + разбиение на батчи заданного размера
      • Адаптивная сетка: обновление каждые N эпох
      • Learning rate decay: lr(t) = lr₀ / (1 + decay·epoch)
      • Логирование: epoch, loss, lr на каждой эпохе
   
   5. ИНИЦИАЛИЗАЦИЯ
      • Xavier-like: wb ∈ [-0.1, 0.1], ws ∈ [0.5, 1.5]
      • Сплайн-коэффициенты: ∈ [-0.01, 0.01] (малый начальный вклад)
      • LayerNorm: γ=1.0, β=0.0 (identity при старте)
      • Per-input grids: каждый вход имеет свою сетку knots
   
   ═══════════════════════════════════════════════════════════"
  (:require [kan-kat.ad :as ad]
            [kan-kat.math :as m]
            [kan-kat.spline :as spl]
            [kan-kat.kan-layer :as kan]))

;; ============================================================
;; LOSS-ФУНКЦИИ
;; ============================================================

(defn mse-loss
  "MSE loss (Dual-aware) — для использования внутри AD.
   
   MSE = (1/n) Σᵢ (predᵢ - targetᵢ)²
   
   Принимает и возвращает Dual: позволяет forward-mode AD
   автоматически вычислить ∂MSE/∂θ через цепное правило."
  [predicted target]
  (let [diffs (mapv ad/d-sub predicted target)
        sqrs  (mapv #(ad/d-mul % %) diffs)]
    (ad/d-div (ad/d-sum sqrs)
              (ad/const-d (double (count diffs))))))

(defn mse-loss-double
  "MSE loss (plain Double) — для мониторинга обучения.
   Быстрее, чем Dual-версия, т.к. не тащит tangent."
  [predicted target]
  (let [diffs (m/vec-sub predicted target)
        sqrs  (mapv #(* % %) diffs)]
    (/ (reduce + 0.0 sqrs) (count diffs))))

(defn mse-batch-loss-double
  "Средний MSE по батчу: mean_batch(MSE(predᵢ, targetᵢ))."
  [predictions targets]
  (let [losses (mapv mse-loss-double predictions targets)]
    (/ (reduce + 0.0 losses) (max 1 (count losses)))))

;; ============================================================
;; ГРАДИЕНТ (forward-mode AD)
;; ============================================================

(defn kan-gradient
  "Вычисление градиента MSE loss по параметрам KAN.
   
   Алгоритм:
   1. Зафиксировать input и target как Dual-константы
   2. Для каждого параметра θᵢ: установить tangent=1, остальные=0
   3. Один forward pass → tangent на выходе = ∂MSE/∂θᵢ
   4. Повторить для всех n_params параметров
   
   Сетки (grids) не дифференцируются — они обновляются
   отдельно через update-grids-from-batch.
   
   Параметры: [wb | ws | spline-coeffs | ln-gamma | ln-beta]"
  [layer input target]
  (let [in-f    (:in-features layer)
        out-f   (:out-features layer)
        k       (:spline-order layer)
        n-spl   (kan/num-spl-coeffs layer)
        ;; Per-input grids как Dual-константы (не обучаемые)
        grids-d (mapv (fn [knots] (mapv ad/const-d knots)) (:grids layer))]
    (ad/compute-gradient
      (fn [dual-params]
        (let [input-d  (mapv ad/const-d input)
              target-d (mapv ad/const-d target)
              output   (kan/forward-generic in-f out-f k n-spl
                                            grids-d dual-params input-d)]
          (mse-loss output target-d)))
      (kan/extract-params layer))))

;; ============================================================
;; ОПТИМИЗАТОРЫ
;; ============================================================

(defn clip-gradients
  "Обрезка градиентов по max-norm для стабильности обучения.
   
   Если ||g||∞ > max-norm, масштабируем все градиенты:
   g' = g · min(1, max-norm / ||g||∞)
   
   Предотвращает взрывающиеся градиенты (exploding gradients),
   особенно в первые эпохи или при адаптации сетки."
  [grads max-norm]
  (let [max-g (apply max (map abs grads))]
    (if (> max-g max-norm)
      (let [scale (/ max-norm max-g)]
        (mapv #(* scale %) grads))
      grads)))

(defn sgd-update
  "SGD: θ_new = θ_old - lr · gradient.
   Простейший оптимизатор, baseline для сравнения."
  [lr params grads]
  (mapv (fn [p g] (- p (* lr g))) params grads))

(defn sgd-momentum-update
  "SGD с momentum (Polyak heavy ball):
   
   v_t = μ · v_{t-1} + g_t
   θ_t = θ_{t-1} - lr · v_t
   
   Momentum μ ∈ [0, 1) сглаживает осцилляции градиента.
   Типичное значение: μ = 0.9.
   
   Возвращает [new-params, new-velocity]."
  [lr momentum params grads velocity]
  (let [v' (mapv (fn [vi gi] (+ (* momentum vi) gi))
                 velocity grads)
        params' (mapv (fn [p v] (- p (* lr v))) params v')]
    [params' v']))

;; ============================================================
;; МИНИ-БАТЧИ
;; ============================================================

(defn make-mini-batches
  "Shuffle + разбиение датасета на мини-батчи.
   
   Mini-batch SGD: обновляем параметры после каждого батча,
   а не после всего датасета (batch GD) или каждого примера (SGD).
   Баланс между скоростью сходимости и шумом градиента."
  [dataset batch-size]
  (let [shuffled (shuffle dataset)]
    (mapv vec (partition-all batch-size shuffled))))

;; ============================================================
;; LEARNING RATE DECAY
;; ============================================================

(defn decay-lr
  "Inverse-time decay: lr(t) = lr₀ / (1 + decay · epoch).
   
   Постепенно уменьшает шаг обучения:
   - Большой lr в начале → быстрое приближение к минимуму
   - Малый lr в конце → точная настройка без осцилляций
   
   decay=0.0 → нет decay (постоянный lr)"
  [lr0 decay epoch]
  (/ lr0 (+ 1.0 (* decay (double epoch)))))

;; ============================================================
;; ОБУЧАЮЩИЙ ЦИКЛ v2
;; ============================================================

(defn train-kan
  "Обучение KAN-слоя — полный цикл.
   
   ═══════════════════════════════════════
   КОНФИГУРАЦИЯ
   ═══════════════════════════════════════
   
   layer    — начальный KAN-слой (из init-random-kan)
   dataset  — [[input target] ...], каждый input/target — вектор
   lr       — начальный learning rate (рекомендация: 0.01-0.05)
   epochs   — количество эпох обучения
   
   Опциональные параметры (передаются через opts map):
     :grid-update-every  — обновлять сетку каждые N эпох (nil = никогда)
     :grid-eps           — blend: 1.0=uniform, 0.0=quantile (default 0.5)
     :momentum           — коэф. momentum (0.0=чистый SGD, 0.9=типичный)
     :lr-decay           — inverse-time decay коэф. (0.0=нет decay)
     :batch-size         — размер мини-батча (nil=весь датасет)
     :max-grad-norm      — max-norm для clipping (nil=нет clipping)
   
   ═══════════════════════════════════════
   АЛГОРИТМ (per epoch)
   ═══════════════════════════════════════
   
   1. Вычислить текущий lr: lr_t = lr / (1 + decay · epoch)
   2. Если epoch % grid-update-every == 0:
      → update-grids-from-batch (адаптивная сетка)
   3. Shuffle + разбить на мини-батчи
   4. Для каждого мини-батча:
      a. Для каждого (input, target):
         - kan-gradient → ∂loss/∂θ (forward-mode AD)
         - Усреднить градиенты по батчу
      b. Clip gradients (если задан max-grad-norm)
      c. SGD+momentum update: θ -= lr_t · v_t
   5. Вычислить и вывести средний loss
   
   Возвращает обученный KAN-слой."
  [layer dataset lr epochs & [opts]]
  (let [{:keys [grid-update-every grid-eps momentum lr-decay
                batch-size max-grad-norm]
         :or   {grid-eps 0.5 momentum 0.0 lr-decay 0.0
                batch-size nil max-grad-norm nil}} opts
        n-params (kan/num-params layer)
        init-velocity (vec (repeat n-params 0.0))]
    (loop [current-layer layer
           velocity init-velocity
           epoch 1]
      (if (> epoch epochs)
        current-layer
        (let [;; 1. Learning rate decay
              current-lr (decay-lr lr lr-decay (dec epoch))
              
              ;; 2. Адаптивная сетка
              current-layer
              (if (and grid-update-every
                       (pos? grid-update-every)
                       (zero? (mod (dec epoch) grid-update-every)))
                (let [batch-x (mapv first dataset)]
                  (kan/update-grids-from-batch current-layer batch-x
                                              (or grid-eps 0.5)))
                current-layer)
              
              ;; 3. Мини-батчи
              batches (if batch-size
                        (make-mini-batches dataset batch-size)
                        [dataset])  ;; full-batch если не задан
              
              ;; 4. Обработка батчей
              [final-layer final-velocity total-loss sample-count]
              (reduce
                (fn [[layer vel acc-loss cnt] batch]
                  ;; Средний градиент по батчу
                  (let [grads-list (mapv (fn [[input target]]
                                          (kan-gradient layer input target))
                                        batch)
                        n-batch   (count grads-list)
                        avg-grads (if (= n-batch 1)
                                    (first grads-list)
                                    (mapv (fn [i]
                                            (/ (reduce + (map #(nth % i) grads-list))
                                               n-batch))
                                          (range n-params)))
                        ;; Clip
                        clipped   (if max-grad-norm
                                    (clip-gradients avg-grads max-grad-norm)
                                    avg-grads)
                        ;; Update
                        params    (kan/extract-params layer)
                        [params' vel']
                        (if (pos? momentum)
                          (sgd-momentum-update current-lr momentum params clipped vel)
                          [(sgd-update current-lr params clipped) vel])
                        layer'    (kan/inject-params layer params')
                        ;; Loss для мониторинга
                        batch-loss (reduce + (map (fn [[input target]]
                                                    (mse-loss-double
                                                      (kan/forward-layer layer input)
                                                      target))
                                                  batch))]
                    [layer' vel' (+ acc-loss batch-loss) (+ cnt (count batch))]))
                [current-layer velocity 0.0 0]
                batches)
              
              avg-loss (/ total-loss (max 1 sample-count))]
          
          (println (format "  Epoch %d/%d | Loss: %.6f | lr: %.5f"
                           epoch epochs avg-loss current-lr))
          (recur final-layer final-velocity (inc epoch)))))))

;; ============================================================
;; ИНИЦИАЛИЗАЦИЯ KAN v2
;; ============================================================

(defn rand-range
  "Случайное число в диапазоне [lo, hi]."
  [lo hi]
  (+ lo (* (rand) (- hi lo))))

(defn init-random-kan
  "Инициализация KAN v2 слоя со случайными весами.
   
   ═══════════════════════════════════════
   ПАРАМЕТРЫ ИНИЦИАЛИЗАЦИИ
   ═══════════════════════════════════════
   
   in-f    — число входных признаков
   out-f   — число выходных признаков
   k       — порядок B-сплайна (3 = кубический, как в оригинале)
   g       — число интервалов сетки (grid-size)
   bounds  — [min max] диапазон сетки
   
   ═══════════════════════════════════════
   СТРАТЕГИЯ ИНИЦИАЛИЗАЦИИ
   ═══════════════════════════════════════
   
   • wb (base weights):      U[-0.1, 0.1]  — Xavier-like
   • ws (spline scales):     U[0.5, 1.5]   — сплайн активен с начала
   • coeffs (B-spline):      U[-0.01, 0.01] — малый начальный вклад
   • ln-gamma (LayerNorm):   1.0 — identity при старте
   • ln-beta (LayerNorm):    0.0 — identity при старте
   • grids:                  per-input, uniform knots
   
   Число параметров = out*in*(2 + n_spl) + 2*in
   где n_spl = g + k (число B-spline базисов на ребро)"
  [in-f out-f k g bounds]
  (let [;; Per-input dimension grids
        grids (mapv (fn [_] (spl/init-knots k g bounds))
                    (range in-f))
        n-spl (+ g k)
        bw (mapv (fn [_]
                   (mapv (fn [_] (rand-range -0.1 0.1))
                         (range in-f)))
                 (range out-f))
        ss (mapv (fn [_]
                   (mapv (fn [_] (rand-range 0.5 1.5))
                         (range in-f)))
                 (range out-f))
        sw (mapv (fn [_]
                   (mapv (fn [_]
                           (mapv (fn [_] (rand-range -0.01 0.01))
                                 (range n-spl)))
                         (range in-f)))
                 (range out-f))]
    {:in-features    in-f
     :out-features   out-f
     :spline-order   k
     :grid-size      g
     :grid-bounds    bounds
     :grids          grids
     :base-weights   bw
     :spline-scales  ss
     :spline-weights sw
     :ln-gamma       (vec (repeat in-f 1.0))
     :ln-beta        (vec (repeat in-f 0.0))}))
