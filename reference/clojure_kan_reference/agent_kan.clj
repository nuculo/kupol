(ns kan-kat.agent-kan
  "Агентная KAN: каждое ребро φ_{j,i} — автономный агент.
   
   ═══════════════════════════════════════════════════
   АРХИТЕКТУРА
   ═══════════════════════════════════════════════════
   
   Обычный KAN = «армия» (backprop командует всем).
   Агентный KAN = «муравейник» (порядок из простых правил).
   
   Каждый агент-ребро:
   - Имеет свою φ (любого типа через PhiFunction protocol)
   - Хранит историю loss (память)
   - Выбирает стратегию: :explore или :exploit
   - Может МЕНЯТЬ тип φ если стагнирует
   - Общается с соседями (output→input координация)
   
   Всё на immutable maps + pmap для параллелизма.
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.phi-protocol :as phi]))

;; ============================================================
;; КОНФИГУРАЦИЯ АГЕНТОВ
;; ============================================================

(def agent-config
  {:stagnation-window   5      ; сколько шагов без улучшения → мутация
   :exploit-threshold   0.01   ; если loss < этого → exploit mode
   :explore-lr          0.05   ; lr в режиме explore
   :exploit-lr          0.005  ; lr в режиме exploit (fine-tune)
   :mutation-sigma      0.3    ; сила мутации параметров
   :history-size        10})   ; размер памяти

(def available-types
  "Типы φ для мутации."
  [[:bspline  {:order 3 :grid-size 5 :grid-range [-3.0 3.0]}]
   [:bspline  {:order 3 :grid-size 8 :grid-range [-3.0 3.0]}]
   [:poly     {:degree 3}]
   [:poly     {:degree 5}]
   [:rational {:p-deg 3 :q-deg 2}]])

;; ============================================================
;; СОЗДАНИЕ АГЕНТА
;; ============================================================

(defn make-agent
  "Создаёт агент-ребро с начальной φ."
  [j i & [{:keys [phi-type opts]
           :or   {phi-type :bspline
                  opts {:order 3 :grid-size 5 :grid-range [-3.0 3.0]}}}]]
  {:j        j
   :i        i
   :phi      (phi/make-phi phi-type opts)
   :type     phi-type
   :opts     opts
   :history  []          ; история loss values
   :strategy :explore    ; :explore | :exploit
   :age      0           ; сколько шагов прожил
   :mutations 0})        ; сколько раз мутировал тип

;; ============================================================
;; ЛОКАЛЬНЫЙ LOSS АГЕНТА
;; ============================================================

(defn agent-local-loss
  "Вычисляет локальный loss агента: MSE между φ(x) и target.
   data = [[x target] ...]"
  [agent data]
  (let [f (fn [x] (phi/phi-forward (:phi agent) x))
        n (count data)]
    (/ (reduce + 0.0
         (map (fn [[x target]]
                (let [err (- (f x) target)]
                  (* err err)))
              data))
       (max 1 n))))

;; ============================================================
;; ДЕТЕКЦИЯ СТАГНАЦИИ
;; ============================================================

(defn stagnating?
  "Агент стагнирует если loss не улучшился за window шагов."
  [history window]
  (when (>= (count history) window)
    (let [recent (take-last window history)
          best   (apply min recent)
          worst  (apply max recent)
          improvement (- worst best)]
      ;; Если разброс < 1% от текущего — стагнация
      (< improvement (* 0.01 (max 0.001 best))))))

;; ============================================================
;; СТРАТЕГИЯ АГЕНТА
;; ============================================================

(defn choose-strategy
  "Агент выбирает стратегию на основе своего состояния."
  [agent]
  (let [history (:history agent)
        current-loss (if (empty? history) 1.0 (last history))]
    (cond
      ;; Loss очень маленький → exploit (fine-tune)
      (< current-loss (:exploit-threshold agent-config))
      :exploit
      
      ;; Стагнация → explore (будет мутация)
      (stagnating? history (:stagnation-window agent-config))
      :explore
      
      ;; Иначе — продолжаем текущую стратегию
      :else
      (:strategy agent))))

;; ============================================================
;; МУТАЦИЯ ТИПА (структурная)
;; ============================================================

(defn mutate-type!
  "Полная смена типа φ — «перерождение» агента."
  [agent]
  (let [[new-type new-opts] (rand-nth available-types)]
    (-> agent
        (assoc :phi (phi/make-phi new-type new-opts))
        (assoc :type new-type)
        (assoc :opts new-opts)
        (update :mutations inc)
        (assoc :history [])    ; сброс памяти
        (assoc :strategy :explore))))

;; ============================================================
;; ГРАДИЕНТНЫЙ ШАГ (численный, но локальный)
;; ============================================================

