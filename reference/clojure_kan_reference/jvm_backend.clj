(ns kan-kat.jvm-backend
  "High-Performance JVM Backend для KAN Tensor Engine.
   
   ═══════════════════════════════════════════════════
   ИДЕЯ: ВЫЖИМАЕМ МАКСИМУМ ИЗ JVM
   ═══════════════════════════════════════════════════
   
   1. DirectByteBuffer — off-heap, нет GC pressure
   2. Tiled MatMul — cache-friendly блочное умножение
   3. Parallel ops — ForkJoinPool для elementwise
   4. SIMD-friendly loops — auto-vectorization
   5. Memory pool — reuse буферов
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math])
  (:import [java.nio ByteBuffer ByteOrder DoubleBuffer]
           [java.util.concurrent ForkJoinPool ForkJoinTask RecursiveAction]))

;; ============================================================
;; DIRECT MEMORY TENSOR (off-heap)
;; ============================================================

(defn alloc-direct ^DoubleBuffer [n]
  (-> (ByteBuffer/allocateDirect (* (int n) 8))
      (.order (ByteOrder/nativeOrder))
      (.asDoubleBuffer)))

(defn direct-tensor [values shape]
  (let [n (int (reduce * shape))
        buf (alloc-direct n)]
    (dotimes [i (min n (count values))]
      (.put buf (int i) (double (nth values i))))
    {:data buf :shape (vec shape) :numel n :type :direct}))

(defn heap-tensor [values shape]
  (let [n (int (reduce * shape))
        da (double-array n)]
    (dotimes [i (min n (count values))]
      (aset da i (double (nth values i))))
    {:data da :shape (vec shape) :numel n :type :heap}))

;; ============================================================
;; FAST ACCESSORS
;; ============================================================

(defn tget ^double [t i]
  (if (= :direct (:type t))
    (.get ^DoubleBuffer (:data t) (int i))
    (aget ^doubles (:data t) (int i))))

(defn tset! [t i v]
  (if (= :direct (:type t))
    (.put ^DoubleBuffer (:data t) (int i) (double v))
    (aset ^doubles (:data t) (int i) (double v))))

;; ============================================================
;; PARALLEL ELEMENTWISE (ForkJoinPool)
;; ============================================================

(def ^ForkJoinPool pool (ForkJoinPool/commonPool))
(def ^:const PARALLEL-THRESHOLD 4096)

(defn parallel-elementwise! [out a op n]
  (let [n (int n)]
    (if (< n PARALLEL-THRESHOLD)
      (dotimes [i n] (tset! out i (op (tget a i))))
      (let [chunks (min (.getParallelism pool) 8)
            chunk-size (quot n chunks)]
        (.invoke pool
          (proxy [RecursiveAction] []
            (compute []
              (let [tasks
                    (mapv (fn [c]
                            (let [start (int (* c chunk-size))
                                  end   (int (if (= c (dec chunks)) n (+ start chunk-size)))]
                              (proxy [RecursiveAction] []
                                (compute []
                                  (loop [i start]
                                    (when (< i end)
                                      (tset! out i (op (tget a i)))
                                      (recur (inc i))))))))
                          (range chunks))]
                (ForkJoinTask/invokeAll ^java.util.Collection tasks)))))))))

;; ============================================================
;; TILED MATMUL (cache-friendly)
;; ============================================================

(def ^:const TILE-SIZE 32)

(defn tiled-matmul!
  "Блочное C += A@B. A[m×k] B[k×n] C[m×n]."
  [c a b dims]
  (let [^doubles c c
        ^doubles a a
        ^doubles b b
        m (int (nth dims 0))
        k (int (nth dims 1))
        n (int (nth dims 2))
        ts (int TILE-SIZE)]
    (loop [ii (int 0)]
      (when (< ii m)
        (let [ie (int (min (+ ii ts) m))]
          (loop [jj (int 0)]
            (when (< jj n)
              (let [je (int (min (+ jj ts) n))]
                (loop [pp (int 0)]
                  (when (< pp k)
                    (let [pe (int (min (+ pp ts) k))]
                      (loop [i (int ii)]
                        (when (< i ie)
                          (loop [p (int pp)]
                            (when (< p pe)
                              (let [av (aget a (+ (* i k) p))]
                                (loop [j (int jj)]
                                  (when (< j je)
                                    (aset c (+ (* i n) j)
                                          (+ (aget c (+ (* i n) j))
                                             (* av (aget b (+ (* p n) j)))))
                                    (recur (inc j)))))
                              (recur (inc p))))
                          (recur (inc i)))))
                    (recur (+ pp ts)))))
              (recur (+ jj ts)))))
        (recur (+ ii ts))))))

