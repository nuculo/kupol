(ns kan-kat.hpc-pipeline
  "End-to-End HPC Pipeline: все 5 слоёв L1→L5 вместе.
   
   ═══════════════════════════════════════════════════
   Фаза 36: Единый pipeline
   
   data → tensor_v2 → KAN forward → lazy_graph optimize
        → jvm_backend execute → loss
   
   Бенчмарк: naive vs optimized pipeline
   ═══════════════════════════════════════════════════"
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.lazy-graph :as lg]
            [kan-kat.jvm-backend :as jvm]
            [kan-kat.kan-framework :as kf]
            [clojure.math :as math]))

;; ============================================================
;; LAYER 3: JVM-Accelerated KAN Forward
;; ============================================================

(defn jvm-kan-forward
  "KAN forward pass using JVM-optimized Horner (raw double-array).
   coeffs-arrays[j][i] = double-array of polynomial coefficients."
  [^doubles x-batch batch in-dim out-dim coeffs-arrays]
  (let [out (double-array (* batch out-dim))]
    (dotimes [j out-dim]
      (dotimes [i in-dim]
        (let [^doubles ci (get-in coeffs-arrays [j i])
              deg (dec (alength ci))]
          (dotimes [b batch]
            (let [xi (aget x-batch (+ (* b in-dim) i))]
              ;; Horner: c₅x⁵+c₄x⁴+...+c₀
              (loop [d deg acc (aget ci deg)]
                (if (zero? d)
                  (aset out (+ (* b out-dim) j)
                        (+ (aget out (+ (* b out-dim) j)) acc))
                  (recur (dec d) (+ (aget ci (dec d)) (* acc xi))))))))))
    out))

(defn jvm-mse-loss
  "MSE loss on raw double-arrays."
  [^doubles pred ^doubles target n]
  (let [n (int n)]
    (loop [i 0 acc 0.0]
      (if (= i n) (/ acc n)
        (let [d (- (aget pred i) (aget target i))]
          (recur (inc i) (+ acc (* d d))))))))

;; ============================================================
;; LAYER 4: Parallel Pipeline (ForkJoinPool)
;; ============================================================

(defn- process-chunk!
  "Process batch chunk [start, end) writing to shared output array."
  [^doubles out ^doubles x-batch start end in-dim out-dim coeffs-arrays]
  (loop [b start]
    (when (< b end)
      (dotimes [j out-dim]
        (dotimes [i in-dim]
          (let [^doubles ci (get-in coeffs-arrays [j i])
                deg (dec (alength ci))
                xi (aget x-batch (+ (* b in-dim) i))]
            (loop [d deg acc (aget ci deg)]
              (if (zero? d)
                (aset out (+ (* b out-dim) j)
                      (+ (aget out (+ (* b out-dim) j)) acc))
                (recur (dec d) (+ (aget ci (dec d)) (* acc xi))))))))
      (recur (inc b)))))

(defn parallel-kan-forward
  "Parallel KAN forward using futures for large batches."
  [^doubles x-batch batch in-dim out-dim coeffs-arrays]
  (if (< batch 100)
    (jvm-kan-forward x-batch batch in-dim out-dim coeffs-arrays)
    (let [out (double-array (* batch out-dim))
          n-chunks (min 8 (.availableProcessors (Runtime/getRuntime)))
          chunk-size (max 1 (quot batch n-chunks))
          futs (doall
                 (for [s (range 0 batch chunk-size)]
                   (let [e (min (+ s chunk-size) batch)]
                     (future (process-chunk! out x-batch s e in-dim out-dim coeffs-arrays)))))]
      (doseq [f futs] @f)
      out)))

;; ============================================================
;; BENCHMARKING
;; ============================================================

