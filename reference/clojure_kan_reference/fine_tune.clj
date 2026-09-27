(ns kan-kat.fine-tune
  "Full fine-tuning loop for KAT with LoRA, using Dual-number AD.
   Features: Adam optimizer, gradient clipping, cross-entropy loss.
   LoRA has very few parameters (2 * dim * rank), making forward-mode AD tractable."
  (:require [clojure.math :as math]
            [kan-kat.ad :as ad]
            [kan-kat.kan-layer :as kan]
            [kan-kat.lora :as lora]
            [kan-kat.math :as m]
            [kan-kat.training :as tr]))

;; ============================================================
;; Fine-Tune Model
;; ============================================================

(defn make-ft-layer
  "Create a fine-tune layer with KAN and LoRA."
  [kan-layer lora-adapter]
  {:kan kan-layer :lora lora-adapter})

(defn init-ft-model
  "Initialize a small fine-tune model."
  [dim n-layers lora-rank kan-grid]
  {:layers (mapv (fn [_]
                   (make-ft-layer
                     (tr/init-random-kan dim dim 3 kan-grid [-2.0 2.0])
                     (lora/init-lora dim lora-rank)))
                 (range n-layers))
   :dim dim})

;; ============================================================
;; Forward pass
;; ============================================================

(defn forward-ft-layer
  "Forward: KAN → LoRA."
  [layer x]
  (let [kan-out (kan/forward-layer-batch (:kan layer) x)]
    (lora/apply-lora (:lora layer) kan-out)))

(defn forward-ft-model
  "Forward through all layers."
  [model x]
  (reduce (fn [acc l] (forward-ft-layer l acc))
          x (:layers model)))

;; ============================================================
;; Parameter extraction/injection (only LoRA is trainable)
;; ============================================================

