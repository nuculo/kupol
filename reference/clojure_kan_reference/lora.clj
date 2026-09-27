(ns kan-kat.lora
  "LoRA (Low-Rank Adaptation) for efficient fine-tuning.
   Instead of updating all W parameters, learn small A (d×r) and B (r×d)
   so that W_new = W_original + A * B, where r << d."
  (:require [kan-kat.math :as m]
            [kan-kat.ad :as ad]))

;; ============================================================
;; LoRA Adapter structure
;; ============================================================
;; {:lora-a [[Double...] ...]   ;; [dim, rank]
;;  :lora-b [[Double...] ...]   ;; [rank, dim]
;;  :rank   Int
;;  :dim    Int}

(defn init-lora
  "Initialize LoRA with small random A and zero B (starts as identity)."
  [dim rank]
  {:lora-a (mapv (fn [_]
                   (mapv (fn [_] (+ -0.01 (* 0.02 (rand))))
                         (range rank)))
                 (range dim))
   :lora-b (mapv (fn [_] (vec (repeat dim 0.0)))
                 (range rank))
   :rank rank
   :dim  dim})

(defn apply-lora
  "Apply LoRA: output = x + x * A * B."
  [lora x]
  (let [delta (m/mat-mul (m/mat-mul x (:lora-a lora)) (:lora-b lora))]
    (m/mat-add x delta)))

(defn extract-lora-params
  "Extract LoRA parameters as flat vector."
  [lora]
  (vec (concat (apply concat (:lora-a lora))
               (apply concat (:lora-b lora)))))

(defn inject-lora-params
  "Inject flat params back into LoRA."
  [lora params]
  (let [r (:rank lora)
        d (:dim lora)
        n-a (* d r)
        [a-flat b-flat] (split-at n-a params)
        a (mapv vec (m/chunks-of r a-flat))
        b (mapv vec (m/chunks-of d b-flat))]
    (assoc lora :lora-a a :lora-b b)))

(defn num-lora-params
  "Number of LoRA parameters."
  [lora]
  (* 2 (:dim lora) (:rank lora)))

(defn apply-lora-generic
  "Polymorphic LoRA forward for AD (Dual numbers).
   params is a flat vector of Duals [A_flat ++ B_flat]."
  [dim rank params x]
  (let [n-a (* dim rank)
        [a-flat b-flat] (split-at n-a params)
        a (m/chunks-of rank a-flat)
        b (m/chunks-of dim b-flat)
        ;; x * A (Dual-aware mat-mul)
        tmp1 (mapv (fn [x-row]
                     (mapv (fn [a-col]
                             (ad/d-dot-product x-row a-col))
                           (apply mapv vector a)))
                   x)
        ;; tmp1 * B
        delta (mapv (fn [t-row]
                      (mapv (fn [b-col]
                              (ad/d-dot-product t-row b-col))
                            (apply mapv vector b)))
                    tmp1)]
    ;; x + delta
    (mapv (fn [x-row d-row]
            (mapv ad/d-add x-row d-row))
          x delta)))