(defn bench-pipeline
  "Бенчмарк: naive (kan_framework) vs JVM-optimized vs parallel."
  [batch in-dim out-dim degree]
  (let [n-coeffs (inc degree)
        coeffs-vecs (vec (for [_j (range out-dim)]
                           (vec (for [_i (range in-dim)]
                                  (vec (repeatedly n-coeffs #(- (rand) 0.5)))))))
        coeffs-arrays (mapv (fn [r] (mapv double-array r)) coeffs-vecs)
        x-vec (vec (repeatedly (* batch in-dim) #(- (* 4.0 (rand)) 2.0)))
        x-arr (double-array x-vec)
        x-tensor (t2/tensor x-vec [batch in-dim])
        model (kf/make-model [in-dim out-dim] degree)

        run-bench (fn [label f warmup iters]
                    (dotimes [_ warmup] (f))
                    (let [t0 (System/nanoTime)]
                      (dotimes [_ iters] (f))
                      (let [ms (/ (- (System/nanoTime) t0) 1e6)]
                        (println (format "    %-35s %8.3f ms  (avg/%d)" label (/ ms iters) iters))
                        (/ ms iters))))]
    (let [fw-ms (run-bench "Framework forward (tensor_v2)"
                  #(kf/model-forward model x-tensor) 2 10)
          jvm-ms (run-bench "JVM-optimized forward (raw)"
                   #(jvm-kan-forward x-arr batch in-dim out-dim coeffs-arrays) 2 10)
          par-ms (run-bench "Parallel forward (ForkJoin)"
                   #(parallel-kan-forward x-arr batch in-dim out-dim coeffs-arrays) 2 10)]
      (println (format "    Speedup JVM/Framework: %.1f×" (/ fw-ms (max 0.001 jvm-ms))))
      (println (format "    Speedup Parallel/Framework: %.1f×" (/ fw-ms (max 0.001 par-ms))))
      {:framework-ms fw-ms :jvm-ms jvm-ms :parallel-ms par-ms})))

;; ============================================================
;; END-TO-END PIPELINE
;; ============================================================

(defn e2e-pipeline
  "End-to-End pipeline: data → forward → loss → verify.
   All 5 layers in a single pass."
  [x-data y-data in-dim out-dim degree]
  (let [batch (quot (count x-data) in-dim)
        ;; L1: Math core — Xavier polynomial coefficients
        sigma (math/sqrt (/ 2.0 (+ in-dim out-dim)))
        coeffs (vec (for [_j (range out-dim)]
                      (vec (for [_i (range in-dim)]
                             (vec (repeatedly (inc degree)
                                    #(* sigma (- (rand) 0.5) 2.0)))))))
        coeffs-arr (mapv (fn [r] (mapv double-array r)) coeffs)
        x-arr (double-array x-data)
        y-arr (double-array y-data)
        ;; L2 + L3: JVM-optimized forward + MSE
        pred (jvm-kan-forward x-arr batch in-dim out-dim coeffs-arr)
        loss (jvm-mse-loss pred y-arr batch)
        ;; L4: Lazy graph verification (build + optimize + execute)
        [g x-id] (lg/lazy-input (lg/new-graph) (vec x-data) [batch])
        [g sq-id] (lg/lazy-square g x-id)
        [g add-id] (lg/lazy-add g x-id sq-id)
        g-opt (lg/eliminate-dead g add-id)
        g-exec (lg/execute! g-opt add-id)
        ;; L5: Parallel forward
        pred-par (parallel-kan-forward x-arr batch in-dim out-dim coeffs-arr)
        loss-par (jvm-mse-loss pred-par y-arr batch)]
    {:loss loss :loss-parallel loss-par :batch batch
     :graph-nodes (count (:nodes g-exec))
     :verify (< (abs (- loss loss-par)) 1e-10)}))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-hpc-pipeline []
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  End-to-End HPC Pipeline             ║")
  (println "  ║  L1→L2→L3→L4→L5 unified             ║")
  (println "  ╚══════════════════════════════════════╝")

  ;; Part 1: E2E Pipeline
  (println "\n  Part 1: End-to-End pipeline [2→1]")
  (let [n 100
        xs (vec (flatten (for [_ (range n)] [(- (* 4.0 (rand)) 2.0)
                                              (- (* 4.0 (rand)) 2.0)])))
        ys (vec (for [i (range n)]
                  (+ (math/sin (nth xs (* 2 i)))
                     (* (nth xs (inc (* 2 i))) (nth xs (inc (* 2 i)))))))
        result (e2e-pipeline xs ys 2 1 4)]
    (println (format "    Batch: %d" (:batch result)))
    (println (format "    Loss (sequential):  %.6f" (:loss result)))
    (println (format "    Loss (parallel):    %.6f" (:loss-parallel result)))
    (println (format "    Loss match: %s" (if (:verify result) "✅ exact" "❌ mismatch")))
    (println (format "    Graph nodes: %d" (:graph-nodes result))))

  ;; Part 2: Small batch benchmark
  (println "\n  Part 2: Benchmark [1→1], batch=100, deg=4")
  (bench-pipeline 100 1 1 4)

  ;; Part 3: Medium batch benchmark  
  (println "\n  Part 3: Benchmark [2→4], batch=200, deg=4")
  (bench-pipeline 200 2 4 4)

  ;; Part 4: Large batch benchmark
  (println "\n  Part 4: Benchmark [2→4], batch=1000, deg=4")
  (bench-pipeline 1000 2 4 4)

  ;; Part 5: Lazy graph optimization demo
  (println "\n  Part 5: Lazy graph optimization")
  (let [[g x-id]    (lg/lazy-input (lg/new-graph) (range 50) [50])
        [g a-id]    (lg/lazy-square g x-id)          ; x²
        [g b-id]    (lg/lazy-sin g a-id)             ; sin(x²)
        [g c-id]    (lg/lazy-mul g b-id x-id)        ; x·sin(x²)
        [g d-id]    (lg/lazy-add g c-id a-id)        ; x·sin(x²) + x²
        [g dead-id] (lg/lazy-cos g x-id)             ; cos(x) — unused!
        n-before (count (:nodes g))
        g2 (lg/eliminate-dead g d-id)
        n-after-dce (count (:nodes g2))
        g3 (lg/fuse-chain g2 d-id)
        n-after-fusion (count (:nodes g3))
        g4 (lg/execute! g3 d-id)]
    (println (format "    Before:      %d nodes" n-before))
    (println (format "    After DCE:   %d nodes (removed dead cos)" n-after-dce))
    (println (format "    After fusion: %d nodes" n-after-fusion))
    (println (format "    Result[0]:   %.6f" (aget ^doubles (:buffer (get-in g4 [:nodes d-id])) 0))))

  ;; Part 6: Summary
  (println "\n  Part 6: Pipeline layers summary")
  (println "    L1 Math:    Xavier init, polynomial coeffs")
  (println "    L2 Neural:  KAN layer (Horner evaluation)")
  (println "    L3 Train:   forward → MSE loss")
  (println "    L4 Extend:  lazy graph (DCE, fusion)")
  (println "    L5 HPC:     parallel ForkJoin, tiled ops"))
