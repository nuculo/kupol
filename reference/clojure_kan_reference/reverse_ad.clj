(ns kan-kat.reverse-ad
  "Reverse-mode (backward pass) for the LoRA+LMHead+CrossEntropy chain.
   Instead of N forward passes (forward-mode), we do 1 forward + 1 backward.
   
   Chain:  x → LoRA(x + x*A*B) → LMHead(×W) → softmax → cross-entropy
   We derive dL/dA and dL/dB analytically."
  (:require [clojure.math :as math]
            [kan-kat.math :as m]))

(defn softmax
  "Softmax of a vector (numerically stable)."
  [v]
  (let [mx (apply max v)
        exps (mapv #(math/exp (- % mx)) v)
        s    (reduce + 0.0 exps)]
    (mapv #(/ % s) exps)))

(defn lora-backward
  "Forward + Backward for the LoRA→LMHead→CrossEntropy chain.
   Returns [loss, grad-a, grad-b].
   
   Forward:
     tmp1     = x * A              [seq, rank]
     tmp2     = tmp1 * B           [seq, dim]
     lora-out = x + tmp2           [seq, dim]
     logits   = lora-out * Head    [seq, vocab]
     probs    = softmax(logits)    [seq, vocab]
     loss     = mean(-log(probs[target]))
   
   Backward:
     dL/dLogits  = probs - one_hot(target)   [seq, vocab]
     dL/dLoraOut = dLogits * Head^T           [seq, dim]
     dL/dA       = x^T * (dLoraOut * B^T)    [dim, rank]
     dL/dB       = tmp1^T * dLoraOut          [rank, dim]"
  [x a b head-w targets]
  (let [;; Forward pass
        tmp1     (m/mat-mul x a)         ;; [seq, rank]
        tmp2     (m/mat-mul tmp1 b)      ;; [seq, dim]
        lora-out (m/mat-add x tmp2)      ;; [seq, dim]
        logits   (m/mat-mul lora-out head-w) ;; [seq, vocab]
        probs    (mapv softmax logits)   ;; [seq, vocab]
        
        ;; Loss: mean cross-entropy
        seq-len  (count targets)
        losses   (mapv (fn [p t]
                         (- (math/log (max 1e-10 (nth p t)))))
                       probs targets)
        loss     (/ (reduce + 0.0 losses) seq-len)
        
        ;; Backward: dL/dLogits = (probs - one_hot) / seq-len
        d-logits (mapv (fn [p t]
                         (let [n (double seq-len)]
                           (mapv (fn [pi i]
                                   (/ (- pi (if (= i t) 1.0 0.0)) n))
                                 p (range (count p)))))
                       probs targets)
        
        ;; dL/dLoraOut = dLogits * Head^T
        head-t    (m/transpose head-w)
        d-lora-out (m/mat-mul d-logits head-t)
        
        ;; dL/dtmp2 = dL/dLoraOut (since lora-out = x + tmp2)
        d-tmp2    d-lora-out
        
        ;; dL/dtmp1 = dTmp2 * B^T
        b-t       (m/transpose b)
        d-tmp1    (m/mat-mul d-tmp2 b-t)
        
        ;; dL/dA = x^T * dTmp1
        x-t       (m/transpose x)
        grad-a    (m/mat-mul x-t d-tmp1)
        
        ;; dL/dB = tmp1^T * dTmp2
        tmp1-t    (m/transpose tmp1)
        grad-b    (m/mat-mul tmp1-t d-tmp2)]
    
    [loss grad-a grad-b]))

(defn clip-grad
  "Gradient clipping by max element value."
  [max-val grad]
  (mapv (fn [row]
          (mapv #(max (- max-val) (min max-val %)) row))
        grad))
