(ns kan-kat.backprop
  "Аналитический Backpropagation для KAN v2.
   
   ═══════════════════════════════════════════════════════════
   ЗАЧЕМ НУЖЕН АНАЛИТИЧЕСКИЙ BACKWARD?
   ═══════════════════════════════════════════════════════════
   
   Forward-mode AD (ad.clj) корректен, но требует O(n_params)
   forward passes на одну точку. Для KAN [2→5→1] с 120 параметрами
   это 120 проходов на 1 семпл — очень медленно.
   
   Аналитический backward вычисляет все градиенты за 1 forward +
   1 backward pass = O(1), давая ускорение 50-100×.
   
   ═══════════════════════════════════════════════════════════
   ФОРМУЛЫ ГРАДИЕНТОВ
   ═══════════════════════════════════════════════════════════
   
   Для одного ребра φ_{j,i}(x_i) = wb·SiLU(x_i) + ws·Σ_c(c_c·B_c(x_i)):
   
   ∂φ/∂wb      = SiLU(x_i)
   ∂φ/∂ws      = Σ_c(c_c · B_c(x_i))    [= spline_val]
   ∂φ/∂coeff_c = ws · B_c(x_i)
   ∂φ/∂x_i     = wb·SiLU'(x_i) + ws·Σ_c(c_c · B'_c(x_i))  [для chain rule]
   
   SiLU'(x) = σ(x) + x·σ(x)·(1 - σ(x))
   
   B'_{i,k}(x) = (k-1)·[B_{i,k-1}(x)/(t_{i+k-1}-t_i)
                        - B_{i+1,k-1}(x)/(t_{i+k}-t_{i+1})]
   
   ═══════════════════════════════════════════════════════════
   ЧИСТО ФУНКЦИОНАЛЬНЫЙ СТИЛЬ
   ═══════════════════════════════════════════════════════════
   
   В отличие от референсной реализации (atom/swap!), здесь все
   операции чисто функциональные — reduce вместо мутаций.
   ═══════════════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.math :as m]
            [kan-kat.spline :as spl]
            [kan-kat.kan-layer :as kan]))

;; ============================================================
;; ПРОИЗВОДНЫЕ АКТИВАЦИЙ
;; ============================================================

(defn sigmoid
  "σ(x) = 1 / (1 + exp(-x))"
  [x]
  (/ 1.0 (+ 1.0 (math/exp (- x)))))

(defn silu-deriv
  "Производная SiLU: SiLU'(x) = σ(x) + x·σ(x)·(1 - σ(x))
   
   Вывод: SiLU(x) = x·σ(x)
   d/dx[x·σ(x)] = σ(x) + x·σ'(x) = σ(x) + x·σ(x)·(1-σ(x))"
  [x]
  (let [s (sigmoid x)]
    (+ s (* x s (- 1.0 s)))))

;; ============================================================
;; ПРОИЗВОДНАЯ B-SPLINE
;; ============================================================

(defn bspline-deriv
  "Производная B-spline базиса B'_{i,k}(x).
   
   Формула (Cox-de Boor для производных):
   B'_{i,k}(x) = (k)·[ B_{i,k-1}(x) / (t_{i+k} - t_i)
                       - B_{i+1,k-1}(x) / (t_{i+k+1} - t_{i+1}) ]
   
   Для k=0: B'=0 (кусочно-постоянная, производная=0 всюду кроме узлов)"
  [i k x knots]
  (if (zero? k)
    0.0
    (let [t-i   (nth knots i)
          t-i1  (nth knots (inc i))
          t-ik  (nth knots (+ i k))
          t-ik1 (nth knots (+ i k 1))
          d1    (- t-ik t-i)
          d2    (- t-ik1 t-i1)
          term1 (if (pos? d1)
                  (/ (spl/b-spline i (dec k) x knots) d1)
                  0.0)
          term2 (if (pos? d2)
                  (/ (spl/b-spline (inc i) (dec k) x knots) d2)
                  0.0)]
      (* (double k) (- term1 term2)))))

;; ============================================================
;; FORWARD + BACKWARD ДЛЯ ОДНОГО РЕБРА φ_{j,i}
;; ============================================================

