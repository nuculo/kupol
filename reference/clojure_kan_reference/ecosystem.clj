(ns kan-kat.ecosystem
  "Агентный функциональный мир — экосистема φ-агентов.
   
   ═══════════════════════════════════════════════════
   ЭКОСИСТЕМА (расширение agent-kan)
   ═══════════════════════════════════════════════════
   
   В agent-kan агенты адаптируются, но не рождаются/умирают.
   Здесь — полная экосистема:
   
   • РОЖДЕНИЕ: сильные агенты производят потомков (мутация)
   • СМЕРТЬ: слабые агенты удаляются
   • КОНКУРЕНЦИЯ: ресурсы (memory) ограничены → carrying capacity
   • ПАРАЛЛЕЛЬНОСТЬ: pmap для одновременной эволюции
   • РАЗНООБРАЗИЕ: мутация типа при рождении
   
   Аналогия: трава→зайцы→волки. Баланс ВОЗНИКАЕТ.
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.phi-protocol :as phi]))

;; ============================================================
;; КОНФИГУРАЦИЯ ЭКОСИСТЕМЫ
;; ============================================================

(def eco-config
  {:max-population    30       ; carrying capacity
   :death-threshold   2.0      ; loss > этого → кандидат на смерть
   :reproduce-threshold 0.1    ; loss < этого → может размножиться
   :max-age           50       ; максимальный возраст
   :mutation-rate     0.3      ; вероятность мутации при рождении
   :type-mutation-rate 0.15    ; вероятность смены типа при рождении
   :param-sigma       0.2     ; σ шума при мутации
   :lr                0.03})   ; learning rate для gradient step

(def available-types
  [[:bspline  {:order 3 :grid-size 5 :grid-range [-3.0 3.0]}]
   [:poly     {:degree 3}]
   [:poly     {:degree 5}]
   [:rational {:p-deg 3 :q-deg 2}]])

;; ============================================================
;; АГЕНТ (организм)
;; ============================================================

(defn make-organism
  "Создаёт организм — φ-агент с генеалогией."
  [& [{:keys [phi-type opts parent-id generation]
       :or   {phi-type :bspline
              opts {:order 3 :grid-size 5 :grid-range [-3.0 3.0]}
              parent-id nil
              generation 0}}]]
  {:id          (gensym "org-")
   :phi         (phi/make-phi phi-type opts)
   :type        phi-type
   :opts        opts
   :age         0
   :generation  generation
   :parent-id   parent-id
   :loss        1.0        ; начальный loss
   :alive       true})

;; ============================================================
;; FITNESS / LOSS
;; ============================================================

(defn organism-loss
  "Вычисляет loss организма на данных."
  [org data]
  (let [f (fn [x] (phi/phi-forward (:phi org) x))
        n (count data)]
    (/ (reduce + 0.0
         (map (fn [[x y]]
                (let [err (- (f x) y)]
                  (* err err)))
              data))
       (max 1 n))))

;; ============================================================
;; АДАПТАЦИЯ (gradient step)
;; ============================================================

(defn adapt
  "Один gradient step для организма."
  [org data lr]
  (let [params (:params org (phi/phi-params (:phi org)))
        eps    1e-4
        grads  (mapv (fn [i]
                       (let [p+ (assoc params i (+ (nth params i) eps))
                             p- (assoc params i (- (nth params i) eps))
                             o+ (assoc org :phi (phi/phi-update (:phi org) p+))
                             o- (assoc org :phi (phi/phi-update (:phi org) p-))
                             l+ (organism-loss o+ data)
                             l- (organism-loss o- data)]
                         (/ (- l+ l-) (* 2.0 eps))))
                     (range (count params)))
        new-params (mapv (fn [p g] (- p (* lr g))) params grads)]
    (assoc org :phi (phi/phi-update (:phi org) new-params))))

;; ============================================================
;; РОЖДЕНИЕ
;; ============================================================

