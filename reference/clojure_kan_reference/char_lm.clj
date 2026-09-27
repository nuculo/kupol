(ns kan-kat.char-lm
  "Character-level Language Modeling с KAT.
   
   ═══════════════════════════════════════════════════
   АРХИТЕКТУРА
   ═══════════════════════════════════════════════════
   
   Используем наш KAT (kat_decoder.clj) для задачи
   предсказания следующего символа.
   
   Модель: крошечная (vocab~20, embed=8, 1 layer, 2 heads)
   Обучение: числовые градиенты (конечные разности)
   Генерация: greedy + temperature sampling
   
   ═══════════════════════════════════════════════════
   ЗАЧЕМ ЧИСЛОВЫЕ ГРАДИЕНТЫ?
   ═══════════════════════════════════════════════════
   
   Полный backprop через attention — отдельная сложная задача.
   Для toy-демо числовые градиенты через forward-kat работают
   корректно, хотя медленнее. Это доказывает, что KAT *обучаем*
   и может моделировать последовательности символов.
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.math :as m]
            [kan-kat.kan-layer :as kan]
            [kan-kat.training :as tr]
            [kan-kat.kat-decoder :as kat]))

;; ============================================================
;; СЛОВАРЬ (маленький, для toy-задачи)
;; ============================================================

(defn build-vocab
  "Строит char→idx и idx→char из текста."
  [text]
  (let [chars (vec (sort (distinct text)))
        c->i (into {} (map-indexed (fn [i c] [c i]) chars))
        i->c (into {} (map-indexed (fn [i c] [i c]) chars))]
    {:chars chars :char->idx c->i :idx->char i->c :vocab-size (count chars)}))

(defn encode
  "Текст → вектор индексов."
  [text vocab]
  (mapv (:char->idx vocab) text))

(defn decode
  "Вектор индексов → текст."
  [ids vocab]
  (apply str (map (:idx->char vocab) ids)))

;; ============================================================
;; СОЗДАНИЕ КРОШЕЧНОЙ KAT-МОДЕЛИ
;; ============================================================

