(ns kan-kat.multi-head-kan
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]))

(defn make-multi-head-kan
  "Создаёт архитектуру Multi-Head KAN.
   Вход параллельно обрабатывается `num-heads` независимыми KAN слойями,
   каждый из которых сжимает проекцию до размерности `head-dim`.
   Затем выходы всех голов конкатенируются и проходят через финальный KAN проектор `o-kan`,
   возвращая вектор размерности `out-dim`."
  [num-heads in-dim head-dim out-dim degree]
  (let [heads (vec (for [i (range num-heads)]
                     (fw/make-model [in-dim head-dim] degree)))
        o-kan-input-dim (* num-heads head-dim)
        o-kan (fw/make-model [o-kan-input-dim out-dim] degree)]
    {:heads heads
     :o-kan o-kan
     :num-heads num-heads}))

(defn mhk-forward
  "Прямой проход для Multi-Head KAN слоя.
   Вход `X` подаётся в каждую из голов `H_i`.
   Результаты всех голов H_1, ..., H_N конкатенируются.
   Конкатенированный тензор подаётся в финальную K-проекцию `O`."
  [mhk X]
  (let [heads (:heads mhk)
        ;; 1. Параллельный прямой проезд через все головы
        head-outputs (mapv #(fw/model-forward % X) heads)
        
        ;; 2. Конкатенируем головы по оси признаков 
        ;; Выполняем reduce t-cat для объединения любого числа тензоров
        cat-output (reduce t2/t-cat head-outputs)
        
        ;; 3. Пропускаем через финальный проектор
        final-output (fw/model-forward (:o-kan mhk) cat-output)]
    final-output))

(defn mhk-params
  "Возвращает плоский вектор параметров от всех параллельных голов и финального проектора."
  [mhk]
  (let [heads-params (mapcat fw/all-params (:heads mhk))
        o-params (fw/all-params (:o-kan mhk))]
    (vec (concat heads-params o-params))))

(defn mhk-update
  "Возвращает обновленную модель Multi-Head KAN с применением новых параметров `new-params`.
   Параметры должны быть сгенерированы оптимизатором в том же порядке, что и `mhk-params`."
  [mhk new-params]
  (let [num-heads (:num-heads mhk)
        head-p-count (count (fw/all-params (first (:heads mhk))))
        
        ;; Обновляем каждую голову по очереди
        {updated-heads :heads
         remaining :rem} (reduce (fn [acc _]
                                   (let [n-p (vec (take head-p-count (:rem acc)))
                                         new-rem (drop head-p-count (:rem acc))
                                         old-head (nth (:heads mhk) (count (:heads acc)))
                                         new-head (fw/model-update old-head n-p)]
                                     {:heads (conj (:heads acc) new-head)
                                      :rem new-rem}))
                                 {:heads [] :rem new-params}
                                 (range num-heads))
        ;; Оставшиеся параметры идут в o-kan
        new-o-kan (fw/model-update (:o-kan mhk) (vec remaining))]
    (assoc mhk
           :heads updated-heads
           :o-kan new-o-kan)))

;; ==========================================
;; Demo 42: Multi-Head KAN Function Appx
;; ==========================================

(defn demo-multi-head-kan []
  (println "\n==========================================")
  (println " Demo 42: Multi-Head KAN Function Appx")
  (println "==========================================\n")
  
  (let [NumHeads 4
        Batch 20
        InDim 2
        HeadDim 3
        OutDim 1
        
        ;; Датасет: Target = sin(x_0) + cos(x_1) + x_0*x_1
        x-data (double-array (* Batch InDim))
        y-data (double-array Batch)]
    
    (dotimes [b Batch]
      (let [x0 (* (- (rand) 0.5) 4.0) ; -2 to 2
            x1 (* (- (rand) 0.5) 4.0)]
        (aset x-data (+ (* b InDim) 0) x0)
        (aset x-data (+ (* b InDim) 1) x1)
        (aset y-data b (+ (Math/sin x0) (Math/cos x1) (* x0 x1)))))
        
    (let [X-train (t2/tensor (vec x-data) [Batch InDim])
          Y-train (t2/tensor (vec y-data) [Batch OutDim])
          mhk-model (make-multi-head-kan NumHeads InDim HeadDim OutDim 3)]
          
      (println "Training Multi-Head KAN...")
      (println (format "Parallel Heads: %d | HeadDim: %d | Total Concat Dim: %d" 
                       NumHeads HeadDim (* NumHeads HeadDim)))
                       
      (loop [ep 1
             model mhk-model]
        (if (> ep 50)
          (do
            (println "Training finished.")
            model)
          (let [pred (mhk-forward model X-train)
                loss-t (t2/mse-loss pred Y-train)
                loss-val (aget ^doubles (:data loss-t) 0)]
                
            (t2/backward! loss-t)
            
            (let [all-p (mhk-params model)
                  ;; Апдейт параметров Multi-Head
                  new-all-p (mapv #(t2/sgd-step! % 0.005 1.0) all-p)
                  new-model (mhk-update model new-all-p)]
                  
              (when (zero? (mod ep 10))
                (println (format "  Epoch %3d | Loss: %8.4f" ep loss-val)))
              (recur (inc ep) new-model))))))))
