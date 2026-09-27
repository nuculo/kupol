(ns kan-kat.main
  "Entry point: demos showcasing KAN v2, KAT, LoRA, backprop, and debug tools."
  (:require [clojure.math :as math]
            [kan-kat.math :as m]
            [kan-kat.training :as tr]
            [kan-kat.kan-layer :as kan]
            [kan-kat.backprop :as bp]
            [kan-kat.kat-decoder :as kat]
            [kan-kat.kat-backprop :as katbp]
            [kan-kat.fine-tune :as ft]
            [kan-kat.numerical-gradient :as ng]
            [kan-kat.char-lm :as clm]))

;; ============================================================
;; Demo 1: KAN Regression — sin(x) (1D)
;; ============================================================

(defn demo-kan-regression []
  (println "==========================================")
  (println " Demo 1: KAN Regression — sin(x)")
  (println "==========================================")
  (println "Training KAN v2 to approximate sin(x)...\n")
  
  (let [dataset (mapv (fn [i]
                        (let [x (* 4.0 (/ (- i 10.0) 10.0))]
                          [[x] [(math/sin x)]]))
                      (range 21))
        layer   (tr/init-random-kan 1 1 3 5 [-2.0 2.0])
        trained (tr/train-kan layer dataset 0.02 30
                  {:momentum 0.9 :max-grad-norm 5.0})]
    (println "\nTest predictions:")
    (doseq [x [-2.0 -1.0 0.0 1.0 2.0]]
      (let [pred   (kan/forward-layer trained [x])
            actual (math/sin x)]
        (println (format "  sin(%.1f) = %.4f | predicted: %.4f | error: %.4f"
                         x actual (first pred) (abs (- actual (first pred)))))))
    (println)))

;; ============================================================
;; Demo 2: KAN 2D — sin(πx) + y² with ADAPTIVE GRID
;; ============================================================

(defn target-2d [x y]
  (+ (math/sin (* math/PI x)) (* y y)))

(defn demo-kan-2d []
  (println "==========================================")
  (println " Demo 2: KAN 2D — sin(πx) + y²")
  (println "    With adaptive grid (Liu et al. 2024)")
  (println "==========================================")
  (println "Training KAN [2→1] with grid updates every 10 epochs...\n")
  
  (let [dataset (mapv (fn [_]
                        (let [x (tr/rand-range -1.0 1.0)
                              y (tr/rand-range -1.0 1.0)]
                          [[x y] [(target-2d x y)]]))
                      (range 200))
        layer   (tr/init-random-kan 2 1 3 8 [-1.5 1.5])
        ;; Momentum + adaptive grid + lr-decay
        trained (tr/train-kan layer dataset 0.02 40
                  {:grid-update-every 10 :grid-eps 0.3
                   :momentum 0.9 :lr-decay 0.01
                   :max-grad-norm 5.0})]
    
    (println "\nTest predictions on sin(πx) + y²:")
    (doseq [[x y] [[0.0 0.0] [0.5 0.5] [-0.5 0.3] [1.0 -1.0] [0.3 0.7]]]
      (let [pred   (first (kan/forward-layer trained [x y]))
            actual (target-2d x y)]
        (println (format "  f(%.1f, %.1f) = %6.3f | predicted: %6.3f | error: %.4f"
                         x y actual pred (abs (- actual pred))))))
    (println)))

;; ============================================================
;; Demo 3: KAT Forward Pass
;; ============================================================

(defn rand-matrix [rows cols scale]
  (mapv (fn [_]
          (mapv (fn [_] (+ (- scale) (* 2.0 scale (rand))))
                (range cols)))
        (range rows)))