(defn phi-forward-backward
  "Forward + backward для одного ребра φ_{j,i}(x_i).
   
   Входы:
     x        — значение входного признака x_i
     wb       — базовый вес (SiLU path)
     ws       — масштаб сплайна
     coeffs   — коэффициенты B-spline [c_0 ... c_{n-1}]
     knots    — узлы для данного входного измерения
     k        — порядок B-spline
     dL_dphi  — upstream градиент ∂L/∂φ_{j,i}
   
   Выходы: [phi_value, {:dwb, :dws, :dcoeffs, :dx}]
     :dwb     — ∂L/∂wb     = dL_dphi · SiLU(x)
     :dws     — ∂L/∂ws     = dL_dphi · spline_val
     :dcoeffs — ∂L/∂c_c    = dL_dphi · ws · B_c(x)
     :dx      — ∂L/∂x_i    = dL_dphi · (wb·SiLU'(x) + ws·Σ(c·B'(x)))"
  [x wb ws coeffs knots k dL_dphi]
  (let [n          (count coeffs)
        ;; Forward: evaluate basis functions + spline
        basis-vals (mapv #(spl/b-spline % k x knots) (range n))
        spline-val (reduce + 0.0 (map * coeffs basis-vals))
        silu-x     (m/silu x)
        phi-val    (+ (* wb silu-x) (* ws spline-val))
        
        ;; Backward: gradients
        dwb     (* dL_dphi silu-x)
        dws     (* dL_dphi spline-val)
        dcoeffs (mapv #(* dL_dphi ws %) basis-vals)
        
        ;; ∂φ/∂x (для chain rule к предыдущему слою)
        basis-derivs (mapv #(bspline-deriv % k x knots) (range n))
        spline-deriv (reduce + 0.0 (map * coeffs basis-derivs))
        dphi-dx      (+ (* wb (silu-deriv x)) (* ws spline-deriv))
        dx           (* dL_dphi dphi-dx)]
    [phi-val {:dwb dwb :dws dws :dcoeffs dcoeffs :dx dx}]))

;; ============================================================
;; LAYER BACKWARD (чисто функциональный)
;; ============================================================

(defn layer-norm-forward-cache
  "Forward LayerNorm + сохранение промежуточных значений для backward.
   Возвращает [normed, cache] где cache нужен для backward."
  [x gamma beta]
  (let [n   (count x)
        mu  (/ (reduce + 0.0 x) n)
        d   (mapv #(- % mu) x)
        var (/ (reduce + 0.0 (map #(* % %) d)) n)
        std (math/sqrt (+ var 1e-5))
        normed (mapv #(/ % std) d)
        out (mapv (fn [ni gi bi] (+ (* gi ni) bi)) normed gamma beta)]
    [out {:mu mu :std std :normed normed :d d :n n}]))

(defn layer-norm-backward
  "Backward pass для LayerNorm.
   dL/dx, dL/dgamma, dL/dbeta из dL/dout."
  [dL-dout gamma {:keys [std normed d n]}]
  (let [;; dL/dgamma_i = dL/dout_i · normed_i
        dgamma (mapv * dL-dout normed)
        ;; dL/dbeta_i = dL/dout_i
        dbeta  (vec dL-dout)
        ;; dL/dnormed = dL/dout · gamma
        dL-dnormed (mapv * dL-dout gamma)
        ;; dL/dx (LayerNorm backward formula)
        sum1    (reduce + 0.0 dL-dnormed)
        sum2    (reduce + 0.0 (map * dL-dnormed normed))
        dx      (mapv (fn [dln ni]
                        (/ (- dln (/ sum1 n) (* ni (/ sum2 n))) std))
                      dL-dnormed normed)]
    {:dx dx :dgamma dgamma :dbeta dbeta}))

(defn kan-layer-backward
  "Backward pass для одного KAN-слоя (чисто функциональный).
   
   Входы:
     layer          — KAN-слой
     batch-x        — входной батч [[x1] [x2] ...] (до LayerNorm)
     batch-upstream  — upstream градиенты ∂L/∂output [[dL/dy1] ...]
   
   Выходы:
     [grad-map, batch-input-delta]
   
   grad-map содержит:
     :dwb-grads     — [out × in] градиенты по wb
     :dws-grads     — [out × in] градиенты по ws
     :dcoeff-grads  — [out × in × n_spl] градиенты по coeffs
     :dgamma        — [in] градиенты по ln-gamma
     :dbeta         — [in] градиенты по ln-beta
   
   batch-input-delta — ∂L/∂x для chaining к предыдущему слою"
  [layer batch-x batch-upstream]
  (let [{:keys [in-features out-features grids spline-order
                base-weights spline-scales spline-weights
                ln-gamma ln-beta]} layer
        k        spline-order
        n-spl    (kan/num-spl-coeffs layer)
        n-edges  (* out-features in-features)
        bs       (count batch-x)
        
        ;; Аккумулируем градиенты по всему батчу (чисто функционально)
        result
        (reduce
          (fn [acc s-idx]
            (let [sample   (nth batch-x s-idx)
                  upstream (nth batch-upstream s-idx)
                  
                  ;; LayerNorm forward + cache
                  [normed ln-cache]
                  (if (<= in-features 1)
                    ;; 1D: identity (apply gamma*x+beta)
                    [(mapv (fn [xi gi bi] (+ (* gi xi) bi))
                           sample ln-gamma ln-beta)
                     nil]
                    (layer-norm-forward-cache sample ln-gamma ln-beta))
                  
                  ;; Backward через все рёбра
                  edge-result
                  (reduce
                    (fn [eacc j]
                      (let [dL-doutj (nth upstream j)]
                        (reduce
                          (fn [iacc i]
                            (let [p-idx  (+ (* j in-features) i)
                                  wb     (nth (nth base-weights j) i)
                                  ws     (nth (nth spline-scales j) i)
                                  coeffs (nth (nth spline-weights j) i)
                                  knots  (nth grids i)
                                  xi     (nth normed i)
                                  
                                  [_phi-val grads]
                                  (phi-forward-backward xi wb ws coeffs knots k dL-doutj)]
                              (-> iacc
                                  (update-in [:dwb p-idx] + (:dwb grads))
                                  (update-in [:dws p-idx] + (:dws grads))
                                  (update :dcoeffs
                                          (fn [dc]
                                            (update dc p-idx
                                                    #(mapv + % (:dcoeffs grads)))))
                                  (update-in [:dx-normed i] + (:dx grads)))))
                          eacc (range in-features))))
                    {:dwb      (vec (repeat n-edges 0.0))
                     :dws      (vec (repeat n-edges 0.0))
                     :dcoeffs  (vec (repeat n-edges (vec (repeat n-spl 0.0))))
                     :dx-normed (vec (repeat in-features 0.0))}
                    (range out-features))
                  
                  ;; LayerNorm backward
                  ln-grads
                  (if ln-cache
                    (layer-norm-backward (:dx-normed edge-result) ln-gamma ln-cache)
                    {:dx      (mapv (fn [dxn gi] (* dxn gi))
                                   (:dx-normed edge-result) ln-gamma)
                     :dgamma  (mapv * (:dx-normed edge-result) sample)
                     :dbeta   (:dx-normed edge-result)})]
              
              ;; Аккумулируем
              (-> acc
                  (update :dwb-sum #(mapv + % (:dwb edge-result)))
                  (update :dws-sum #(mapv + % (:dws edge-result)))
                  (update :dcoeff-sum
                          (fn [dc]
                            (mapv (fn [a b] (mapv + a b))
                                  dc (:dcoeffs edge-result))))
                  (update :dgamma-sum #(mapv + % (:dgamma ln-grads)))
                  (update :dbeta-sum  #(mapv + % (:dbeta ln-grads)))
                  (update :input-deltas conj (:dx ln-grads)))))
          
          ;; Initial accumulator
          {:dwb-sum     (vec (repeat n-edges 0.0))
           :dws-sum     (vec (repeat n-edges 0.0))
           :dcoeff-sum  (vec (repeat n-edges (vec (repeat n-spl 0.0))))
           :dgamma-sum  (vec (repeat in-features 0.0))
           :dbeta-sum   (vec (repeat in-features 0.0))
           :input-deltas []}
          
          (range bs))]
    
    ;; Нормализуем по размеру батча
    (let [inv-bs (/ 1.0 (double bs))]
      [{:dwb-grads    (mapv #(* inv-bs %) (:dwb-sum result))
        :dws-grads    (mapv #(* inv-bs %) (:dws-sum result))
        :dcoeff-grads (mapv (fn [v] (mapv #(* inv-bs %) v))
                            (:dcoeff-sum result))
        :dgamma       (mapv #(* inv-bs %) (:dgamma-sum result))
        :dbeta        (mapv #(* inv-bs %) (:dbeta-sum result))}
       (:input-deltas result)])))

;; ============================================================
;; ПРИМЕНЕНИЕ ГРАДИЕНТОВ К СЛОЮ
;; ============================================================

(defn apply-grads-to-layer
  "Обновление параметров слоя по градиентам (SGD).
   Чисто функциональное: возвращает новый слой."
  [layer grads lr]
  (let [{:keys [in-features out-features]} layer
        {:keys [dwb-grads dws-grads dcoeff-grads dgamma dbeta]} grads]
    (assoc layer
           :base-weights
           (mapv (fn [j]
                   (mapv (fn [i]
                           (let [idx (+ (* j in-features) i)]
                             (- (nth (nth (:base-weights layer) j) i)
                                (* lr (nth dwb-grads idx)))))
                         (range in-features)))
                 (range out-features))
           
           :spline-scales
           (mapv (fn [j]
                   (mapv (fn [i]
                           (let [idx (+ (* j in-features) i)]
                             (- (nth (nth (:spline-scales layer) j) i)
                                (* lr (nth dws-grads idx)))))
                         (range in-features)))
                 (range out-features))
           
           :spline-weights
           (mapv (fn [j]
                   (mapv (fn [i]
                           (let [idx (+ (* j in-features) i)]
                             (mapv (fn [c old-c]
                                     (- old-c (* lr c)))
                                   (nth dcoeff-grads idx)
                                   (nth (nth (:spline-weights layer) j) i))))
                         (range in-features)))
                 (range out-features))
           
           :ln-gamma (mapv (fn [g dg] (- g (* lr dg)))
                           (:ln-gamma layer) dgamma)
           :ln-beta  (mapv (fn [b db] (- b (* lr db)))
                           (:ln-beta layer) dbeta))))

;; ============================================================
;; MULTI-LAYER BACKWARD
;; ============================================================

(defn kan-forward-with-cache
  "Forward pass через несколько KAN-слоёв с кэшированием активаций.
   Возвращает вектор активаций: [input, after-L1, after-L2, ...]"
  [layers batch-x]
  (reduce (fn [activations layer]
            (conj activations
                  (kan/forward-layer-batch layer (last activations))))
          [batch-x]
          layers))

(defn mse-output-gradient
  "Градиент MSE loss по предсказанию: ∂MSE/∂pred = 2(pred-target)/n.
   Возвращает батч upstream-градиентов."
  [predictions targets]
  (let [n (count (first predictions))]
    (mapv (fn [pred tgt]
            (mapv (fn [p t] (/ (* 2.0 (- p t)) n))
                  pred tgt))
          predictions targets)))

(defn kan-multi-backward
  "Полный backward через стек KAN-слоёв.
   
   1. Forward с кэшем активаций
   2. MSE gradient на выходе
   3. Backward слой за слоём (от последнего к первому)
   
   Возвращает [layer-grads-list, loss].
   НЕ обновляет параметры — это делает оптимизатор."
  [layers batch-x batch-y]
  (let [activations (kan-forward-with-cache layers batch-x)
        predictions (last activations)
        loss (/ (reduce + 0.0
                         (mapcat (fn [p t] (map #(* % %) (map - p t)))
                                 predictions batch-y))
                (* (count predictions) (count (first predictions))))
        upstream (mse-output-gradient predictions batch-y)
        n-layers (count layers)
        [grads-list _]
        (reduce (fn [[grads-acc current-upstream] l-idx]
                  (let [layer    (nth layers l-idx)
                        layer-in (nth activations l-idx)
                        [grads input-deltas]
                        (kan-layer-backward layer (vec layer-in) (vec current-upstream))]
                    [(assoc grads-acc l-idx grads) input-deltas]))
                [(vec (repeat n-layers nil)) upstream]
                (range (dec n-layers) -1 -1))]
    [grads-list loss]))

;; ============================================================
;; FLATTEN / UNFLATTEN ГРАДИЕНТОВ
;; ============================================================

(defn flatten-grads
  "Преобразование структурированных градиентов в плоский вектор.
   Порядок: [dwb | dws | dcoeffs | dgamma | dbeta] — совпадает с extract-params."
  [grads]
  (vec (concat (:dwb-grads grads)
               (:dws-grads grads)
               (mapcat identity (:dcoeff-grads grads))
               (:dgamma grads)
               (:dbeta grads))))

;; ============================================================
;; ADAM ОПТИМИЗАТОР (для backprop-пути)
;; ============================================================

(defn init-adam-state
  "Инициализация состояния Adam для одного KAN-слоя.
   m, v — вектора моментов, по размеру совпадают с flatten-grads."
  [layer]
  (let [n (kan/num-params layer)]
    {:m    (vec (repeat n 0.0))
     :v    (vec (repeat n 0.0))
     :step 0}))

(defn adam-step
  "Один шаг AdamW (Adam + decoupled weight decay).
   
   Loshchilov & Hutter (2019):
   1. m_t = β₁·m_{t-1} + (1-β₁)·g_t
   2. v_t = β₂·v_{t-1} + (1-β₂)·g_t²
   3. m̂_t = m_t / (1 - β₁^t),  v̂_t = v_t / (1 - β₂^t)
   4. θ_t = θ_{t-1} - lr · m̂_t / (√v̂_t + ε) - lr · wd · θ_{t-1}
   
   weight-decay=0.0 → чистый Adam (по умолчанию)
   weight-decay>0   → AdamW (decoupled, улучшает обобщение)
   
   Возвращает [updated-layer, updated-adam-state]."
  [layer grads adam-state lr
   & [{:keys [beta1 beta2 eps max-grad-norm weight-decay]
       :or   {beta1 0.9 beta2 0.999 eps 1e-8
              max-grad-norm 5.0 weight-decay 0.0}}]]
  (let [flat-g (flatten-grads grads)
        ;; Gradient clipping
        max-g  (apply max (map abs flat-g))
        scale  (if (> max-g max-grad-norm)
                 (/ max-grad-norm max-g) 1.0)
        clipped (if (< scale 1.0)
                  (mapv #(* scale %) flat-g) flat-g)
        ;; Adam moments
        t  (inc (:step adam-state))
        m' (mapv (fn [mi gi] (+ (* beta1 mi) (* (- 1.0 beta1) gi)))
                 (:m adam-state) clipped)
        v' (mapv (fn [vi gi] (+ (* beta2 vi) (* (- 1.0 beta2) (* gi gi))))
                 (:v adam-state) clipped)
        m-hat (mapv #(/ % (- 1.0 (math/pow beta1 t))) m')
        v-hat (mapv #(/ % (- 1.0 (math/pow beta2 t))) v')
        ;; Update params: Adam step + decoupled weight decay
        params  (kan/extract-params layer)
        params' (mapv (fn [p mh vh]
                        (let [adam-upd (/ (* lr mh) (+ (math/sqrt vh) eps))
                              wd-upd  (* lr weight-decay p)]
                          (- p adam-upd wd-upd)))
                      params m-hat v-hat)
        layer'  (kan/inject-params layer params')]
    [layer' {:m m' :v v' :step t}]))

;; ============================================================
;; UNIFIED ОБУЧАЮЩИЙ ЦИКЛ (Adam + Backprop + Adaptive Grid + Refinement)
;; ============================================================

(defn train-kan-backprop
  "Полный цикл обучения KAN через аналитический backprop + Adam.
   
   ═══════════════════════════════════════
   КОНФИГУРАЦИЯ
   ═══════════════════════════════════════
   
   layers   — вектор KAN-слоёв [layer1, layer2, ...]
   dataset  — [[input target] ...], каждый input/target — вектор
   lr       — learning rate (рек. 0.001-0.01 для Adam)
   epochs   — число эпох
   
   opts (map):
     :batch-size          — размер мини-батча (nil = full batch)
     :grid-update-every   — обновлять сетку каждые N эпох
     :grid-eps            — blend: 1.0=uniform, 0.0=quantile
     :refine-every        — удвоение G каждые N эпох (nil = никогда)
     :max-grad-norm       — max-norm для gradient clipping
   
   ═══════════════════════════════════════
   АЛГОРИТМ (per epoch)
   ═══════════════════════════════════════
   
   1. Grid refinement (если epoch % refine-every == 0):
      → refine-grid: G → 2G, переинициализация Adam state
   2. Adaptive grid update (если epoch % grid-update-every == 0)
   3. Shuffle → мини-батчи
   4. Для каждого батча:
      a. Forward + Backward (аналитический)
      b. Adam update для каждого слоя
   5. Вычислить и вывести loss
   
   Возвращает обученные KAN-слои."
  [layers dataset lr epochs & [opts]]
  (let [{:keys [batch-size grid-update-every grid-eps
                refine-every max-grad-norm]
         :or   {grid-eps 0.5 max-grad-norm 5.0}} opts
        init-adam-states (mapv init-adam-state layers)]
    (loop [ls     layers
           adams  init-adam-states
           epoch  1]
      (if (> epoch epochs)
        ls
        (let [;; 1. Grid Refinement (G → 2G)
              [ls adams]
              (if (and refine-every (> epoch 1)
                       (zero? (mod (dec epoch) refine-every)))
                (let [refined (mapv kan/refine-grid ls)]
                  (println (str "  ⚡ Grid refined: G="
                               (:grid-size (first ls)) " → "
                               (:grid-size (first refined))))
                  [refined (mapv init-adam-state refined)])
                [ls adams])
              
              ;; 2. Adaptive grid update
              ls
              (if (and grid-update-every (pos? grid-update-every)
                       (zero? (mod (dec epoch) grid-update-every)))
                (let [batch-x (mapv first dataset)]
                  (mapv #(kan/update-grids-from-batch % batch-x
                                                     (or grid-eps 0.5))
                        ls))
                ls)
              
              ;; 3. Мини-батчи
              batches (if batch-size
                        (mapv vec (partition-all batch-size (shuffle dataset)))
                        [(vec dataset)])
              
              ;; 4. Обработка батчей
              [final-ls final-adams total-loss batch-count]
              (reduce
                (fn [[cur-ls cur-adams acc-loss cnt] batch]
                  (let [batch-x (mapv first batch)
                        batch-y (mapv second batch)
                        ;; Backward
                        [grads-list loss]
                        (kan-multi-backward cur-ls batch-x batch-y)
                        ;; Adam update для каждого слоя
                        updated
                        (mapv (fn [layer grads adam-st]
                                (adam-step layer grads adam-st lr
                                          {:max-grad-norm max-grad-norm}))
                              cur-ls grads-list cur-adams)
                        new-ls    (mapv first updated)
                        new-adams (mapv second updated)]
                    [new-ls new-adams (+ acc-loss loss) (inc cnt)]))
                [ls adams 0.0 0]
                batches)
              
              avg-loss (/ total-loss (max 1 batch-count))]
          
          (println (format "  Epoch %d/%d | Loss: %.6f" epoch epochs avg-loss))
          (recur final-ls final-adams (inc epoch)))))))

;; ============================================================
;; ВЕРИФИКАЦИЯ: BACKPROP vs FORWARD-MODE AD
;; ============================================================

(defn verify-backprop-vs-ad
  "Сравнение градиентов из аналитического backprop с forward-mode AD.
   Если совпадают до tolerance — backprop корректен.
   
   Это ключевой тест: AD — эталон (использует дуальные числа),
   backprop — оптимизация (аналитические формулы).
   
   Алгоритм:
   1. Forward pass → prediction
   2. MSE upstream = 2(pred - target) / n_output
   3. kan-layer-backward с upstream → backprop градиенты
   4. kan-gradient (AD) → эталонные градиенты
   5. Сравнение покомпонентно"
  ([layer input target] (verify-backprop-vs-ad layer input target 1e-3))
  ([layer input target tolerance]
   (let [;; Forward pass для вычисления upstream gradient
         pred      (kan/forward-layer layer input)
         n-out     (count pred)
         ;; ∂MSE/∂pred = 2(pred - target) / n
         upstream  (mapv (fn [p t] (/ (* 2.0 (- p t)) n-out))
                        pred target)
         ;; Backprop gradient
         [grads _] (kan-layer-backward layer [input] [upstream])
         bp-flat   (flatten-grads grads)
         ;; AD gradient (из training.clj — эталон)
         ad-grad   ((requiring-resolve 'kan-kat.training/kan-gradient)
                    layer input target)
         errors    (mapv (fn [b a] (abs (- b a))) bp-flat ad-grad)
         max-err   (if (empty? errors) 0.0 (apply max errors))
         n-params  (count bp-flat)]
     (println "╔══════════════════════════════════════════╗")
     (println "║  Backprop vs AD Gradient Verification    ║")
     (println "╚══════════════════════════════════════════╝")
     (println (str "  Parameters: " n-params))
     (println)
     (println "  Param# |   Backprop   |    AD grad   |    Error")
     (println "  -------|--------------|--------------|----------")
     (doseq [i (range (min 20 n-params))]
       (println (format "  %5d  | %12.6f | %12.6f | %.2e"
                        i (nth bp-flat i) (nth ad-grad i) (nth errors i))))
     (when (> n-params 20)
       (println (str "  ... (" (- n-params 20) " more parameters)")))
     (println)
     (println (str "  Max absolute error: " (format "%.2e" max-err)))
     (println (str "  Tolerance:          " (format "%.2e" tolerance)))
     (println (str "  Result:             "
                   (if (<= max-err tolerance) "✅ PASS" "❌ FAIL")))
     (println)
     (<= max-err tolerance))))

