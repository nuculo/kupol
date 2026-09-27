(ns kan-kat.learning-rate-finder
  "Фаза 43: Learning Rate Finder (Алгоритм Лесли Смита).
   Находит оптимальный Learning Rate, постепенно увеличивая его во время прогона батчей,
   и анализируя кривую сглаженного Loss для нахождения точки наибольшего падения."
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]))

;; ============================================================
;; MODEL STATE BACKUP & RESTORE
;; ============================================================

(defn backup-model
  "Создает глубокую копию (срез) весов модели, чтобы 
   Finder мог вернуть модель в исходное состояние."
  [model]
  (let [params (fw/all-params model)]
    (mapv (fn [p]
            (let [n (:numel p)
                  d (double-array n)]
              (System/arraycopy (:data p) 0 d 0 n)
              (t2/tensor (vec d) (:shape p))))
          params)))

(defn restore-model
  "Восстанавливает модель из ранее сохраненных параметрических копий."
  [model backup-params]
  (fw/model-update model backup-params))

;; ============================================================
;; LEARNING RATE SWEEP
;; ============================================================

(defn- calc-smoothed-loss [beta current-loss prev-loss]
  (if (nil? prev-loss)
    current-loss
    (+ (* beta prev-loss) (* (- 1.0 beta) current-loss))))

(defn lr-sweep
  "Экспоненциально увеличивает LR от min-lr до max-lr на протяжении steps шагов.
   Возвращает вектор мап: [{:lr ... :loss ... :smoothed-loss ...} ...]."
  [model x-batch y-batch min-lr max-lr steps]
  (let [bs (first (:shape x-batch))
        ;; Чтобы было достаточно шагов, мы просто имитируем случайные "микро-батчи" 
        ;; из полноразмерного x-batch для каждого шага SGD.
        in-dim (last (:shape x-batch))
        out-dim (last (:shape y-batch))
        
        ^doubles xd (:data x-batch)
        ^doubles yd (:data y-batch)
        
        mult (Math/pow (/ max-lr min-lr) (/ 1.0 (dec steps)))
        beta 0.98]
        
    (loop [step 0
           lr min-lr
           smoothed-loss nil
           best-loss Double/POSITIVE_INFINITY
           m model
           results []]
      (if (>= step steps)
        results
        (let [;; берем 1 случайный элемент или небольшой батч 
              ;; Для простоты демо берем весь x-batch целиком, так как он обычно невелик.
              ;; Но правильнее брать мини-батчи. 
              ;; В этом демо, будем считать, что x-batch и y-batch — это уже выборка (b=32..64)
              
              pred (fw/model-forward m x-batch)
              mseloss (t2/mse-loss pred y-batch)
              loss-val (aget ^doubles (:data mseloss) 0)]
          
          (t2/backward! mseloss)
          
          (let [smoothed (calc-smoothed-loss beta loss-val smoothed-loss)
                ;; Bias correction: smoothed / (1 - beta^(step+1))
                smoothed-corrected (/ smoothed (- 1.0 (Math/pow beta (inc step))))
                
                new-best-loss (min best-loss smoothed-corrected)
                
                ;; Проверка на расхождение (взрыв)
                explode? (or (Double/isNaN loss-val) 
                             (> smoothed-corrected (* 4.0 new-best-loss)))
                
                new-results (conj results {:lr lr 
                                           :loss loss-val 
                                           :smoothed-loss smoothed-corrected})]
                                           
            (if explode?
              new-results
              (let [;; SGD step
                    new-p (mapv #(t2/sgd-step! % lr 5.0) (fw/all-params m))
                    new-m (fw/model-update m new-p)]
                (recur (inc step) (* lr mult) smoothed new-best-loss new-m new-results)))))))))

;; ============================================================
;; ANALYSIS
;; ============================================================

(defn suggest-lr
  "Анализирует профиль Loss vs LR и ищет точку 'steepest descent'.
   Возвращает значение LR, которое находится на самом крутом участке."
  [history]
  (let [;; Считаем производные (разницы) Loss по отношению к log(LR)
        ;; steepness = (L_{i} - L_{i-1}) / (log(LR_{i}) - log(LR_{i-1}))
        with-steepness
        (map-indexed 
         (fn [i h]
           (if (zero? i)
             (assoc h :steepness 0.0)
             (let [prev (nth history (dec i))
                   dLoss (- (:smoothed-loss h) (:smoothed-loss prev))
                   dLogLr (- (Math/log10 (:lr h)) (Math/log10 (:lr prev)))]
               (assoc h :steepness (/ dLoss dLogLr)))))
         history)
         
        ;; Мы ищем самое отрицательное steepness (наибольшее падение)
        ;; Исключая самые первые шумные шаги и самые последние
        valid-candidates (drop 15 with-steepness)
        best-point (apply min-key :steepness valid-candidates)]
    
    (:lr best-point)))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-lr-finder []
  (println "==========================================")
  (println " Demo 36: Learning Rate Finder")
  (println "    (Leslie Smith Algorithm)    ")
  (println "==========================================\n")
  
  (let [;; Синтетический датасет
        bs 50
        x-raw (mapv #(vector (float (/ % bs))) (range (- bs) bs))
        y-raw (mapv #(vector (float (+ (Math/sin (first %)) 0.5))) x-raw)
        
        x (t2/tensor (flatten x-raw) [(* 2 bs) 1])
        y (t2/tensor (flatten y-raw) [(* 2 bs) 1])
        
        model (fw/make-model [1 3 1] 3)
        backup (backup-model model)
        
        ;; Запускаем Sweep от 1e-4 до 10.0
        min-lr 1e-4
        max-lr 10.0
        steps 100
        
        _ (println (format "Running LR Sweep [%.1e, %.1f] (%d steps)..." min-lr max-lr steps))
        history (lr-sweep model x y min-lr max-lr steps)
        
        optimal-lr (suggest-lr history)]
        
    (println (format "\nDone! Sweep stopped after %d steps." (count history)))
    (println "Sample of Loss trajectory:")
    (doseq [i (range 0 (count history) (quot (count history) 10))]
      (when-let [h (nth history i nil)]
        (println (format "  LR: %8.2e | Loss: %8.4f" (:lr h) (:smoothed-loss h)))))
        
    (println (format "\n=> Suggested Optimal LR (steepest descent): %.4f" optimal-lr))
    
    ;; Восстанавливаем модель к первоначальному состоянию
    (let [restored-model (restore-model model backup)]
      (println "Model weights successfully restored to pre-sweep baseline."))
    (println)))
