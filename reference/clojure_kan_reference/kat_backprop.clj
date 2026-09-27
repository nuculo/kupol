(ns kan-kat.kat-backprop
  "Аналитический Backpropagation через полный KAT.
   
   ═══════════════════════════════════════════════════
   АРХИТЕКТУРА BACKWARD (в обратном порядке)
   ═══════════════════════════════════════════════════
   
   Logits → LM Head backward
         → Decoder Block backward:
             ← LN2 backward
             ← KAN backward (уже есть в backprop.clj)
             ← Residual
             ← LN1 backward
             ← Attention backward (softmax Jacobian)
             ← Residual
         → Embedding backward
   
   ═══════════════════════════════════════════════════
   ФОРМУЛЫ ATTENTION BACKWARD
   ═══════════════════════════════════════════════════
   
   Forward: Attn = softmax(X·X^T / √d) · X  (Q=K=V=X)
   
   dV     = Attn^T · dOut
   dAttn  = dOut · V^T
   dScores = softmax_jacobian(Attn, dAttn)
   dX_attn = (dScores + dScores^T) · X / √d + Attn^T · dOut
   
   Softmax Jacobian (per row):
   dS_i = Σ_j dAttn_{i,j} · Attn_{i,j} · (δ_{ij} - Attn_{i,j})
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.math :as m]
            [kan-kat.kan-layer :as kan]
            [kan-kat.backprop :as bp]
            [kan-kat.kat-decoder :as kat]))

;; ============================================================
;; SOFTMAX BACKWARD
;; ============================================================

(defn softmax-backward-row
  "Backward через softmax для одной строки.
   attn-row — softmax output [a_1 ... a_n]
   dupstream — dL/d(softmax output) [du_1 ... du_n]
   
   Якобиан: ∂softmax_i/∂score_j = a_i·(δ_{ij} - a_j)
   dL/dscore_i = Σ_j dL/da_j · a_j · (δ_{ij} - a_i)
              = a_i · (du_i - Σ_j du_j · a_j)"
  [attn-row dupstream]
  (let [dot (reduce + 0.0 (map * dupstream attn-row))]
    (mapv (fn [ai dui]
            (* ai (- dui dot)))
          attn-row dupstream)))

;; ============================================================
;; ATTENTION FORWARD + CACHE
;; ============================================================

(defn attention-forward-cache
  "Attention forward с кэшированием для backward.
   Q=K=V=x (self-attention, как в kat_decoder.clj).
   
   Возвращает [output, cache]."
  [x mask]
  (let [dk   (double (count (first x)))
        ;; Q·K^T / √d
        q-kt (m/mat-mul x (m/transpose x))
        scaled (m/mat-scale (/ 1.0 (math/sqrt dk)) q-kt)
        ;; Causal mask
        masked (kat/apply-mask mask scaled)
        ;; Softmax
        attn-weights (mapv m/softmax-vec masked)
        ;; Attn · V
        output (m/mat-mul attn-weights x)]
    [output {:x x :dk dk :scaled scaled :mask mask
             :attn-weights attn-weights}]))

;; ============================================================
;; ATTENTION BACKWARD
;; ============================================================