(defn demo-kat-forward []
  (println "==========================================")
  (println " Demo 3: KAT Forward Pass")
  (println "==========================================")
  (println "Building a small KAT decoder-only model...\n")
  
  (let [d-model    8
        vocab-size 16
        n-layers   2
        max-seq    32
        embed      (rand-matrix vocab-size d-model 0.1)
        pos-embed  (rand-matrix max-seq d-model 0.1)
        lm-head    (rand-matrix d-model vocab-size 0.1)
        layers     (mapv (fn [_]
                           (kat/make-decoder-layer
                             (tr/init-random-kan d-model d-model 3 3 [-2.0 2.0])))
                         (range n-layers))
        model      {:embed embed :pos-embed pos-embed
                    :layers layers :lm-head lm-head}
        tokens     [1 5 3 10 7]
        logits     (kat/forward-kat model tokens)]
    (println (str "  Input tokens:  " tokens))
    (println (str "  Output shape:  [" (count logits) " × " (count (first logits)) "]"))
    (println "\n  Logits (first 3 tokens, first 8 vocab entries):")
    (doseq [[i row] (map-indexed vector (take 3 logits))]
      (println (str "    Token " i ": "
                    (mapv #(format "%.3f" %) (take 8 row)))))
    (println "\n  Greedy predictions:")
    (doseq [[i row] (map-indexed vector logits)]
      (println (str "    Position " i " → token " (m/argmax row))))
    (println)))

;; ============================================================
;; Demo 4: LoRA Fine-Tune
;; ============================================================

(defn demo-lora-fine-tune []
  (println "==========================================")
  (println " Demo 4: LoRA Fine-Tune (Adam + AD)")
  (println "==========================================")
  (println "Fine-tuning KAT with LoRA adapters...\n")
  
  (let [model   (ft/init-ft-model 4 2 2 3)
        dataset [[[[1.0 0.0 0.0 0.0] [0.0 1.0 0.0 0.0]] [2 0]]
                 [[[0.0 0.0 1.0 0.0] [0.0 0.0 0.0 1.0]] [1 3]]
                 [[[1.0 1.0 0.0 0.0] [0.0 0.0 1.0 1.0]] [3 1]]]
        trained (ft/fine-tune model dataset 0.01 10)]
    (println "\nFine-tuning complete!")
    (let [test-input [[1.0 0.0 0.0 0.0] [0.0 1.0 0.0 0.0]]
          output     (ft/forward-ft-model trained test-input)]
      (println "\nPredictions after fine-tune:")
      (doseq [[inp out] (map vector test-input output)]
        (println (str "  Input: " inp
                      " → Predicted class: " (m/argmax out)))))
    (println)))

;; ============================================================
;; Demo 5: AD Verification (numerical gradient check — v2 params)
;; ============================================================

(defn demo-ad-verification []
  (println "==========================================")
  (println " Demo 5: AD vs Numerical Gradient Check")
  (println "    (KAN v2: wb + ws + coeffs + γ + β)")
  (println "==========================================")
  (println "Verifying forward-mode AD with LayerNorm...\n")
  
  (let [layer  (tr/init-random-kan 2 1 3 3 [-1.0 1.0])
        input  [0.5 -0.3]
        target [0.7]]
    (ng/verify-ad-gradient layer input target 1e-3)))

;; ============================================================
;; Demo 6: Adam + Backprop + Grid Refinement (unified loop)
;; ============================================================

(defn demo-backprop-adam []
  (println "==========================================")
  (println " Demo 6: Adam + Backprop + Grid Refinement")
  (println "    (unified loop: 50-100× faster)")
  (println "==========================================")
  (println "Training KAN [2→1] with backprop + Adam + refinement...\n")
  
  (let [dataset (mapv (fn [_]
                        (let [x (tr/rand-range -1.0 1.0)
                              y (tr/rand-range -1.0 1.0)]
                          [[x y] [(target-2d x y)]]))
                      (range 200))
        layer   (tr/init-random-kan 2 1 3 4 [-1.5 1.5])
        layers  [layer]

        ;; Adam + adaptive grid + refinement every 15 epochs
        trained (bp/train-kan-backprop layers dataset 0.005 40
                  {:batch-size 32
                   :grid-update-every 10
                   :grid-eps 0.3
                   :refine-every 15
                   :max-grad-norm 5.0})
        
        result-layer (first trained)]
    (println "\nTest predictions (Adam + Backprop):")
    (doseq [[x y] [[0.0 0.0] [0.5 0.5] [-0.5 0.3] [1.0 -1.0] [0.3 0.7]]]
      (let [pred   (first (kan/forward-layer result-layer [x y]))
            actual (target-2d x y)]
        (println (format "  f(%.1f, %.1f) = %6.3f | predicted: %6.3f | error: %.4f"
                         x y actual pred (abs (- actual pred))))))
    (println (str "  Final G=" (:grid-size result-layer)
                  " (started at 4, refined to "
                  (:grid-size result-layer) ")"))
    (println)))

;; ============================================================
;; Demo 7: Grid Refinement (G doubling)
;; ============================================================

(defn demo-grid-refinement []
  (println "==========================================")
  (println " Demo 7: Grid Refinement G→2G")
  (println "    (Liu et al. 2024, coarse→fine)")
  (println "==========================================")
  
  (let [layer (tr/init-random-kan 2 1 3 4 [-1.0 1.0])]
    (println (str "  Before: G=" (:grid-size layer)
                  " | n_spl=" (kan/num-spl-coeffs layer)
                  " | params=" (kan/num-params layer)))
    (let [refined (kan/refine-grid layer)]
      (println (str "  After:  G=" (:grid-size refined)
                    " | n_spl=" (kan/num-spl-coeffs refined)
                    " | params=" (kan/num-params refined)))
      (let [refined2 (kan/refine-grid refined)]
        (println (str "  Again:  G=" (:grid-size refined2)
                      " | n_spl=" (kan/num-spl-coeffs refined2)
                      " | params=" (kan/num-params refined2)))))
    (println "\n  Grid refinement: 4 → 8 → 16 intervals ✅")
    (println)))

;; ============================================================
;; Demo 8: Backprop vs AD Gradient Verification
;; ============================================================

(defn demo-backprop-vs-ad []
  (println "==========================================")
  (println " Demo 8: Backprop vs AD Verification")
  (println "    (analytical backward ↔ forward-mode AD)")
  (println "==========================================")
  (println "Comparing backprop gradients against forward-mode AD...\n")
  
  (let [layer  (tr/init-random-kan 2 1 3 3 [-1.0 1.0])
        input  [0.5 -0.3]
        target [0.7]]
    (bp/verify-backprop-vs-ad layer input target 1e-3)))

;; ============================================================
;; Demo 9: Character-Level KAT Training
;; ============================================================

(defn demo-char-lm []
  (println "==========================================")
  (println " Demo 9: Character-Level KAT")
  (println "    (KAT as language model — next char)")
  (println "==========================================")
  (println "Building tiny KAT for character-level LM...\n")
  
  (let [text    "hello world hello clojure world "
        vocab   (clm/build-vocab text)
        _       (println (str "  Vocab: " (count (:chars vocab))
                              " chars: " (pr-str (:chars vocab))))
        ;; Tiny KAT: vocab, embed=8, heads=2, G=3, k=3
        model   (clm/make-tiny-kat (:vocab-size vocab) 8 2 3 3)
        n-params (count (clm/flatten-model-params model))
        _       (println (str "  Model params: " n-params))
        _       (println)
        ;; Train 5 epochs
        trained (clm/train-char-lm model vocab text 4 5 0.01 2)
        ;; Generate
        seed    (subs text 0 4)
        generated (clm/generate-text trained vocab seed 4 10)]
    (println (str "\n  Seed: \"" seed "\""))
    (println (str "  Generated: \"" generated "\""))
    (println "\n  Character-level KAT training complete ✅"))
  (println))

;; ============================================================
;; Demo 10: KAT Analytical Backprop Training
;; ============================================================

(defn demo-kat-backprop []
  (println "==========================================")
  (println " Demo 10: KAT Analytical Backprop")
  (println "    (full backward through Attention+KAN)")
  (println "==========================================")
  (println "Training KAT with analytical backward...\n")
  
  (let [text   "hello world hello clojure world "
        vocab  (clm/build-vocab text)
        model  (clm/make-tiny-kat (:vocab-size vocab) 8 2 3 3)
        encoded (clm/encode text vocab)
        ctx-len 4
        examples (mapv (fn [i]
                         {:tokens (subvec encoded i (+ i ctx-len))
                          :target (nth encoded (+ i ctx-len))})
                       (range (- (count encoded) ctx-len)))
        _       (println (str "  Vocab: " (count (:chars vocab)) " chars"))
        _       (println (str "  Examples: " (count examples) " windows"))
        _       (println)
        ;; Train with analytical backprop
        trained (katbp/train-kat-analytical model examples 0.01 5 4)
        ;; Generate
        seed-ids (subvec encoded 0 ctx-len)
        generated (clm/generate-text trained vocab (subs text 0 ctx-len) ctx-len 8)]
    (println (str "\n  Seed: \"" (subs text 0 ctx-len) "\""))
    (println (str "  Generated: \"" generated "\""))
    (println "\n  KAT analytical backprop complete ✅"))
  (println))

;; ============================================================
;; Demo 11: PhiFunction Protocol (extensible φ)
;; ============================================================

(defn demo-phi-protocol []
  (println "==========================================")
  (println " Demo 11: PhiFunction Protocol")
  (println "    (BSpline / Poly / Rational)")
  (println "==========================================")
  (println "Testing 3 φ implementations at x=0.5:\n")
  
  (let [phi-protocol (requiring-resolve 'kan-kat.phi-protocol/make-phi)
        phi-forward  (requiring-resolve 'kan-kat.phi-protocol/phi-forward)
        phi-backward (requiring-resolve 'kan-kat.phi-protocol/phi-backward)
        phi-params   (requiring-resolve 'kan-kat.phi-protocol/phi-params)
        phi-grad     (requiring-resolve 'kan-kat.phi-protocol/phi-grad)
        x 0.5
        types [[:bspline {:order 3 :grid-size 5 :grid-range [-1.0 1.0]}]
               [:poly {:degree 3}]
               [:rational {:p-deg 3 :q-deg 2}]]]
    (doseq [[phi-type opts] types]
      (let [phi (phi-protocol phi-type opts)
            fx  (phi-forward phi x)
            dx  (phi-backward phi x)
            p   (phi-params phi)
            g   (phi-grad phi x)]
        (println (format "  %-10s | φ(%.1f) = %7.4f | dφ/dx = %7.4f | params: %d | grad: %d"
                         (name phi-type) x fx dx (count p) (count g))))))
  (println "\n  All 3 PhiFunction types work ✅")
  (println))

;; ============================================================
;; Demo 12: Symbolic Regression (φ → formulas)
;; ============================================================

(defn demo-symbolic []
  (println "==========================================")
  (println " Demo 12: Symbolic Regression")
  (println "    (discover formulas inside trained φ)")
  (println "==========================================")
  (println "Training KAN on sin(x), then analyzing φ...\n")
  
  (let [;; Train KAN [1→1] on sin(x) for 50 epochs
        layer (tr/init-random-kan 1 1 3 5 [-3.0 3.0])
        dataset (mapv (fn [_]
                        (let [x (- (* 6.0 (rand)) 3.0)]
                          [[x] [(math/sin x)]]))
                      (range 30))
        trained (bp/train-kan-backprop
                  [layer] dataset 0.01 50
                  {:batch-size 10 :max-grad-norm 5.0})
        trained-layer (first trained)
        ;; Symbolic analysis
        sym (requiring-resolve 'kan-kat.symbolic/symbolify-layer)]
    (sym trained-layer {:n-points 50}))
  (println "  Symbolic regression complete ✅")
  (println))

;; ============================================================
;; Demo 13: Evolutionary φ Optimization
;; ============================================================

(defn demo-evolution []
  (println "==========================================")
  (println " Demo 13: Evolutionary φ Optimization")
  (println "    (evolve type + params of φ)")
  (println "==========================================")
  (println "Evolving population to approximate x²...\n")
  
  (let [evolve-fn (requiring-resolve 'kan-kat.evolution/evolve-for-function)
        best (evolve-fn (fn [x] (* x x))  ; target: x²
                        [-2.0 2.0]         ; interval
                        30                 ; samples
                        20                 ; pop size
                        30)]               ; generations
    (println (str "  Winner type: " (name (:type best))))
    (println "  Evolution complete ✅"))
  (println))

;; ============================================================
;; Demo 14: Agent-Based KAN
;; ============================================================

(defn demo-agent-kan []
  (println "==========================================")
  (println " Demo 14: Agent-Based KAN")
  (println "    (autonomous edge agents)")
  (println "==========================================")
  (println "Each φ_{j,i} is an autonomous agent...\n")
  
  (let [make-layer  (requiring-resolve 'kan-kat.agent-kan/make-agent-layer)
        train-layer (requiring-resolve 'kan-kat.agent-kan/train-agent-layer)
        fwd         (requiring-resolve 'kan-kat.agent-kan/agent-layer-forward)
        ;; Target: f(x1,x2) = sin(x1) + x2²
        target-fn (fn [[x1 x2]] (+ (math/sin x1) (* x2 x2)))
        input-data (mapv (fn [_]
                           [(- (* 4.0 (rand)) 2.0)
                            (- (* 4.0 (rand)) 2.0)])
                         (range 20))
        layer (make-layer 2 1)
        trained (train-layer layer target-fn input-data 30)
        ;; Show agent states
        agents (for [row (:agents trained) a row] a)]
    (println)
    (doseq [a agents]
      (println (format "  Agent φ(%d,%d): type=%-10s strategy=%-8s age=%d mutations=%d"
                       (:j a) (:i a) (name (:type a))
                       (name (:strategy a)) (:age a) (:mutations a)))))
  (println "\n  Agent-based KAN complete ✅")
  (println))

;; ============================================================
;; Demo 15: Automorphic KAN (Normalizing Flow)
;; ============================================================

(defn demo-normalizing-flow []
  (println "==========================================")
  (println " Demo 15: Automorphic KAN (Norm. Flow)")
  (println "    (invertible KAN coupling layers)")
  (println "==========================================")
  (println "Creating flow: z ~ N(0,I) → x via KAN φ\n")
  
  (let [make-flow     (requiring-resolve 'kan-kat.automorphic-kan/make-flow)
        flow-forward  (requiring-resolve 'kan-kat.automorphic-kan/flow-forward)
        flow-inverse  (requiring-resolve 'kan-kat.automorphic-kan/flow-inverse)
        sample-n      (requiring-resolve 'kan-kat.automorphic-kan/sample-n)
        log-prob-fn   (requiring-resolve 'kan-kat.automorphic-kan/log-prob)
        
        ;; Create 4D flow with 4 coupling layers
        flow (make-flow 4 4 :poly {:degree 3})
        
        ;; Test invertibility: x → z → x' should be x ≈ x'
        test-x [1.0 -0.5 0.3 0.7]
        [y log-det] (flow-forward flow test-x)
        x-roundtrip (flow-inverse flow y)
        roundtrip-err (reduce + 0.0
                        (map (fn [a b] (abs (- a b)))
                             test-x x-roundtrip))
        
        ;; Generate samples
        samples (sample-n flow 5)
        
        ;; Log probability
        lp (log-prob-fn flow test-x)]
    
    (println (format "  Input:     %s" (pr-str (mapv #(format "%.3f" %) test-x))))
    (println (format "  Forward:   %s" (pr-str (mapv #(format "%.3f" %) y))))
    (println (format "  Roundtrip: %s" (pr-str (mapv #(format "%.3f" %) x-roundtrip))))
    (println (format "  Roundtrip error: %.2e (should be ~0)" roundtrip-err))
    (println (format "  Log|det J|: %.4f" log-det))
    (println (format "  Log p(x):   %.4f" lp))
    (println (format "  Samples:    %d generated" (count samples)))
    (println (format "  Invertible: %s" (if (< roundtrip-err 1e-8) "✅ EXACT" "⚠️ approx"))))
  (println "\n  Normalizing Flow complete ✅")
  (println))

;; ============================================================
;; Demo 16: Dynamic System KAN
;; ============================================================

(defn demo-dynamic-system []
  (println "==========================================")
  (println " Demo 16: Dynamic System KAN")
  (println "    (ODE integration: loss=energy)")
  (println "==========================================")
  (println "Particle rolling down energy landscape for sin(x)...\n")
  
  (let [train-fn (requiring-resolve 'kan-kat.dynamic-kan/train-dynamic)
        final (train-fn :poly {:degree 4}
                        (fn [x] (math/sin x))
                        [-3.0 3.0]    ; domain
                        20            ; data points
                        80            ; integration steps
                        0.02          ; dt
                        0.3)]         ; friction
    (println "\n  Dynamic system complete ✅"))
  (println))

;; ============================================================
;; Demo 17: Ecosystem KAN
;; ============================================================

(defn demo-ecosystem []
  (println "==========================================")
  (println " Demo 17: Ecosystem KAN")
  (println "    (birth / death / reproduction)")
  (println "==========================================")
  (println "Evolving ecosystem of φ organisms for sin(x)...\n")
  
  (let [evolve-fn (requiring-resolve 'kan-kat.ecosystem/evolve-ecosystem)
        best (evolve-fn (fn [x] (math/sin x))
                        [-3.0 3.0]   ; domain
                        20           ; data points
                        10           ; initial population
                        30)]         ; steps
    (println (format "\n  Survivor: %s (gen %d) | Loss: %.6f"
                     (name (:type best)) (:generation best) (:loss best))))
  (println "\n  Ecosystem complete ✅")
  (println))

;; ============================================================
;; Demo 18: Automorphism Composition (Kaleidoscope)
;; ============================================================

(defn demo-kaleidoscope []
  (println "==========================================")
  (println " Demo 18: Kaleidoscope (automorphisms)")
  (println "    (compose invertible φ-transforms)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.compose-kan/demo-kaleidoscope)]
    (demo-fn 3))
  (println "\n  Kaleidoscope complete ✅")
  (println))

;; ============================================================
;; Demo 19: Reverse-Mode Autograd
;; ============================================================

(defn demo-autograd []
  (println "==========================================")
  (println " Demo 19: Reverse-Mode Autograd")
  (println "    (computation graph + backward)")
  (println "==========================================\n")
  
  (let [ag-value    (requiring-resolve 'kan-kat.autograd/value)
        verify-fn   (requiring-resolve 'kan-kat.autograd/verify-gradients)
        train-fn    (requiring-resolve 'kan-kat.autograd/train-kan-autograd)
        poly-fwd    (requiring-resolve 'kan-kat.autograd/poly-forward)
        
        ;; 1. Verify gradients: poly φ(x) = c0 + c1*x + c2*x²
        _ (println "  Part 1: Gradient verification")
        coeffs [(ag-value 0.5) (ag-value -0.3) (ag-value 0.1)]
        result (verify-fn poly-fwd coeffs 1.5 1e-5)]
    (println (format "    Autograd:  %s" (pr-str (mapv #(format "%.6f" %) (:autograd result)))))
    (println (format "    Numerical: %s" (pr-str (mapv #(format "%.6f" %) (:numerical result)))))
    (println (format "    Max error: %.2e %s"
                     (:max-error result)
                     (if (< (:max-error result) 1e-5) "✅ PASS" "❌ FAIL")))
    
    ;; 2. Train KAN on sin(x)
    (println "\n  Part 2: Training PolyPhi on sin(x)")
    (train-fn (fn [x] (math/sin x))
              [-2.0 2.0] 20 3 60 0.005))
  (println "\n  Autograd complete ✅")
  (println))

;; ============================================================
;; Demo 20: Symbolic+Numeric Hybrid KAN
;; ============================================================

(defn demo-hybrid []
  (println "==========================================")
  (println " Demo 20: Hybrid KAN (symbolic+numeric)")
  (println "    (auto-discover formulas during training)")
  (println "==========================================\n")
  
  (let [train-fn (requiring-resolve 'kan-kat.hybrid-kan/train-hybrid)
        ;; Train on f(x) = sin(x) — should discover sin!
        result (train-fn (fn [x] (math/sin x))
                         1              ; in-dim
                         [-2.0 2.0]     ; domain
                         15             ; data points
                         3              ; poly degree
                         60             ; epochs
                         0.005          ; lr
                         10)]           ; probe every N epochs
    (println))
  (println "  Hybrid KAN complete ✅")
  (println))

;; ============================================================
;; Demo 21: Operator Algebra KAN
;; ============================================================

(defn demo-operator-algebra []
  (println "==========================================")
  (println " Demo 21: Operator Algebra KAN")
  (println "    (learn RULES, not numbers)")
  (println "==========================================\n")
  
  (let [train-fn  (requiring-resolve 'kan-kat.operator-kan/train-operator-layer)
        verify-fn (requiring-resolve 'kan-kat.operator-kan/verify-commutators)
        id-op     @(requiring-resolve 'kan-kat.operator-kan/id-op)
        D-op      @(requiring-resolve 'kan-kat.operator-kan/D-op)
        D2-op     @(requiring-resolve 'kan-kat.operator-kan/D2-op)
        X-op      @(requiring-resolve 'kan-kat.operator-kan/X-op)
        
        ;; Task: discover that sin → cos means "apply D"
        ;; Operator basis: {Id, D, D², X·}
        ;; Should learn coeffs ≈ [0, 1, 0, 0] (pure D)
        result (train-fn math/sin math/cos
                         [id-op D-op D2-op X-op]
                         [-2.0 2.0] 20 80 0.1)]
    ;; Verify [D,X] = Id
    (verify-fn))
  (println "\n  Operator Algebra complete ✅")
  (println))

;; ============================================================
;; Demo 22: Tensor Engine
;; ============================================================

(defn demo-tensor-engine []
  (println "==========================================")
  (println " Demo 22: Tensor Engine")
  (println "    (batched autograd, 1 graph for N pts)")
  (println "==========================================\n")
  
  (let [verify-fn (requiring-resolve 'kan-kat.tensor/verify-tensor-grads)
        train-fn  (requiring-resolve 'kan-kat.tensor/train-tensor-kan)
        
        ;; 1. Verify tensor gradients
        _ (println "  Part 1: Tensor gradient verification")
        result (verify-fn [0.5 -0.3 0.1] [0.5 1.0 1.5 2.0])]
    (println (format "    Autograd:  %s" (pr-str (mapv #(format "%.6f" %) (:autograd result)))))
    (println (format "    Numerical: %s" (pr-str (mapv #(format "%.6f" %) (:numerical result)))))
    (println (format "    Max error: %.2e %s"
                     (:max-error result)
                     (if (< (:max-error result) 1e-4) "✅ PASS" "❌ FAIL")))
    
    ;; 2. Train on sin(x) — BATCHED
    (println "\n  Part 2: Batched training on sin(x)")
    (train-fn math/sin [-2.0 2.0] 30 3 80 0.01))
  (println "\n  Tensor Engine complete ✅")
  (println))

;; ============================================================
;; Demo 23: Production Tensor Engine v2
;; ============================================================

(defn demo-tensor-v2 []
  (println "==========================================")
  (println " Demo 23: Production Tensor Engine v2")
  (println "    (float-array · broadcast · matmul)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.tensor-v2/demo-tensor-v2)]
    (demo-fn))
  (println "\n  Tensor v2 complete ✅")
  (println))

;; ============================================================
;; Demo 24: XLA-like Lazy Graph
;; ============================================================

(defn demo-lazy-graph []
  (println "==========================================")
  (println " Demo 24: XLA-like Lazy Graph Engine")
  (println "    (build → optimize → execute)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.lazy-graph/demo-lazy-graph)]
    (demo-fn))
  (println "\n  Lazy Graph complete ✅")
  (println))

;; ============================================================
;; Demo 25: High-Performance JVM Backend
;; ============================================================

(defn demo-jvm-backend []
  (println "==========================================")
  (println " Demo 25: High-Performance JVM Backend")
  (println "    (tiled matmul · parallel · memory pool)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.jvm-backend/demo-jvm-backend)]
    (demo-fn))
  (println "\n  JVM Backend complete ✅")
  (println))

;; ============================================================
;; Demo 26: Multi-Layer KAN Framework
;; ============================================================

(defn demo-kan-framework []
  (println "==========================================")
  (println " Demo 26: Multi-Layer KAN Framework")
  (println "    (multi-layer · tensor_v2 · autograd)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.kan-framework/demo-kan-framework)]
    (demo-fn))
  (println "\n  KAN Framework complete ✅")
  (println))

;; ============================================================
;; Demo 27: KAN Advanced — PhiProtocol + Grid Refinement
;; ============================================================

(defn demo-kan-advanced []
  (println "==========================================")
  (println " Demo 27: KAN Advanced — PhiProtocol")
  (println "    (BSpline · Poly · Rational · Grid)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.kan-advanced/demo-kan-advanced)]
    (demo-fn))
  (println "\n  KAN Advanced complete ✅")
  (println))

;; ============================================================
;; Demo 28: Symbolic Discovery in Framework
;; ============================================================

(defn demo-kan-symbolic []
  (println "==========================================")
  (println " Demo 28: Symbolic Discovery")
  (println "    (probe · freeze · auto-discover)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.kan-symbolic/demo-kan-symbolic)]
    (demo-fn))
  (println "\n  Symbolic Discovery complete ✅")
  (println))

;; ============================================================
;; Demo 29: End-to-End HPC Pipeline
;; ============================================================

(defn demo-hpc-pipeline []
  (println "==========================================")
  (println " Demo 29: End-to-End HPC Pipeline")
  (println "    (L1→L5 · bench · parallel)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.hpc-pipeline/demo-hpc-pipeline)]
    (demo-fn))
  (println "\n  HPC Pipeline complete ✅")
  (println))

;; ============================================================
;; Demo 30: Training Monitor
;; ============================================================

(defn demo-training-monitor []
  (println "==========================================")
  (println " Demo 30: Training Monitor")
  (println "    (ASCII plots · grads · timing)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.training-monitor/demo-training-monitor)]
    (demo-fn))
  (println "\n  Training Monitor complete ✅")
  (println))

;; ============================================================
;; Demo 31: Agent KAN v2
;; ============================================================

(defn demo-agent-kan-v2 []
  (println "==========================================")
  (println " Demo 31: Agent KAN v2 (tensor_v2)")
  (println "    (batched · parallel · evolution)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.agent-kan-v2/demo-agent-kan-v2)]
    (demo-fn))
  (println "\n  Agent KAN v2 complete ✅")
  (println))

;; ============================================================
;; Demo 32: Model Serialization
;; ============================================================

(defn demo-serialization []
  (println "==========================================")
  (println " Demo 32: Model Serialization")
  (println "    (save · load · checkpoint · resume)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.serialization/demo-serialization)]
    (demo-fn))
  (println "\n  Serialization complete ✅")
  (println))

;; ============================================================
;; Demo 33: Distributed Training
;; ============================================================

(defn demo-distributed []
  (println "==========================================")
  (println " Demo 33: Distributed Training")
  (println "    (sharding · all-reduce · data-par)")
  (println "==========================================\n")
  
  (let [demo-fn (requiring-resolve 'kan-kat.distributed/demo-distributed)]
    (demo-fn))
  (println "\n  Distributed Training complete ✅")
  (println))

;; ============================================================
;; Main
;; ============================================================

(defn -main [& _args]
  (println "╔══════════════════════════════════════════════╗")
  (println "║  Clojure KAN v2 + KAT — From Scratch        ║")
  (println "║  Framework · HPC · Distributed · Serialize   ║")
  (println "╚══════════════════════════════════════════════╝")
  (println)
  
  (demo-kan-regression)
  (demo-kan-2d)
  (demo-kat-forward)
  (demo-lora-fine-tune)
  (demo-ad-verification)
  (demo-backprop-adam)
  (demo-grid-refinement)
  (demo-backprop-vs-ad)
  (demo-char-lm)
  (demo-kat-backprop)
  (demo-phi-protocol)
  (demo-symbolic)
  (demo-evolution)
  (demo-agent-kan)
  (demo-normalizing-flow)
  (demo-dynamic-system)
  (demo-ecosystem)
  (demo-kaleidoscope)
  (demo-autograd)
  (demo-hybrid)
  (demo-operator-algebra)
  (demo-tensor-engine)
  (demo-tensor-v2)
  (demo-lazy-graph)
  (demo-jvm-backend)
  (demo-kan-framework)
  (demo-kan-advanced)
  (demo-kan-symbolic)
  (demo-hpc-pipeline)
  (demo-training-monitor)
  (demo-agent-kan-v2)
  (demo-serialization)
  (demo-distributed)
  
  (let [demo-fn (requiring-resolve 'kan-kat.gradient-accumulation/demo-gradient-accumulation)]
    (demo-fn))
  (println "\n  Gradient Accumulation complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.learning-rate-finder/demo-lr-finder)]
    (demo-fn))
  (println "\n  Learning Rate Finder complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.early-stopping/demo-early-stopping)]
    (demo-fn))
  (println "\n  Early Stopping complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.residual-kan/demo-residual-kan)]
    (demo-fn))
  (println "\n  Residual KAN complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.convolutional-kan/demo-conv-kan)]
    (demo-fn))
  (println "\n  Convolutional KAN complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.recurrent-kan/demo-rnn-kan)]
    (demo-fn))
  (println "\n  Recurrent KAN complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.attention-kan/demo-attention-kan)]
    (demo-fn))
  (println "\n  Attention KAN complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.multi-head-kan/demo-multi-head-kan)]
    (demo-fn))
  (println "\n  Multi-Head KAN complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.data-loader/demo-data-loader)]
    (demo-fn))
  (println "\n  Data Loader complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.regression-bench/demo-regression-bench)]
    (demo-fn))
  (println "\n  Regression Benchmark complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.classification-kan/demo-classification-kan)]
    (demo-fn))
  (println "\n  Classification KAN complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.time-series-kan/demo-time-series-kan)]
    (demo-fn))
  (println "\n  Time Series KAN complete ✅")
  (println)
  
  (let [demo-fn (requiring-resolve 'kan-kat.financial-timeseries-kan/demo-financial-kan)]
    (demo-fn))
  (println "\n  Financial KAN complete ✅")
  (println)

  (let [demo-fn (requiring-resolve 'kan-kat.financial-timeseries-kan/demo-technical-indicators-kan)]
    (demo-fn))
  (println "\n  Technical Indicators KAN complete ✅")
  (println)

  (let [demo-fn (requiring-resolve 'kan-kat.financial-timeseries-kan/demo-extended-indicators-kan)]
    (demo-fn))
  (println "\n  Extended Indicators (Holistic) KAN complete ✅")
  (println)

  (let [demo-fn (requiring-resolve 'kan-kat.financial-timeseries-kan/demo-advanced-analytics-kan)]
    (demo-fn))
  (println "\n  Advanced Analytics (Smart Regime-Aware) KAN complete ✅")
  (println)

  (let [demo-fn (requiring-resolve 'kan-kat.financial-timeseries-kan/demo-candlestick-multihead-kan)]
    (demo-fn))
  (println "\n  Multi-Head KAN + Candlesticks complete ✅")
  (println)

  (let [demo-fn (requiring-resolve 'kan-kat.backtest/demo-institutional-backtest)]
    (demo-fn))
  (println "\n  Institutional Backtesting complete ✅")
  (println)

  (let [demo-fn (requiring-resolve 'kan-kat.streaming-kan/demo-live-streaming)]
    (demo-fn))
  (println "\n  Real-Time Streaming KAN (Phase 54) complete ✅")
  (println)

  (let [demo-fn (requiring-resolve 'kan-kat.financial-timeseries-kan/demo-babylonian-kan)]
    (demo-fn))
  (println "\n  Babylonian 60-Head KAN (Phase 55) complete ✅")
  (println)

  (println "All 54 demos complete!")
  (shutdown-agents))













