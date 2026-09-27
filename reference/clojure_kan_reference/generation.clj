(ns kan-kat.generation
  "Text generation utilities: cross-entropy loss, top-k sampling,
   autoregressive generation."
  (:require [clojure.math :as math]
            [kan-kat.math :as m]))

;; ============================================================
;; Loss functions
;; ============================================================

(defn cross-entropy-loss
  "Cross-entropy loss: -log(softmax(logits)[target-idx])."
  [logits target-idx]
  (let [probs (m/softmax-vec logits)
        p     (max 1e-10 (nth probs target-idx))]
    (- (math/log p))))

(defn sequence-loss
  "Average cross-entropy over a sequence of [logits, target] pairs."
  [logits-seq targets]
  (let [losses (mapv cross-entropy-loss logits-seq targets)]
    (/ (reduce + 0.0 losses) (count losses))))

;; ============================================================
;; Sampling strategies
;; ============================================================

(defn sort-by-prob-desc
  "Sort [(index, prob)] pairs by prob descending."
  [indexed]
  (sort-by second > indexed))

(defn sample-top-k
  "Top-k sampling with temperature. Returns the index of the sampled token."
  [logits temperature k]
  (let [;; Apply temperature
        scaled (mapv #(/ % temperature) logits)
        probs  (m/softmax-vec scaled)
        ;; Get top-k
        indexed (map-indexed vector probs)
        sorted  (sort-by-prob-desc indexed)
        top-k   (take k sorted)
        ;; Renormalize
        top-k-sum (reduce + 0.0 (map second top-k))
        top-k-norm (mapv (fn [[i p]] [i (/ p top-k-sum)]) top-k)
        ;; Sample
        r (rand)]
    (loop [remaining top-k-norm
           acc 0.0]
      (if (empty? remaining)
        (first (first top-k-norm))
        (let [[idx p] (first remaining)]
          (if (<= r (+ acc p))
            idx
            (recur (rest remaining) (+ acc p))))))))

(defn greedy-decode
  "Greedy decoding (argmax)."
  [logits]
  (m/argmax logits))

;; ============================================================
;; Text generation
;; ============================================================

(defn encode-text
  "Character-level tokenization."
  [s]
  (mapv int s))

(defn decode-text
  "Decode token IDs back to string."
  [tokens]
  (apply str (map char tokens)))

(defn generate-text
  "Autoregressive text generation.
   forward-fn: tokens → logits-per-position [[Double...] ...]
   Returns generated string."
  [forward-fn prompt max-new temp top-k]
  (loop [ctx (encode-text prompt)
         remaining max-new]
    (if (zero? remaining)
      (decode-text ctx)
      (let [logits-all  (forward-fn ctx)
            last-logits (last logits-all)
            next-token  (sample-top-k last-logits temp top-k)]
        (recur (conj ctx next-token) (dec remaining))))))