(defn attention-backward
  "Backward через scaled dot-product self-attention (Q=K=V=x).
   
   cache     — из attention-forward-cache
   dupstream — dL/d(attention output) [seq-len × d-model]
   
   Возвращает dL/dx [seq-len × d-model].
   
   Формулы:
   dV = Attn^T · dOut           (градиент по V=x через Attn·V)
   dAttn = dOut · V^T           (градиент по весам attention)
   dScores = softmax_jacobian   (через softmax backward)
   dQ = dScores · K / √d        (градиент по Q=x)
   dK = dScores^T · Q / √d      (градиент по K=x)
   
   Поскольку Q=K=V=x: dx = dQ + dK + dV"
  [cache dupstream]
  (let [{:keys [x dk attn-weights mask]} cache
        inv-sqrt-dk (/ 1.0 (math/sqrt dk))
        ;; dV = Attn^T · dOut
        attn-t (m/transpose attn-weights)
        dV (m/mat-mul attn-t dupstream)
        ;; dAttn = dOut · V^T = dOut · x^T
        x-t (m/transpose x)
        dAttn-full (m/mat-mul dupstream x-t)
        ;; Mask: zero out градиенты для замаскированных позиций
        dAttn (mapv (fn [m-row da-row]
                      (mapv (fn [mi dai]
                              (if (= mi 1) dai 0.0))
                            m-row da-row))
                    mask dAttn-full)
        ;; Softmax backward (per row)
        dScores (mapv softmax-backward-row attn-weights dAttn)
        ;; dQ = dScores · K · (1/√d) = dScores · x · (1/√d)
        dQ (m/mat-scale inv-sqrt-dk (m/mat-mul dScores x))
        ;; dK = dScores^T · Q · (1/√d) = dScores^T · x · (1/√d)
        dScores-t (m/transpose dScores)
        dK (m/mat-scale inv-sqrt-dk (m/mat-mul dScores-t x))]
    ;; Q=K=V=x, поэтому dx = dQ + dK + dV
    (m/mat-add (m/mat-add dQ dK) dV)))

;; ============================================================
;; LAYER NORM BACKWARD
;; ============================================================