(defn reproduce
  "Организм производит потомка (с мутацией)."
  [parent]
  (let [;; Мутация типа?
        type-mutate? (< (rand) (:type-mutation-rate eco-config))
        [child-type child-opts]
        (if type-mutate?
          (rand-nth available-types)
          [(:type parent) (:opts parent)])
        ;; Создаём потомка
        child (make-organism {:phi-type   child-type
                              :opts       child-opts
                              :parent-id  (:id parent)
                              :generation (inc (:generation parent))})]
    ;; Если тип не менялся — наследуем параметры с шумом
    (if (and (not type-mutate?)
             (= (count (phi/phi-params (:phi child)))
                (count (phi/phi-params (:phi parent)))))
      (let [parent-params (phi/phi-params (:phi parent))
            sigma (:param-sigma eco-config)
            child-params (mapv (fn [p] (+ p (* sigma (- (rand) 0.5) 2.0)))
                               parent-params)]
        (assoc child :phi (phi/phi-update (:phi child) child-params)))
      child)))

;; ============================================================
;; СМЕРТЬ
;; ============================================================

(defn alive?
  "Проверяет, жив ли организм."
  [org]
  (and (:alive org)
       (< (:age org) (:max-age eco-config))
       (< (:loss org) (:death-threshold eco-config))))

;; ============================================================
;; ШАГ ЭКОСИСТЕМЫ
;; ============================================================

(defn ecosystem-step
  "Один шаг экосистемы: адаптация → оценка → смерть → рождение."
  [population data]
  (let [lr (:lr eco-config)
        ;; 1. Адаптация + оценка (параллельно!)
        adapted (pmap (fn [org]
                        (let [adapted-org (adapt org data lr)
                              loss (organism-loss adapted-org data)]
                          (-> adapted-org
                              (assoc :loss loss)
                              (update :age inc))))
                      population)
        ;; 2. Смерть: убираем слабых (но всегда сохраняем лучшего)
        survivors (filterv alive? adapted)
        survivors (if (empty? survivors)
                   [(apply min-key :loss (vec adapted))]
                   survivors)
        ;; 3. Рождение: сильные размножаются
        parents (filter #(< (:loss %) (:reproduce-threshold eco-config))
                        survivors)
        children (mapv reproduce parents)
        ;; 4. Новая популяция (с ограничением carrying capacity)
        new-pop (vec (take (:max-population eco-config)
                           (concat survivors children)))]
    new-pop))

;; ============================================================
;; ЭВОЛЮЦИЯ ЭКОСИСТЕМЫ
;; ============================================================

(defn run-ecosystem
  "Запускает экосистему на n шагов.
   
   target-fn  — целевая функция
   domain     — [lo hi]
   n-points   — обучающие точки
   init-pop   — начальная популяция
   steps      — шагов эволюции"
  [target-fn domain n-points init-pop steps]
  (let [data (mapv (fn [_]
                     (let [x (+ (first domain)
                                (* (rand) (- (second domain) (first domain))))]
                       [x (target-fn x)]))
                   (range n-points))]
    (println "  ╔══════════════════════════════════════╗")
    (println "  ║  Ecosystem: φ organisms evolving     ║")
    (println "  ╚══════════════════════════════════════╝")
    (println (format "  Target: f(x) | Data: %d pts | Max pop: %d"
                     n-points (:max-population eco-config)))
    (println "  ────────────────────────────────────────")
    (println "  Step | Pop | Best Loss | Types                | Gens")
    (println "  ───__|_____|___________|______________________|_____")
    (loop [pop init-pop step 1]
      (if (> step steps)
        (let [best (apply min-key :loss pop)]
          (println "  ────────────────────────────────────────")
          (println (format "  Winner: %s (gen %d) | Loss: %.6f | Params: %d"
                           (name (:type best)) (:generation best)
                           (:loss best) (count (phi/phi-params (:phi best)))))
          best)
        (let [new-pop (ecosystem-step pop data)
              best    (apply min-key :loss new-pop)
              types   (frequencies (map :type new-pop))
              max-gen (apply max (map :generation new-pop))]
          (when (zero? (mod step 5))
            (println (format "  %4d | %3d | %9.6f | %-20s | %d"
                             step (count new-pop) (:loss best)
                             (pr-str types) max-gen)))
          (recur new-pop (inc step)))))))

;; ============================================================
;; УДОБНАЯ ОБЁРТКА
;; ============================================================

(defn evolve-ecosystem
  "Запуск экосистемы: создаёт начальную популяцию и эволюционирует."
  [target-fn domain n-points pop-size steps]
  (let [init-pop (mapv (fn [_]
                         (let [[t o] (rand-nth available-types)]
                           (make-organism {:phi-type t :opts o})))
                       (range pop-size))]
    (run-ecosystem target-fn domain n-points init-pop steps)))