(defn extract-all-lora
  [model]
  (vec (mapcat #(lora/extract-lora-params (:lora %)) (:layers model))))

(defn inject-all-lora
  [model params]
  (loop [layers (:layers model)
         ps params
         result []]
    (if (empty? layers)
      (assoc model :layers result)
      (let [layer (first layers)
            n (lora/num-lora-params (:lora layer))
            [these rest-ps] (split-at n ps)
            layer' (assoc layer :lora (lora/inject-lora-params (:lora layer) these))]
        (recur (rest layers) rest-ps (conj result layer'))))))

;; ============================================================
;; AD-based loss computation
;; ============================================================

(defn forward-ft-layer-ad
  "AD forward: KAN frozen (const-d), LoRA trainable (Dual)."
  [layer dim rank lora-params x]
  (let [kan-l    (:kan layer)
        k        (:spline-order kan-l)
        n-spl    (kan/num-spl-coeffs kan-l)
        ;; Per-input grids as Dual constants
        grids-a  (mapv (fn [knots] (mapv ad/const-d knots)) (:grids kan-l))
        kan-ps   (mapv ad/const-d (kan/extract-params kan-l))
        kan-out  (mapv (fn [row]
                         (kan/forward-generic
                           (:in-features kan-l) (:out-features kan-l)
                           k n-spl grids-a kan-ps row))
                       x)]
    (lora/apply-lora-generic dim rank lora-params kan-out)))

(defn split-lora-params
  "Split flat LoRA param vector by layers."
  [params layers]
  (loop [ls layers ps params result []]
    (if (empty? ls)
      result
      (let [n (lora/num-lora-params (:lora (first ls)))
            [these rest-ps] (split-at n ps)]
        (recur (rest ls) rest-ps (conj result these))))))

(defn fine-tune-loss
  "Cross-entropy loss via AD over the full fine-tune model."
  [model input-embs targets lora-params]
  (let [param-chunks (split-lora-params lora-params (:layers model))
        output (reduce (fn [x [layer lps]]
                         (forward-ft-layer-ad layer (:dim model)
                                              (:rank (:lora layer)) lps x))
                       (mapv (fn [row] (mapv ad/const-d row)) input-embs)
                       (map vector (:layers model) param-chunks))
        losses (mapv (fn [out-row tgt]
                       (let [max-v (reduce (fn [a b] (if (> (ad/d-val a) (ad/d-val b)) a b))
                                           out-row)
                             exps (mapv #(ad/d-exp (ad/d-sub % max-v)) out-row)
                             sum-e (ad/d-sum exps)
                             probs (mapv #(ad/d-div % sum-e) exps)
                             p (nth probs tgt)]
                         (ad/d-negate (ad/d-log (ad/d-add p (ad/const-d 1e-10))))))
                     output targets)]
    (ad/d-div (ad/d-sum losses) (ad/const-d (double (count losses))))))

;; ============================================================
;; Adam Optimizer
;; ============================================================

(defn init-adam
  [n]
  {:m (vec (repeat n 0.0))
   :v (vec (repeat n 0.0))
   :step 0})

(defn adam-update
  "Adam: params' = params - lr * m_hat / (sqrt(v_hat) + eps)."
  [lr beta1 beta2 eps state params grads]
  (let [t (inc (:step state))
        max-grad 1.0
        clipped (mapv #(max (- max-grad) (min max-grad %)) grads)
        m' (mapv (fn [mi gi] (+ (* beta1 mi) (* (- 1.0 beta1) gi)))
                 (:m state) clipped)
        v' (mapv (fn [vi gi] (+ (* beta2 vi) (* (- 1.0 beta2) (* gi gi))))
                 (:v state) clipped)
        m-hat (mapv #(/ % (- 1.0 (math/pow beta1 t))) m')
        v-hat (mapv #(/ % (- 1.0 (math/pow beta2 t))) v')
        params' (mapv (fn [p mh vh] (- p (/ (* lr mh) (+ (math/sqrt vh) eps))))
                      params m-hat v-hat)]
    [{:m m' :v v' :step t} params']))

;; ============================================================
;; Training step and loop
;; ============================================================

(defn train-step-adam
  [model input-embs targets adam-st lr]
  (let [params  (extract-all-lora model)
        loss-val (:primal (fine-tune-loss model input-embs targets (mapv ad/const-d params)))
        grads   (ad/compute-gradient
                  #(fine-tune-loss model input-embs targets %)
                  params)
        [adam-st' params'] (adam-update lr 0.9 0.999 1e-8 adam-st params grads)
        model'  (inject-all-lora model params')]
    [model' adam-st' loss-val]))

(defn fine-tune
  "Full fine-tuning with Adam."
  [model dataset lr epochs]
  (let [n-params (count (extract-all-lora model))
        adam-0   (init-adam n-params)]
    (loop [m model adam adam-0 epoch 1]
      (if (> epoch epochs)
        m
        (let [[m' adam' total-loss cnt]
              (reduce (fn [[m a acc cnt] [embs tgts]]
                        (let [[m' a' loss] (train-step-adam m embs tgts a lr)]
                          [m' a' (+ acc loss) (inc cnt)]))
                      [m adam 0.0 0]
                      dataset)
              avg-loss (/ total-loss (max 1 cnt))]
          (println (str "  Epoch " epoch "/" epochs
                        " | Loss: " (format "%.6f" avg-loss)
                        " | LoRA params: " (count (extract-all-lora m'))))
          (recur m' adam' (inc epoch)))))))

;; ============================================================
;; Demo
;; ============================================================

(defn demo-fine-tune
  "LoRA Fine-Tune demo (Adam + AD)."
  []
  (println "==========================================")
  (println " Demo: LoRA Fine-Tune (Adam + AD)")
  (println "==========================================")
  
  (let [model (init-ft-model 4 2 2 3)
        dataset [[(mapv double [[ 1  0  0  0] [ 0  1  0  0]]) [2 0]]
                 [(mapv double [[ 0  0  1  0] [ 0  0  0  1]]) [1 3]]
                 [(mapv double [[ 1  1  0  0] [ 0  0  1  1]]) [3 1]]]
        ;; Fix: dataset needs to be [[Matrix, [Int]]]
        dataset' (mapv (fn [[embs tgts]]
                         [(mapv (fn [r] (mapv double r)) embs) tgts])
                       dataset)
        trained (fine-tune model dataset' 0.01 15)]
    (println "\nFine-tuning complete!")
    (let [test-input [[1.0 0.0 0.0 0.0] [0.0 1.0 0.0 0.0]]
          output (forward-ft-model trained test-input)]
      (println "\nPredictions after fine-tune:")
      (doseq [[inp out] (map vector test-input output)]
        (println (str "  Input: " inp
                      " → Predicted class: " (m/argmax out)))))))