(defn make-tiny-kat
  "Создание маленькой KAT для char-level LM.
   vocab-size — размер словаря
   d-model    — размерность embedding (рек. 8-16)
   n-heads    — число голов (рек. 2)
   G          — grid size для KAN (рек. 3-4)
   k          — spline order (рек. 3)"
  [vocab-size d-model n-heads G k]
  (let [;; Embedding: [vocab-size × d-model]
        embed (vec (for [_ (range vocab-size)]
                     (vec (repeatedly d-model #(* 0.1 (- (rand) 0.5))))))
        ;; Positional embedding (sinusoidal)
        max-seq 32
        pos-embed (vec (for [pos (range max-seq)]
                         (vec (for [i (range d-model)]
                                (if (even? i)
                                  (math/sin (/ pos (math/pow 10000.0 (/ i d-model))))
                                  (math/cos (/ pos (math/pow 10000.0 (/ (dec i) d-model)))))))))
        ;; 1 decoder layer с KAN FFN [d-model → d-model]
        kan-layer (tr/init-random-kan d-model d-model k G [-2.0 2.0])
        layer (kat/make-decoder-layer kan-layer)
        ;; LM head: [d-model × vocab-size]
        lm-head (vec (for [_ (range d-model)]
                       (vec (repeatedly vocab-size #(* 0.1 (- (rand) 0.5))))))]
    {:embed embed
     :pos-embed pos-embed
     :layers [layer]
     :lm-head lm-head}))

;; ============================================================
;; ИЗВЛЕЧЕНИЕ / ИНЪЕКЦИЯ ПАРАМЕТРОВ
;; ============================================================

(defn flatten-model-params
  "Извлечение всех обучаемых параметров модели в плоский вектор.
   Порядок: [embed | kan-params | lm-head]"
  [model]
  (vec (concat
         (mapcat identity (:embed model))
         (kan/extract-params (get-in model [:layers 0 :kan]))
         (mapcat identity (:lm-head model)))))

(defn inject-model-params
  "Инъекция плоского вектора параметров обратно в модель."
  [model params]
  (let [vocab-size (count (:embed model))
        d-model    (count (first (:embed model)))
        emb-size   (* vocab-size d-model)
        kan-l      (get-in model [:layers 0 :kan])
        kan-size   (kan/num-params kan-l)
        head-size  (* d-model (count (first (:lm-head model))))
        ;; Split
        emb-flat   (subvec params 0 emb-size)
        kan-flat   (subvec params emb-size (+ emb-size kan-size))
        head-flat  (subvec params (+ emb-size kan-size)
                           (+ emb-size kan-size head-size))
        ;; Reshape
        new-embed  (mapv vec (partition d-model emb-flat))
        new-kan    (kan/inject-params kan-l kan-flat)
        vocab-out  (count (first (:lm-head model)))
        new-head   (mapv vec (partition vocab-out head-flat))]
    (-> model
        (assoc :embed new-embed)
        (assoc-in [:layers 0 :kan] new-kan)
        (assoc :lm-head new-head))))

;; ============================================================
;; CROSS-ENTROPY LOSS
;; ============================================================

(defn softmax-vec
  "Softmax для одного вектора."
  [logits]
  (let [mx (apply max logits)
        exps (mapv #(math/exp (- % mx)) logits)
        s (reduce + exps)]
    (mapv #(/ % s) exps)))

(defn cross-entropy-loss
  "Cross-entropy loss: -log(softmax(logits)[target]).
   Использует последнюю позицию для предсказания следующего токена.
   
   tokens  — входная последовательность [t0 t1 ... tn-1]
   target  — целевой токен (следующий после последнего)"
  [model tokens target]
  (let [logits (kat/forward-kat model tokens)
        ;; Берём логиты последней позиции
        last-logits (last logits)
        probs (softmax-vec last-logits)
        p (max 1e-10 (nth probs target))]
    (- (math/log p))))

;; ============================================================
;; ЧИСЛОВЫЕ ГРАДИЕНТЫ
;; ============================================================

(defn numerical-grad-charlm
  "Числовой градиент через конечные разности.
   Обновляем только подмножество параметров для скорости."
  [model tokens target eps]
  (let [params (flatten-model-params model)
        n      (count params)
        ;; Для скорости: обновляем случайные 30% параметров
        indices (take (max 10 (quot n 3))
                      (shuffle (range n)))]
    (reduce
      (fn [grads idx]
        (let [p+  (assoc params idx (+ (nth params idx) eps))
              p-  (assoc params idx (- (nth params idx) eps))
              l+  (cross-entropy-loss (inject-model-params model p+) tokens target)
              l-  (cross-entropy-loss (inject-model-params model p-) tokens target)]
          (assoc grads idx (/ (- l+ l-) (* 2.0 eps)))))
      (vec (repeat n 0.0))
      indices)))

;; ============================================================
;; ОБУЧЕНИЕ С ADAM
;; ============================================================

(defn train-char-lm
  "Обучение KAT на character-level данных.
   
   Алгоритм:
   1. Из текста нарезаем окна [ctx-len символов] → [следующий символ]
   2. На каждой эпохе берём мини-батч окон
   3. Числовой градиент + Adam update
   4. Выводим loss
   
   Возвращает обученную модель."
  [model vocab text ctx-len epochs lr batch-size]
  (let [encoded  (encode text vocab)
        n-tokens (count encoded)
        ;; Нарезаем обучающие примеры
        examples (mapv (fn [i]
                        {:tokens (subvec encoded i (+ i ctx-len))
                         :target (nth encoded (+ i ctx-len))})
                      (range (- n-tokens ctx-len)))
        n-params (count (flatten-model-params model))
        eps      1e-4]

    (loop [m     model
           adam-m (vec (repeat n-params 0.0))
           adam-v (vec (repeat n-params 0.0))
           step   0
           epoch  1]
      (if (> epoch epochs)
        m
        (let [;; Случайный мини-батч
              batch (take batch-size (shuffle examples))
              ;; Средний градиент по батчу
              avg-grad
              (reduce (fn [acc ex]
                        (let [g (numerical-grad-charlm m (:tokens ex) (:target ex) eps)]
                          (mapv + acc g)))
                      (vec (repeat n-params 0.0))
                      batch)
              avg-grad (mapv #(/ % (double batch-size)) avg-grad)
              ;; Adam
              t      (inc step)
              beta1  0.9
              beta2  0.999
              adam-eps 1e-8
              m'     (mapv (fn [mi gi] (+ (* beta1 mi) (* (- 1.0 beta1) gi)))
                           adam-m avg-grad)
              v'     (mapv (fn [vi gi] (+ (* beta2 vi) (* (- 1.0 beta2) (* gi gi))))
                           adam-v avg-grad)
              m-hat  (mapv #(/ % (- 1.0 (math/pow beta1 t))) m')
              v-hat  (mapv #(/ % (- 1.0 (math/pow beta2 t))) v')
              params (flatten-model-params m)
              params' (mapv (fn [p mh vh]
                              (- p (/ (* lr mh) (+ (math/sqrt vh) adam-eps))))
                            params m-hat v-hat)
              new-model (inject-model-params m params')
              ;; Loss для мониторинга (на первом примере батча)
              sample-loss (cross-entropy-loss new-model
                                              (:tokens (first batch))
                                              (:target (first batch)))]
          (println (format "  Epoch %d/%d | Step %d | Loss: %.4f | Params: %d"
                           epoch epochs t sample-loss n-params))
          (recur new-model m' v' t (inc epoch)))))))

;; ============================================================
;; ГЕНЕРАЦИЯ ТЕКСТА (с temperature sampling)
;; ============================================================

(defn sample-with-temperature
  "Temperature sampling: мягкий softmax + сэмплирование.
   temp=0 → greedy (argmax)
   temp→1 → стандартный softmax
   temp>1 → более случайный"
  [logits temperature]
  (if (or (nil? temperature) (<= temperature 0.01))
    ;; Greedy: argmax
    (first (apply max-key second (map-indexed vector logits)))
    ;; Temperature sampling
    (let [scaled (mapv #(/ % temperature) logits)
          mx     (apply max scaled)
          exps   (mapv #(math/exp (- % mx)) scaled)
          s      (reduce + exps)
          probs  (mapv #(/ % s) exps)
          ;; Cumulative → sample
          r      (rand)
          cum    (reductions + probs)]
      (loop [i 0]
        (if (or (>= i (dec (count cum)))
                (>= (nth cum i) r))
          i
          (recur (inc i)))))))

(defn generate-text
  "Генерация текста авторегрессивно.
   seed        — начальный текст (строка, >= ctx-len символов)
   n-chars     — сколько символов сгенерировать
   ctx-len     — размер контекстного окна
   temperature — 0.0=greedy, 0.7=balanced, 1.0+=creative (опционально)"
  ([model vocab seed ctx-len n-chars]
   (generate-text model vocab seed ctx-len n-chars nil))
  ([model vocab seed ctx-len n-chars temperature]
   (let [seed-ids (encode seed vocab)]
     (loop [ids seed-ids
            remaining n-chars]
       (if (zero? remaining)
         (decode ids vocab)
         (let [ctx (subvec ids (max 0 (- (count ids) ctx-len)))
               logits (kat/forward-kat model ctx)
               last-logits (last logits)
               next-id (sample-with-temperature last-logits temperature)]
           (recur (conj ids next-id) (dec remaining))))))))