(defn naive-matmul! [c a b dims]
  (let [^doubles c c
        ^doubles a a
        ^doubles b b
        m (int (nth dims 0))
        k (int (nth dims 1))
        n (int (nth dims 2))]
    (dotimes [i m]
      (dotimes [j n]
        (loop [p (int 0) s 0.0]
          (if (= p k)
            (aset c (+ (* i n) j) s)
            (recur (inc p)
                   (+ s (* (aget a (+ (* i k) p))
                           (aget b (+ (* p n) j)))))))))))

;; ============================================================
;; MEMORY POOL
;; ============================================================

(def buffer-pool (atom {}))

(defn pool-alloc ^doubles [n]
  (let [n (int n)]
    (if-let [buf (first (get @buffer-pool n))]
      (do (swap! buffer-pool update n rest)
          (java.util.Arrays/fill ^doubles buf 0.0)
          buf)
      (double-array n))))

(defn pool-free [^doubles buf]
  (swap! buffer-pool update (alength buf) #(conj (or % '()) buf)))

;; ============================================================
;; VECTORIZED OPS (SIMD-friendly tight loops)
;; ============================================================

(defn vec-add! [^doubles out ^doubles a ^doubles b n]
  (let [n (int n)] (dotimes [i n] (aset out i (+ (aget a i) (aget b i))))))

(defn vec-mul! [^doubles out ^doubles a ^doubles b n]
  (let [n (int n)] (dotimes [i n] (aset out i (* (aget a i) (aget b i))))))

(defn vec-sin! [^doubles out ^doubles a n]
  (let [n (int n)] (dotimes [i n] (aset out i (math/sin (aget a i))))))

(defn vec-square! [^doubles out ^doubles a n]
  (let [n (int n)] (dotimes [i n] (aset out i (* (aget a i) (aget a i))))))

(defn vec-fma! [^doubles out ^doubles a ^doubles b n]
  (let [n (int n)] (dotimes [i n] (aset out i (+ (aget out i) (* (aget a i) (aget b i)))))))

;; ============================================================
;; BATCHED KAN POLY (Horner scheme)
;; ============================================================

(defn poly-batch-horner! [^doubles out ^doubles coeffs ^doubles x params]
  (let [n   (int (nth params 0))
        deg (int (nth params 1))]
    (dotimes [j n]
      (let [xj (aget x j)]
        (aset out j
              (loop [i (int deg) acc (aget coeffs deg)]
                (if (< i 1) acc
                  (recur (dec i) (+ (aget coeffs (dec i)) (* acc xj))))))))))

;; ============================================================
;; BENCHMARK HELPER
;; ============================================================

(defn bench [label f warmup runs]
  (dotimes [_ warmup] (f))
  (let [t0 (System/nanoTime)]
    (dotimes [_ runs] (f))
    (let [total-ms (/ (- (System/nanoTime) t0) 1e6)]
      (println (format "    %-30s %8.3f ms  (avg/%d)" label (/ total-ms runs) runs))
      (/ total-ms runs))))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-jvm-backend []
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  High-Performance JVM Backend        ║")
  (println "  ╚══════════════════════════════════════╝")

  ;; 1. Direct vs Heap
  (println "\n  Part 1: Direct vs Heap memory (N=100000)")
  (let [n 100000
        xs (vec (repeatedly n rand))
        heap (heap-tensor xs [n])
        direct (direct-tensor xs [n])
        h-out (heap-tensor (repeat n 0) [n])
        d-out (direct-tensor (repeat n 0) [n])]
    (bench "Heap sin(x)"
           #(dotimes [i n] (tset! h-out i (math/sin (tget heap i)))) 3 5)
    (bench "Direct sin(x)"
           #(dotimes [i n] (tset! d-out i (math/sin (tget direct i)))) 3 5))

  ;; 2. Naive vs Tiled MatMul
  (println "\n  Part 2: Naive vs Tiled MatMul (128×128)")
  (let [sz 128
        a (double-array (repeatedly (* sz sz) rand))
        b (double-array (repeatedly (* sz sz) rand))
        c1 (double-array (* sz sz))
        c2 (double-array (* sz sz))
        dims [sz sz sz]]
    (bench "Naive matmul 128×128"
           #(do (java.util.Arrays/fill c1 0.0) (naive-matmul! c1 a b dims)) 3 10)
    (bench "Tiled matmul 128×128"
           #(do (java.util.Arrays/fill c2 0.0) (tiled-matmul! c2 a b dims)) 3 10)
    (let [err (loop [i 0 mx 0.0]
                (if (= i (* sz sz)) mx
                  (recur (inc i) (max mx (abs (- (aget c1 i) (aget c2 i)))))))]
      (println (format "    Verify: diff = %.2e %s" err (if (< err 1e-8) "✅" "❌")))))

  ;; 3. Sequential vs Parallel
  (println "\n  Part 3: Sequential vs Parallel (N=500000)")
  (let [n 500000
        a (heap-tensor (repeatedly n rand) [n])
        o1 (heap-tensor (repeat n 0) [n])
        o2 (heap-tensor (repeat n 0) [n])]
    (bench "Sequential sin(x)"
           #(dotimes [i n] (tset! o1 i (math/sin (tget a i)))) 2 3)
    (bench "Parallel sin(x)"
           #(parallel-elementwise! o2 a (fn [x] (math/sin x)) n) 2 3)
    (let [err (loop [i 0 mx 0.0]
                (if (= i n) mx
                  (recur (inc i) (max mx (abs (- (tget o1 i) (tget o2 i)))))))]
      (println (format "    Verify: diff = %.2e %s" err (if (< err 1e-10) "✅" "❌")))))

  ;; 4. Vectorized ops
  (println "\n  Part 4: Vectorized ops (N=100000)")
  (let [n 100000
        a (double-array (repeatedly n rand))
        b (double-array (repeatedly n rand))
        out (double-array n)]
    (bench "vec-add!" #(vec-add! out a b n) 3 20)
    (bench "vec-mul!" #(vec-mul! out a b n) 3 20)
    (bench "vec-square!" #(vec-square! out a n) 3 20)
    (bench "vec-fma!" #(vec-fma! out a b n) 3 20))

  ;; 5. Memory pool
  (println "\n  Part 5: Memory pool vs fresh alloc")
  (let [n 10000]
    (bench "Fresh alloc(10000)"
           #(let [buf (double-array n)] (aset buf 0 1.0)) 3 100)
    (bench "Pool alloc(10000)"
           #(let [buf (pool-alloc n)] (aset buf 0 1.0) (pool-free buf)) 3 100))

  ;; 6. Horner poly
  (println "\n  Part 6: Naive vs Horner poly (N=10000, deg=5)")
  (let [n 10000 deg 5
        coeffs (double-array (repeatedly (inc deg) (fn [] (- (rand) 0.5))))
        x (double-array (repeatedly n (fn [] (- (* 4.0 (rand)) 2.0))))
        o1 (double-array n)
        o2 (double-array n)]
    (bench "Naive poly"
           #(dotimes [j n]
              (let [xj (aget x j)]
                (aset o1 j (loop [i 0 acc 0.0 xp 1.0]
                             (if (> i deg) acc
                               (recur (inc i) (+ acc (* (aget coeffs i) xp)) (* xp xj))))))) 3 20)
    (bench "Horner poly"
           #(poly-batch-horner! o2 coeffs x [n deg]) 3 20)
    (let [err (loop [i 0 mx 0.0]
                (if (= i n) mx
                  (recur (inc i) (max mx (abs (- (aget o1 i) (aget o2 i)))))))]
      (println (format "    Verify: diff = %.2e %s" err (if (< err 1e-10) "✅" "❌"))))))
