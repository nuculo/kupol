(ns kan-kat.kat-decoder
  "Kolmogorov-Arnold Transformer — Decoder-only architecture.
   
   Replaces the traditional MLP feed-forward sublayer with KAN layers,
   while keeping the standard attention mechanism.
   
   Architecture per layer:
     Input → Self-Attention → Add&Norm → KAN FFN → Add&Norm → Output"
  (:require [clojure.math :as math]
            [kan-kat.math :as m]
            [kan-kat.kan-layer :as kan]))

;; ============================================================
;; Configuration
;; ============================================================

(def default-config
  {:d-model    60
   :n-heads    6
   :n-layers   6
   :vocab-size 256
   :max-seq-len 1024})

;; ============================================================
;; Attention Mechanism
;; ============================================================

(defn apply-mask
  "Apply causal mask to attention scores.
   mask: [[Int...] ...], 1 = keep, 0 = mask to -inf."
  [mask scores]
  (mapv (fn [m-row s-row]
          (mapv (fn [m s]
                  (if (= m 1) s -1e9))
                m-row s-row))
        mask scores))

(defn attention
  "Scaled dot-product attention: softmax(Q·K^T / √d_k) · V"
  [q k v mask]
  (let [dk (double (count (first q)))
        q-k-t  (m/mat-mul q (m/transpose k))
        scaled (m/mat-scale (/ 1.0 (math/sqrt dk)) q-k-t)
        masked (apply-mask mask scaled)
        attn-weights (mapv m/softmax-vec masked)]
    (m/mat-mul attn-weights v)))

;; ============================================================
;; Layer Normalization
;; ============================================================

(defn layer-norm
  "Simplified layer normalization (no learnable gamma/beta)."
  [matrix]
  (mapv (fn [row]
          (let [n  (count row)
                mu (/ (reduce + 0.0 row) n)
                v  (/ (reduce + 0.0 (map #(let [d (- % mu)] (* d d)) row)) n)
                std (math/sqrt (+ v 1e-5))]
            (mapv #(/ (- % mu) std) row)))
        matrix))

;; ============================================================
;; Decoder Layer
;; ============================================================

(defn make-decoder-layer
  "Create a decoder layer with a KAN sublayer."
  [kan-layer]
  {:kan kan-layer})

(defn forward-decoder-layer
  "Forward pass for a single decoder layer.
   1) Self-Attention + residual + LayerNorm
   2) KAN FFN + residual + LayerNorm"
  [layer x mask]
  (let [;; Self-Attention (simplified: Q=K=V=x)
        attn-out (attention x x x mask)
        ;; Residual + Norm 1
        x1 (layer-norm (m/mat-add x attn-out))
        ;; KAN Feed-Forward (each token independently)
        kan-out (kan/forward-layer-batch (:kan layer) x1)
        ;; Residual + Norm 2
        x2 (layer-norm (m/mat-add x1 kan-out))]
    x2))

;; ============================================================
;; Full Decoder-only Model
;; ============================================================

(defn make-causal-mask
  "Generate lower-triangular causal mask of size [seq-len × seq-len]."
  [seq-len]
  (mapv (fn [i]
          (mapv (fn [j]
                  (if (<= j i) 1 0))
                (range seq-len)))
        (range seq-len)))

(defn get-embeddings
  "Look up token embeddings from embedding matrix."
  [emb-matrix tokens]
  (mapv #(nth emb-matrix %) tokens))

(defn forward-kat
  "Full forward pass of the KAT decoder-only model.
   model: {:embed Matrix, :pos-embed Matrix, :layers [DecoderLayer], :lm-head Matrix}
   tokens: [Int...]
   Returns: logits matrix [seq-len, vocab-size]"
  [model tokens]
  (let [seq-len (count tokens)
        ;; Token embeddings
        tok-emb (get-embeddings (:embed model) tokens)
        ;; Positional embeddings
        pos-e   (subvec (:pos-embed model) 0 seq-len)
        ;; Combine
        x (m/mat-add tok-emb pos-e)
        ;; Causal mask
        mask (make-causal-mask seq-len)
        ;; Pass through all decoder layers
        out (reduce (fn [acc layer]
                      (forward-decoder-layer layer acc mask))
                    x (:layers model))
        ;; LM head: [seq-len, d-model] × [d-model, vocab-size] → [seq-len, vocab-size]
        logits (m/mat-mul out (:lm-head model))]
    logits))

;; ============================================================
;; Simple Tokenizer
;; ============================================================

(defn encode-text
  "Encode string as character-level token IDs."
  [s]
  (mapv int s))

(defn decode-text
  "Decode token IDs back to string."
  [tokens]
  (apply str (map char tokens)))