(defn layer-norm-forward-cache-row
  "LayerNorm forward для одной строки + cache."
  [row]
  (let [n   (count row)
        mu  (/ (reduce + 0.0 row) n)
        d   (mapv #(- % mu) row)
        var (/ (reduce + 0.0 (map #(* % %) d)) n)
        std (math/sqrt (+ var 1e-5))
        normed (mapv #(/ % std) d)]
    [normed {:mu mu :std std :d d :n n}]))

(defn layer-norm-backward-row
  "Backward для LayerNorm одной строки (gamma=1, beta=0).
   dout — dL/d(layernorm output)"
  [dout {:keys [std n]} normed]
  (let [sum1 (reduce + 0.0 dout)
        sum2 (reduce + 0.0 (map * dout normed))]
    (mapv (fn [doi ni]
            (/ (- doi (/ sum1 n) (* ni (/ sum2 n))) std))
          dout normed)))

(defn layer-norm-forward-cache
  "LayerNorm для матрицы [seq × dim] с кэшем."
  [matrix]
  (let [results (mapv layer-norm-forward-cache-row matrix)]
    [(mapv first results) (mapv second results)]))

(defn layer-norm-backward-matrix
  "Backward LayerNorm для матрицы."
  [dupstream caches normed]
  (mapv layer-norm-backward-row dupstream caches normed))

;; ============================================================
;; KAN FFN BACKWARD (переиспользуем backprop.clj)
;; ============================================================

(defn kan-ffn-forward-cache
  "KAN FFN forward по строкам (каждый токен независимо) + cache."
  [kan-layer x]
  [(kan/forward-layer-batch kan-layer x) x])

(defn kan-ffn-backward
  "Backward через KAN FFN.
   Возвращает [new-kan-layer, dx]."
  [kan-layer x-input dupstream]
  ;; kan-layer-backward ожидает batch format
  (let [[grads input-deltas]
        (bp/kan-layer-backward kan-layer x-input dupstream)]
    [grads input-deltas]))

;; ============================================================
;; LM HEAD (линейный слой) BACKWARD
;; ============================================================

(defn lm-head-backward
  "Backward через линейную проекцию lm-head: logits = x · W.
   W [d-model × vocab-size], x [seq-len × d-model]
   
   dW = x^T · dupstream
   dx = dupstream · W^T
   
   Возвращает {:dW, :dx}."
  [lm-head x dupstream]
  (let [dW (m/mat-mul (m/transpose x) dupstream)
        W-t (m/transpose lm-head)
        dx (m/mat-mul dupstream W-t)]
    {:dW dW :dx dx}))

;; ============================================================
;; CROSS-ENTROPY + SOFTMAX BACKWARD
;; ============================================================

(defn cross-entropy-grad-last
  "Градиент cross-entropy + softmax по логитам последней позиции.
   
   dL/dlogits_i = softmax(logits)_i - 1{i=target}
   
   Возвращает вектор градиентов [vocab-size] для последней позиции."
  [logits target]
  (let [probs (m/softmax-vec logits)]
    (mapv (fn [p i]
            (if (= i target) (- p 1.0) p))
          probs (range (count probs)))))

;; ============================================================
;; DECODER BLOCK FORWARD + CACHE
;; ============================================================

(defn decoder-block-forward-cache
  "Forward для одного блока с полным кэшированием.
   Архитектура: x → Attn → Add&Norm1 → KAN → Add&Norm2 → out"
  [layer x mask]
  (let [;; Self-Attention + cache
        [attn-out attn-cache] (attention-forward-cache x mask)
        ;; Residual + LN1
        x-plus-attn (m/mat-add x attn-out)
        [x1 ln1-caches] (layer-norm-forward-cache x-plus-attn)
        ;; KAN FFN
        [kan-out kan-input] (kan-ffn-forward-cache (:kan layer) x1)
        ;; Residual + LN2
        x1-plus-kan (m/mat-add x1 kan-out)
        [x2 ln2-caches] (layer-norm-forward-cache x1-plus-kan)]
    [x2 {:x x :attn-out attn-out :attn-cache attn-cache
         :x-plus-attn x-plus-attn :x1 x1 :ln1-caches ln1-caches
         :ln1-normed x1
         :kan-out kan-out :kan-input kan-input
         :x1-plus-kan x1-plus-kan :x2 x2 :ln2-caches ln2-caches
         :ln2-normed x2}]))

;; ============================================================
;; DECODER BLOCK BACKWARD
;; ============================================================

(defn decoder-block-backward
  "Backward через один decoder block.
   
   Цепочка (в обратном порядке):
   dOut → LN2 back → residual split → KAN back + dx1-residual
        → LN1 back → residual split → Attn back + dx-residual
   
   Возвращает [kan-grads, dx] где dx — gradients для предыдущего блока."
  [layer cache dupstream]
  (let [{:keys [x attn-cache x-plus-attn x1 ln1-caches ln1-normed
                kan-out x1-plus-kan ln2-caches ln2-normed]} cache
        
        ;; LN2 backward
        dx1-plus-kan (layer-norm-backward-matrix dupstream ln2-caches ln2-normed)
        
        ;; Residual split: dx1-plus-kan идёт в KAN backward + в x1
        ;; KAN backward
        [kan-grads kan-dx] (kan-ffn-backward (:kan layer) x1 dx1-plus-kan)
        ;; dx1 = dKAN_input + residual
        dx1 (m/mat-add kan-dx dx1-plus-kan)
        
        ;; LN1 backward
        dx-plus-attn (layer-norm-backward-matrix dx1 ln1-caches ln1-normed)
        
        ;; Residual split: dx-plus-attn идёт в Attn backward + в x
        ;; Attention backward
        dx-attn (attention-backward attn-cache dx-plus-attn)
        ;; dx = dAttn_input + residual
        dx (m/mat-add dx-attn dx-plus-attn)]
    
    [kan-grads dx]))

;; ============================================================
;; FULL KAT FORWARD + BACKWARD
;; ============================================================

(defn kat-forward-cache
  "Forward через весь KAT с кэшированием.
   Возвращает [logits, caches]."
  [model tokens]
  (let [seq-len (count tokens)
        tok-emb (kat/get-embeddings (:embed model) tokens)
        pos-e   (subvec (:pos-embed model) 0 seq-len)
        x       (m/mat-add tok-emb pos-e)
        mask    (kat/make-causal-mask seq-len)
        ;; Forward through blocks with cache
        [final-out block-caches]
        (reduce (fn [[cur-x caches] layer]
                  (let [[out cache] (decoder-block-forward-cache layer cur-x mask)]
                    [out (conj caches cache)]))
                [x []]
                (:layers model))
        ;; LM head
        logits (m/mat-mul final-out (:lm-head model))]
    [logits {:tokens tokens :x x :block-caches block-caches
             :final-out final-out}]))

(defn kat-backward
  "Полный backward через KAT.
   
   target        — целевой токен (для последней позиции)
   
   Возвращает {:embed-grads, :kan-grads, :head-grads, :loss}."
  [model tokens target]
  (let [;; Forward
        [logits caches] (kat-forward-cache model tokens)
        {:keys [final-out block-caches x]} caches
        
        ;; Loss + gradient на последней позиции
        last-logits (last logits)
        probs (m/softmax-vec last-logits)
        loss  (- (math/log (max 1e-10 (nth probs target))))
        
        ;; dL/dlogits: [seq-len × vocab-size]
        ;; Только последняя позиция имеет ненулевой градиент
        seq-len  (count tokens)
        vocab-sz (count (first logits))
        d-model  (count (first final-out))
        dlogits  (mapv (fn [pos]
                         (if (= pos (dec seq-len))
                           (cross-entropy-grad-last last-logits target)
                           (vec (repeat vocab-sz 0.0))))
                       (range seq-len))
        
        ;; LM head backward
        head-result (lm-head-backward (:lm-head model) final-out dlogits)
        
        ;; Decoder blocks backward (reverse order)
        [all-kan-grads dx-final]
        (reduce (fn [[grads-acc upstream] l-idx]
                  (let [layer (nth (:layers model) l-idx)
                        cache (nth block-caches l-idx)
                        [kan-grads dx] (decoder-block-backward layer cache upstream)]
                    [(assoc grads-acc l-idx kan-grads) dx]))
                [(vec (repeat (count (:layers model)) nil))
                 (:dx head-result)]
                (range (dec (count (:layers model))) -1 -1))
        
        ;; Embedding gradient: dx-final содержит ∂L/∂(embedding+pos)
        ;; Для каждого токена обновляем соответствующую строку embedding
        embed-grads (mapv (fn [s-idx]
                           [(nth tokens s-idx) (nth dx-final s-idx)])
                         (range seq-len))]
    
    {:loss loss
     :head-grads (:dW head-result)
     :kan-grads all-kan-grads
     :embed-grads embed-grads}))

;; ============================================================
;; ОБНОВЛЕНИЕ МОДЕЛИ ПО ГРАДИЕНТАМ (SGD)
;; ============================================================

(defn apply-kat-grads
  "Обновляет все параметры KAT по градиентам.
   Возвращает обновлённую модель."
  [model grads lr]
  (let [{:keys [head-grads kan-grads embed-grads]} grads
        ;; Update LM head
        new-head (m/mat-add (:lm-head model)
                            (m/mat-scale (- lr) head-grads))
        ;; Update KAN layers
        new-layers
        (mapv (fn [layer kan-g l-idx]
                (if kan-g
                  (assoc layer :kan
                         (bp/apply-grads-to-layer (:kan layer) kan-g lr))
                  layer))
              (:layers model) kan-grads (range))
        ;; Update embeddings
        new-embed
        (reduce (fn [emb [tok-idx grad-vec]]
                  (update emb tok-idx
                          (fn [row]
                            (mapv (fn [r g] (- r (* lr g))) row grad-vec))))
                (:embed model)
                embed-grads)]
    (assoc model
           :lm-head new-head
           :layers new-layers
           :embed new-embed)))

;; ============================================================
;; TRAINING LOOP С АНАЛИТИЧЕСКИМ BACKWARD
;; ============================================================

(defn train-kat-analytical
  "Обучение KAT с аналитическим backprop (100×+ быстрее числовых градиентов).
   
   model    — KAT модель
   examples — [{:tokens [..] :target int} ...]
   lr       — learning rate
   epochs   — число эпох
   batch-sz — размер мини-батча"
  [model examples lr epochs batch-sz]
  (loop [m model epoch 1]
    (if (> epoch epochs)
      m
      (let [batch     (take batch-sz (shuffle examples))
            [new-m total-loss]
            (reduce (fn [[cur-m acc-loss] ex]
                      (let [grads (kat-backward cur-m (:tokens ex) (:target ex))
                            updated (apply-kat-grads cur-m grads lr)]
                        [updated (+ acc-loss (:loss grads))]))
                    [m 0.0]
                    batch)
            avg-loss (/ total-loss (max 1 (count batch)))]
        (println (format "  Epoch %d/%d | Loss: %.4f" epoch epochs avg-loss))
        (recur new-m (inc epoch))))))
