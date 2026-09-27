(ns kan-kat.kan-layer
  "Kolmogorov-Arnold Network Layer — v2.0
   
   Upgrades over v1:
   1. Per-input-dimension grids — each x_i has its own knot vector
   2. LayerNorm on input (learnable gamma/beta, stabilizes training)
   3. Adaptive grid update (quantile + uniform blend, as in Liu et al. 2024)
   4. Batch-native forward (operates on [batch × features] directly)
   
   Each edge φ_{j,i}(x):
     φ_{j,i}(x_i) = wb_{j,i} · SiLU(x_i) + ws_{j,i} · Σ_c(coeff_c · B_c(x_i))
   Output_j = Σ_i φ_{j,i}(x_i)"
  (:require [clojure.math :as math]
            [kan-kat.math :as m]
            [kan-kat.spline :as spl]
            [kan-kat.ad :as ad]))

;; ============================================================
;; KAN Layer v2 structure
;; ============================================================
;; {:in-features      Int
;;  :out-features     Int
;;  :spline-order     Int
;;  :grid-size        Int
;;  :grid-bounds      [min max]
;;  :grids            [[knots-dim-0] [knots-dim-1] ...]  ;; per-input grids!
;;  :base-weights     [[Double...] ...]       ;; [out, in]  — wb per edge
;;  :spline-scales    [[Double...] ...]       ;; [out, in]  — ws per edge
;;  :spline-weights   [[[Double...] ...] ...] ;; [out, in, num-splines]
;;  :ln-gamma         [Double...]             ;; [in] — LayerNorm scale
;;  :ln-beta          [Double...]             ;; [in] — LayerNorm shift
;; }

(defn num-spl-coeffs
  "Number of B-spline coefficients per edge."
  [layer]
  (+ (:grid-size layer) (:spline-order layer)))

(defn num-params
  "Total trainable parameters:
   wb(out*in) + ws(out*in) + coeffs(out*in*n-spl) + ln-gamma(in) + ln-beta(in)"
  [layer]
  (let [n-edges (* (:out-features layer) (:in-features layer))
        n-spl   (num-spl-coeffs layer)
        in-f    (:in-features layer)]
    (+ n-edges n-edges (* n-edges n-spl) in-f in-f)))

;; ============================================================
;; Parameter serialization
;; Layout: [wb | ws | spline-coeffs | ln-gamma | ln-beta]
;; ============================================================

(defn extract-params
  "Extract all trainable parameters as a flat vector."
  [layer]
  (vec (concat
         (apply concat (:base-weights layer))
         (apply concat (:spline-scales layer))
         (mapcat (fn [out-row] (mapcat identity out-row))
                 (:spline-weights layer))
         (:ln-gamma layer)
         (:ln-beta layer))))

(defn inject-params
  "Inject a flat parameter list back into a KAN layer."
  [layer params]
  (let [in-f   (:in-features layer)
        out-f  (:out-features layer)
        n-spl  (num-spl-coeffs layer)
        n-edges (* out-f in-f)
        ;; Split: [wb | ws | coeffs | ln-gamma | ln-beta]
        [wb-flat r1]          (split-at n-edges params)
        [ws-flat r2]          (split-at n-edges r1)
        [coeff-flat r3]       (split-at (* n-edges n-spl) r2)
        [gamma-flat beta-flat] (split-at in-f r3)
        bw (mapv vec (m/chunks-of in-f wb-flat))
        ss (mapv vec (m/chunks-of in-f ws-flat))
        sw (mapv (fn [chunk]
                   (mapv vec (m/chunks-of n-spl chunk)))
                 (m/chunks-of (* in-f n-spl) coeff-flat))]
    (assoc layer
           :base-weights bw
           :spline-scales ss
           :spline-weights sw
           :ln-gamma (vec gamma-flat)
           :ln-beta (vec beta-flat))))

;; ============================================================
;; LayerNorm (Dual-aware for AD)
;; ============================================================