(defn gradient-step
  "Локальный градиентный шаг для агента."
  [agent data lr]
  (let [params (phi/phi-params (:phi agent))
        eps    1e-4
        loss0  (agent-local-loss agent data)
        ;; Числовые градиенты по параметрам
        grads  (mapv (fn [idx]
                       (let [p+ (assoc params idx (+ (nth params idx) eps))
                             p- (assoc params idx (- (nth params idx) eps))
                             a+ (assoc agent :phi (phi/phi-update (:phi agent) p+))
                             a- (assoc agent :phi (phi/phi-update (:phi agent) p-))
                             l+ (agent-local-loss a+ data)
                             l- (agent-local-loss a- data)]
                         (/ (- l+ l-) (* 2.0 eps))))
                     (range (count params)))
        ;; SGD update
        new-params (mapv (fn [p g] (- p (* lr g))) params grads)
        new-phi (phi/phi-update (:phi agent) new-params)]
    (assoc agent :phi new-phi)))

;; ============================================================
;; ОСНОВНОЙ ШАГ АГЕНТА
;; ============================================================

(defn agent-step
  "Один шаг автономного агента.
   
   Правила:
   1. Вычислить локальный loss
   2. Обновить историю
   3. Выбрать стратегию
   4. Действовать:
      - :explore + stagnation → мутировать тип!
      - :explore → gradient step с большим lr
      - :exploit → gradient step с маленьким lr"
  [agent data]
  (let [loss     (agent-local-loss agent data)
        history  (take-last (:history-size agent-config)
                            (conj (:history agent) loss))
        strategy (choose-strategy (assoc agent :history history))
        stagnant (stagnating? history (:stagnation-window agent-config))]
    (cond
      ;; Стагнация → мутация типа!
      (and (= strategy :explore) stagnant)
      (-> agent
          mutate-type!
          (assoc :history [loss]))
      
      ;; Explore → большой lr
      (= strategy :explore)
      (-> (gradient-step agent data (:explore-lr agent-config))
          (assoc :history history)
          (assoc :strategy :explore)
          (update :age inc))
      
      ;; Exploit → маленький lr (fine-tune)
      :else
      (-> (gradient-step agent data (:exploit-lr agent-config))
          (assoc :history history)
          (assoc :strategy :exploit)
          (update :age inc)))))

;; ============================================================
;; АГЕНТНЫЙ KAN-СЛОЙ
;; ============================================================

(defn make-agent-layer
  "Создаёт агентный KAN-слой: n-in × n-out агентов.
   Каждый агент — автономное ребро."
  [n-in n-out]
  {:n-in   n-in
   :n-out  n-out
   :agents (vec (for [j (range n-out)]
                  (vec (for [i (range n-in)]
                         (make-agent j i)))))})

(defn agent-layer-forward
  "Forward через агентный слой: y_j = Σ_i φ_{j,i}(x_i)."
  [layer input]
  (mapv (fn [row-agents]
          (reduce + 0.0
            (map (fn [agent xi]
                   (phi/phi-forward (:phi agent) xi))
                 row-agents input)))
        (:agents layer)))

;; ============================================================
;; ОБУЧЕНИЕ АГЕНТНОГО СЛОЯ
;; ============================================================

(defn train-agent-layer
  "Обучение: каждый агент автономно оптимизируется.
   
   target-fn  — целевая функция f(x) → y
   input-data — [[x1 x2 ...] ...] входные векторы
   epochs     — число эпох
   
   Возвращает обученный слой."
  [layer target-fn input-data epochs]
  (loop [l layer epoch 1]
    (if (> epoch epochs)
      l
      (let [;; Для каждого агента: его локальные данные
            ;; (input_i → target contribution)
            ;; Упрощение: если output=1, target = f(x)
            new-agents
            (mapv (fn [row-agents j]
                    (mapv (fn [agent i]
                            ;; data для агента: [x_i, partial_target]
                            (let [data (mapv (fn [input]
                                              (let [xi     (nth input i)
                                                    y-full (target-fn input)
                                                    ;; Каждый агент отвечает за свою
                                                    ;; долю output (≈ y / n-in)
                                                    target-share (/ y-full (:n-in l))]
                                                [xi target-share]))
                                            input-data)]
                              (agent-step agent data)))
                          row-agents (range)))
                  (:agents l) (range))
            
            new-layer (assoc l :agents new-agents)
            
            ;; Мониторинг
            sample-x (first input-data)
            pred     (first (agent-layer-forward new-layer sample-x))
            actual   (target-fn sample-x)
            loss     (let [e (- pred actual)] (* e e))]
        (when (zero? (mod epoch 5))
          (let [types (frequencies
                        (for [row (:agents new-layer) a row] (:type a)))
                strats (frequencies
                         (for [row (:agents new-layer) a row] (:strategy a)))
                muts  (reduce + (for [row (:agents new-layer) a row] (:mutations a)))]
            (println (format "  Epoch %3d | Loss: %.6f | Types: %s | Strategies: %s | Mutations: %d"
                             epoch loss (pr-str types) (pr-str strats) muts))))
        (recur new-layer (inc epoch))))))
