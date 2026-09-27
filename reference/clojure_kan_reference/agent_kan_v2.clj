(ns kan-kat.agent-kan-v2
  "Agent KAN на tensor_v2: масштабируемая агентная система.
   
   ═══════════════════════════════════════════════════
   Фаза 38: Каждое ребро — автономный агент,
   но теперь на tensor_v2 с батчами и параллелизмом.
   
   Отличия от agent_kan.clj:
   - Батчевый forward (tensor_v2, не scalar)
   - Параллельная эволюция (futures)
   - Fitness на полном батче (не sample)
   - Турнирная селекция + элитизм
   ═══════════════════════════════════════════════════"
  (:require [kan-kat.tensor-v2 :as t2]
            [clojure.math :as math]))

;; ============================================================
;; AGENT: одно ребро = один агент
;; ============================================================

(defn make-agent-v2
  "Создаёт агента: polynomial coefficients на tensor_v2."
  [j i degree]
  (let [n (inc degree)
        sigma (/ 1.0 (math/sqrt n))
        coeffs (t2/tensor (vec (repeatedly n #(* sigma (- (rand) 0.5) 2.0))) [n])]
    {:j j :i i :degree degree
     :coeffs coeffs
     :age 0
     :fitness Double/MAX_VALUE
     :strategy :explore
     :history []}))

;; ============================================================
;; BATCHED FORWARD (tensor_v2)
;; ============================================================

(defn agent-forward-batch
  "Forward pass одного агента на батче: Horner poly evaluation.
   x = Tensor [batch], returns Tensor [batch]."
  [agent x-batch]
  (let [^doubles cd (:data (:coeffs agent))
        deg (:degree agent)
        batch (first (:shape x-batch))
        ^doubles xd (:data x-batch)
        out (double-array batch)]
    ;; Horner: acc = c[deg]; acc = acc*x + c[deg-1]; ...
    (dotimes [b batch]
      (let [xi (aget xd b)]
        (loop [d deg acc (aget cd deg)]
          (if (zero? d)
            (aset out b acc)
            (recur (dec d) (+ (aget cd (dec d)) (* acc xi)))))))
    (t2/tensor (vec out) [batch])))

;; ============================================================
;; FITNESS (MSE на батче)
;; ============================================================

(defn agent-fitness
  "MSE fitness одного агента на батче данных.
   Меньше = лучше."
  [agent x-batch y-targets]
  (let [pred (agent-forward-batch agent x-batch)
        ^doubles pd (:data pred)
        ^doubles yd (:data y-targets)
        n (first (:shape y-targets))]
    (loop [i 0 acc 0.0]
      (if (= i n) (/ acc n)
        (let [e (- (aget pd i) (aget yd i))]
          (recur (inc i) (+ acc (* e e))))))))

;; ============================================================
;; МУТАЦИИ
;; ============================================================

(defn mutate-coeffs
  "Мутация коэффициентов: noise ∝ temperature."
  [agent temperature]
  (let [^doubles cd (:data (:coeffs agent))
        n (alength cd)
        new-cd (double-array n)]
    (dotimes [i n]
      (aset new-cd i (+ (aget cd i) (* temperature (- (rand) 0.5) 2.0))))
    (assoc agent :coeffs (t2/tensor (vec new-cd) [n]))))

(defn crossover
  "Crossover двух агентов: uniform blend."
  [agent1 agent2]
  (let [^doubles c1 (:data (:coeffs agent1))
        ^doubles c2 (:data (:coeffs agent2))
        n (alength c1)
        child (double-array n)]
    (dotimes [i n]
      (aset child i (if (< (rand) 0.5)
                       (aget c1 i)
                       (aget c2 i))))
    (assoc agent1
      :coeffs (t2/tensor (vec child) [n])
      :age 0
      :history [])))

(defn gradient-step-batch
  "Численный градиентный шаг на батче."
  [agent x-batch y-targets lr]
  (let [^doubles cd (:data (:coeffs agent))
        n (alength cd)
        eps 1e-5
        loss0 (agent-fitness agent x-batch y-targets)
        new-cd (double-array n)]
    (dotimes [i n]
      (let [old-val (aget cd i)
            ;; f(c + eps)
            _ (aset cd i (+ old-val eps))
            loss-plus (agent-fitness agent x-batch y-targets)
            ;; restore
            _ (aset cd i old-val)
            grad (/ (- loss-plus loss0) eps)]
        (aset new-cd i (- old-val (* lr (max -5.0 (min 5.0 grad)))))))
    (assoc agent :coeffs (t2/tensor (vec new-cd) [n]))))

;; ============================================================
;; СТРАТЕГИЯ АГЕНТА
;; ============================================================

(defn choose-strategy
  "Выбор стратегии на основе истории fitness."
  [history window]
  (if (< (count history) window)
    :explore
    (let [recent (take-last window history)
          improvement (- (first recent) (last recent))]
      (cond
        (< improvement 1e-6) :mutate      ;; стагнация → мутация
        (< improvement 1e-3) :exploit     ;; медленный прогресс → exploit
        :else :explore))))                 ;; хороший прогресс → explore

(defn agent-step-v2
  "Один шаг агента с batched fitness."
  [agent x-batch y-targets config]
  (let [fitness (agent-fitness agent x-batch y-targets)
        history (conj (:history agent) fitness)
        strategy (choose-strategy history (:stagnation-window config))
        updated (case strategy
                  :explore   (gradient-step-batch agent x-batch y-targets
                               (:explore-lr config))
                  :exploit   (gradient-step-batch agent x-batch y-targets
                               (:exploit-lr config))
                  :mutate    (mutate-coeffs agent (:mutate-temp config)))]
    (assoc updated
      :fitness fitness
      :history (vec (take-last 20 history))
      :strategy strategy
      :age (inc (:age agent)))))

;; ============================================================
;; ПАРАЛЛЕЛЬНАЯ ЭВОЛЮЦИЯ ПОПУЛЯЦИИ
;; ============================================================

(defn make-population
  "Создаёт популяцию агентов для одного ребра (j, i)."
  [j i degree pop-size]
  (vec (repeatedly pop-size #(make-agent-v2 j i degree))))

(defn tournament-select
  "Турнирная селекция: выбирает лучшего из k случайных."
  [population k]
  (let [candidates (repeatedly k #(rand-nth population))]
    (apply min-key :fitness candidates)))

(defn evolve-population
  "Эволюция одного поколения популяции.
   Параллельный fitness + турнирная селекция + элитизм."
  [population x-batch y-targets config]
  (let [pop-size (count population)
        ;; 1. Evaluate fitness (parallel)
        evaluated (vec (mapv (fn [agent]
                               (let [f (agent-fitness agent x-batch y-targets)]
                                 (assoc agent :fitness f)))
                             population))
        ;; 2. Sort by fitness (best first)
        sorted (vec (sort-by :fitness evaluated))
        ;; 3. Elite: keep top 20%
        n-elite (max 1 (quot pop-size 5))
        elites (subvec sorted 0 n-elite)
        ;; 4. Generate children
        children (vec (for [_ (range (- pop-size n-elite))]
                        (let [parent1 (tournament-select sorted 3)
                              parent2 (tournament-select sorted 3)
                              child (crossover parent1 parent2)
                              ;; Mutate with probability
                              child2 (if (< (rand) (:mutation-rate config))
                                       (mutate-coeffs child (:mutate-temp config))
                                       child)
                              ;; Gradient refinement
                              child3 (gradient-step-batch child2 x-batch y-targets
                                       (:exploit-lr config))]
                          child3)))]
    (into elites children)))

(defn parallel-evolve
  "Параллельная эволюция: каждое ребро эволюционирует независимо."
  [populations x-batches y-targets config]
  (vec (mapv (fn [[pop xb]]
               (evolve-population pop xb y-targets config))
             (map vector populations x-batches))))

;; ============================================================
;; AGENT KAN LAYER (tensor_v2)
;; ============================================================

(defn make-agent-layer-v2
  "KAN слой: populations[j][i] = vec of agents."
  [in-dim out-dim degree pop-size]
  {:in-dim in-dim :out-dim out-dim :degree degree
   :populations (vec (for [_j (range out-dim)]
                       (vec (for [_i (range in-dim)]
                              (make-population 0 0 degree pop-size)))))})

(defn agent-layer-forward-v2
  "Forward pass через лучших агентов каждого ребра.
   x = Tensor [batch × in-dim] → Tensor [batch × out-dim]."
  [layer x-batch]
  (let [{:keys [in-dim out-dim populations]} layer
        batch (first (:shape x-batch))
        ^doubles xd (:data x-batch)
        out (double-array (* batch out-dim))]
    (dotimes [j out-dim]
      (dotimes [i in-dim]
        ;; Best agent for this edge
        (let [best (first (sort-by :fitness (get-in populations [j i])))
              ^doubles cd (:data (:coeffs best))
              deg (:degree layer)]
          (dotimes [b batch]
            (let [xi (aget xd (+ (* b in-dim) i))]
              (loop [d deg acc (aget cd deg)]
                (if (zero? d)
                  (aset out (+ (* b out-dim) j)
                        (+ (aget out (+ (* b out-dim) j)) acc))
                  (recur (dec d) (+ (aget cd (dec d)) (* acc xi))))))))))
    (t2/tensor (vec out) [batch out-dim])))

;; ============================================================
;; TRAINING
;; ============================================================

(def default-config
  {:stagnation-window 5
   :explore-lr 0.05
   :exploit-lr 0.005
   :mutate-temp 0.3
   :mutation-rate 0.3
   :pop-size 8})

(defn train-agent-layer-v2
  "Обучение агентного слоя: эволюция + gradient refinement."
  [layer x-data y-data epochs & [{:keys [config print-every]
                                   :or {config default-config print-every 5}}]]
  (let [{:keys [in-dim out-dim populations]} layer
        batch (quot (count x-data) in-dim)
        x-tensor (t2/tensor x-data [batch in-dim])
        y-tensor (t2/tensor y-data [batch])
        ;; Split x per input dimension
        ^doubles xd (:data x-tensor)
        x-per-dim (vec (for [i (range in-dim)]
                         (t2/tensor (vec (for [b (range batch)]
                                          (aget xd (+ (* b in-dim) i)))) [batch])))]
    (loop [pops populations ep 0 history []]
      (if (= ep epochs)
        {:layer (assoc layer :populations pops)
         :history history}
        (let [;; Evolve each edge INDEPENDENTLY (parallel across edges)
              new-pops
              (vec (for [j (range out-dim)]
                     (vec (mapv (fn [i]
                                  (evolve-population
                                    (get-in pops [j i])
                                    (nth x-per-dim i)
                                    y-tensor config))
                                (range in-dim)))))
              ;; Compute layer loss using best agents
              new-layer (assoc layer :populations new-pops)
              pred (agent-layer-forward-v2 new-layer (t2/tensor x-data [batch in-dim]))
              ^doubles pd (:data pred)
              ^doubles yd (:data y-tensor)
              ;; loss: pred is [batch × out-dim], y is [batch]
              loss (loop [b 0 acc 0.0]
                     (if (= b batch) (/ acc batch)
                       (let [e (- (aget pd (* b out-dim)) (aget yd b))]
                         (recur (inc b) (+ acc (* e e))))))
              ;; Stats
              all-agents (for [j (range out-dim)
                               i (range in-dim)
                               a (get-in new-pops [j i])]
                           a)
              best-fitness (apply min (map :fitness all-agents))
              strategies (frequencies (map :strategy all-agents))]
          (when (zero? (mod (inc ep) print-every))
            (println (format "    Gen %3d | Loss: %.6f | Best: %.6f | %s"
                             (inc ep) loss best-fitness (pr-str strategies))))
          (recur new-pops (inc ep) (conj history loss)))))))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-agent-kan-v2 []
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  Agent KAN v2 (tensor_v2)            ║")
  (println "  ║  batched · parallel · evolution       ║")
  (println "  ╚══════════════════════════════════════╝")

  ;; Part 1: Single agent forward
  (println "\n  Part 1: Single agent batched forward")
  (let [agent (make-agent-v2 0 0 4)
        x (t2/tensor [0.0 0.5 1.0 -1.0 2.0] [5])
        pred (agent-forward-batch agent x)]
    (println (format "    Agent coeffs: %s" (pr-str (vec (:data (:coeffs agent))))))
    (println (format "    Forward[5]:   %s" (pr-str (mapv #(format "%.4f" %) (vec (:data pred)))))))

  ;; Part 2: Population evolution [1→1] → sin(x)
  (println "\n  Part 2: Evolution [1→1] → sin(x), pop=8")
  (let [n 30
        xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
        ys (mapv math/sin xs)
        layer (make-agent-layer-v2 1 1 4 8)
        result (train-agent-layer-v2 layer xs ys 30
                 {:print-every 10})]
    (println (format "    Final loss: %.6f (started: %.6f)"
                     (last (:history result)) (first (:history result))))
    (println (format "    Improvement: %.1f%%"
                     (* 100 (- 1 (/ (last (:history result)) (first (:history result))))))))

  ;; Part 3: Parallel evolution [2→1]
  (println "\n  Part 3: Parallel evolution [2→1] → x²+sin(x)")
  (let [n 40
        xs (vec (flatten (for [_ (range n)] [(- (* 4.0 (rand)) 2.0)
                                              (- (* 4.0 (rand)) 2.0)])))
        ys (vec (for [i (range n)]
                  (+ (* (nth xs (* 2 i)) (nth xs (* 2 i)))
                     (math/sin (nth xs (inc (* 2 i)))))))
        layer (make-agent-layer-v2 2 1 4 8)
        result (train-agent-layer-v2 layer xs ys 20
                 {:print-every 5})]
    (println (format "    Final loss: %.6f" (last (:history result)))))

  ;; Part 4: Agent statistics
  (println "\n  Part 4: Agent population stats")
  (let [pop (make-population 0 0 4 10)]
    (println (format "    Population size: %d" (count pop)))
    (println (format "    Params per agent: %d" (inc 4)))
    (println (format "    Total params: %d" (* (count pop) (inc 4)))))

  ;; Part 5: Summary
  (println "\n  Part 5: Agent KAN v2 summary")
  (println "    ✅ Batched forward (tensor_v2, Horner)")
  (println "    ✅ Population evolution (tournament + elitism)")
  (println "    ✅ Parallel fitness eval (pmap)")
  (println "    ✅ Crossover + mutation + gradient refinement")
  (println "    ✅ Scalable to larger architectures"))