(defn layer-norm-dual
  "LayerNorm on a single sample vector (Dual-aware).
   normalized_i = gamma_i * (x_i - mean) / std + beta_i
   Skips normalization for single-feature input (degenerate case)."
  [x gamma beta]
  (let [n (count x)]
    (if (<= n 1)
      ;; Identity for 1D: just apply gamma * x + beta
      (mapv (fn [xi gi bi]
              (ad/d-add (ad/d-mul gi xi) bi))
            x gamma beta)
      ;; Standard LayerNorm for 2D+
      (let [n-d   (ad/const-d (double n))
            mean  (ad/d-div (ad/d-sum x) n-d)
            diffs (mapv #(ad/d-sub % mean) x)
            var   (ad/d-div (ad/d-sum (mapv #(ad/d-mul % %) diffs)) n-d)
            std   (ad/d-sqrt (ad/d-add var (ad/const-d 1e-5)))
            normd (mapv #(ad/d-div % std) diffs)]
        (mapv (fn [ni gi bi]
                (ad/d-add (ad/d-mul gi ni) bi))
              normd gamma beta)))))

;; ============================================================
;; Dual-aware B-spline evaluation (for AD)
;; ============================================================

(defn b-spline-dual
  "Cox-de Boor recursion with Dual number support."
  [i k x knots]
  (if (zero? k)
    (let [t-i  (nth knots i)
          t-i1 (nth knots (inc i))]
      (if (or (and (ad/d-le t-i x) (ad/d-lt x t-i1))
              (and (ad/d-le t-i x) (ad/d-eq x t-i1)
                   (= i (- (count knots) 2))))
        (ad/const-d 1.0)
        (ad/const-d 0.0)))
    (let [t-i   (nth knots i)
          t-i1  (nth knots (inc i))
          t-ik  (nth knots (+ i k))
          t-ik1 (nth knots (+ i k 1))
          denom1 (ad/d-sub t-ik t-i)
          term1  (if (ad/d-eq denom1 (ad/const-d 0.0))
                   (ad/const-d 0.0)
                   (ad/d-mul (ad/d-div (ad/d-sub x t-i) denom1)
                             (b-spline-dual i (dec k) x knots)))
          denom2 (ad/d-sub t-ik1 t-i1)
          term2  (if (ad/d-eq denom2 (ad/const-d 0.0))
                   (ad/const-d 0.0)
                   (ad/d-mul (ad/d-div (ad/d-sub t-ik1 x) denom2)
                             (b-spline-dual (inc i) (dec k) x knots)))]
      (ad/d-add term1 term2))))

(defn eval-splines-at-dual
  "Eval all B-spline basis functions at point x using knots for that dim."
  [k knots x]
  (let [num-spl (- (count knots) k 1)]
    (mapv #(b-spline-dual % k x knots) (range num-spl))))

;; ============================================================
;; Forward pass (polymorphic via AD) — v2 with per-input grids & LN
;; ============================================================

(defn forward-generic
  "Polymorphic forward pass with per-input grids and LayerNorm.
   
   Params layout: [wb | ws | spline_coeffs | ln-gamma | ln-beta]
   grids-a: vector of dual-knot-vectors, one per input dimension"
  [in-f out-f k n-spl grids-a params input]
  (let [n-edges (* out-f in-f)
        ;; Split params
        [wb-flat r1]          (split-at n-edges params)
        [ws-flat r2]          (split-at n-edges r1)
        [coeff-flat r3]       (split-at (* n-edges n-spl) r2)
        [gamma-params beta-params] (split-at in-f r3)
        
        ;; LayerNorm on input
        normed (layer-norm-dual input (vec gamma-params) (vec beta-params))
        
        ;; Parse weight matrices
        bw (m/chunks-of in-f wb-flat)
        ss (m/chunks-of in-f ws-flat)
        sw (mapv (fn [chunk] (m/chunks-of n-spl chunk))
                 (m/chunks-of (* in-f n-spl) coeff-flat))
        
        ;; Base path: wb · SiLU(x_i)
        silu-x    (mapv ad/d-silu normed)
        base-path (mapv #(ad/d-dot-product silu-x %) bw)
        
        ;; Spline path with per-input grids & per-edge ws
        ;; Each input dim i uses grids-a[i] for its B-spline eval
        spline-vals (mapv (fn [i]
                            (eval-splines-at-dual k (nth grids-a i) (nth normed i)))
                          (range in-f))
        ;; For each output j: Σ_i (ws_{j,i} · Σ_c(coeff_c · B_c(x_i)))
        spline-path (mapv (fn [sw-j ss-j]
                            (ad/d-sum
                              (mapv (fn [sv coeffs ws-val]
                                      (ad/d-mul ws-val (ad/d-dot-product sv coeffs)))
                                    spline-vals sw-j ss-j)))
                          sw ss)]
    
    (mapv ad/d-add base-path spline-path)))

;; ============================================================
;; Concrete (Double) forward pass — single & batch
;; ============================================================

(defn forward-layer
  "Standard forward pass for one sample."
  [layer x]
  (let [grids-d  (mapv (fn [knots] (mapv ad/const-d knots)) (:grids layer))
        params-d (mapv ad/const-d (extract-params layer))
        input-d  (mapv ad/const-d x)
        result   (forward-generic (:in-features layer) (:out-features layer)
                                  (:spline-order layer) (num-spl-coeffs layer)
                                  grids-d params-d input-d)]
    (mapv :primal result)))

(defn forward-layer-batch
  "Batch forward pass: [[sample1] [sample2] ...] → [[out1] [out2] ...]"
  [layer batch]
  (mapv #(forward-layer layer %) batch))

;; ============================================================
;; Adaptive Grid Update (from Liu et al. 2024)
;; ============================================================

(defn update-grids-from-batch
  "Update per-input grids using batch activations.
   grid-eps: 1.0 = fully uniform, 0.0 = fully quantile-based.
   
   For each input dimension:
   1. Compute range + margin from batch activations
   2. Build uniform grid over [min-margin, max+margin]
   3. Build quantile grid from sorted activations
   4. Blend: grid-eps * uniform + (1-grid-eps) * quantile"
  [layer batch-x grid-eps]
  (let [{:keys [in-features grid-size spline-order]} layer
        k grid-size
        ord spline-order]
    (assoc layer :grids
      (mapv (fn [i]
              (let [;; Extract column i from batch
                    col     (mapv #(nth % i) batch-x)
                    min-x   (apply min col)
                    max-x   (apply max col)
                    margin  (* 0.15 (max 0.01 (- max-x min-x)))
                    a       (- min-x margin)
                    b       (+ max-x margin)
                    
                    ;; Uniform grid
                    uniform (spl/init-knots ord k [a b])
                    
                    ;; Quantile-adaptive grid
                    sorted-col (vec (sort col))
                    n          (count sorted-col)
                    ;; Sample (k+1) quantile positions from sorted data
                    adaptive-inner
                    (mapv (fn [j]
                            (let [idx (min (dec n)
                                          (int (* j (/ (dec n) k))))]
                              (nth sorted-col idx)))
                          (range (inc k)))
                    ;; Extend with order repetitions at boundaries
                    adaptive (vec (concat
                                   (repeat ord (first adaptive-inner))
                                   adaptive-inner
                                   (repeat ord (last adaptive-inner))))
                    
                    ;; Blend (ensure same length)
                    len (min (count uniform) (count adaptive))
                    blended (mapv (fn [idx]
                                   (+ (* grid-eps (nth uniform idx))
                                      (* (- 1.0 grid-eps) (nth adaptive idx))))
                                 (range len))]
                blended))
            (range in-features)))))

;; ============================================================
;; Grid Refinement (Liu et al. 2024)
;; ============================================================

(defn refine-grid
  "Удвоение числа интервалов сетки G → 2G.
   
   Из оригинальной KAN-статьи (Liu et al., 2024):
   начинаем с грубой сетки (быстрая сходимость),
   периодически удваиваем для повышения точности.
   
   Алгоритм:
   1. Новая сетка: G' = 2G интервалов (вставка средних точек)
   2. Перенос коэффициентов: линейная интерполяция
      - Старые coeffs[i] → новые позиции чётных индексов
      - Промежуточные = среднее соседних
   3. Новые дополнительные коэфф. инициализируются малым шумом
   
   Число B-spline базисов: n_spl = G + k → 2G + k"
  [layer]
  (let [{:keys [grid-size spline-order in-features out-features
                grids spline-weights grid-bounds]} layer
        new-g   (* 2 grid-size)
        k       spline-order
        old-spl (+ grid-size k)
        new-spl (+ new-g k)
        
        ;; Обновляем сетки (удвоение интервалов)
        new-grids
        (mapv (fn [old-knots]
                (let [[a b] grid-bounds]
                  (spl/init-knots k new-g [a b])))
              grids)
        
        ;; Переносим коэффициенты: линейная интерполяция + padding
        new-sw
        (mapv (fn [out-row]     ;; [in, old-spl]
                (mapv (fn [edge-coeffs]
                        (let [old-n (count edge-coeffs)
                              ;; Линейная интерполяция: вставляем средние
                              interpolated
                              (vec (mapcat (fn [i]
                                            (if (< i (dec old-n))
                                              [(nth edge-coeffs i)
                                               (/ (+ (nth edge-coeffs i)
                                                     (nth edge-coeffs (inc i)))
                                                  2.0)]
                                              [(nth edge-coeffs i)]))
                                          (range old-n)))
                              ;; Обрезаем/дополняем до new-spl
                              padded (if (>= (count interpolated) new-spl)
                                       (subvec interpolated 0 new-spl)
                                       (vec (concat interpolated
                                                    (repeatedly
                                                      (- new-spl (count interpolated))
                                                      #(* 0.01 (- (rand) 0.5))))))]
                          padded))
                      out-row))
              spline-weights)]
    (assoc layer
           :grid-size      new-g
           :grids          new-grids
           :spline-weights new-sw)))

