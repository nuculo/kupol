(ns kan-kat.early-stopping
  "Фаза 44: Early Stopping и кросс-валидация.
   Отслеживает Validation Loss, сохраняет лучшие веса и прерывает обучение, 
   если метрика перестает улучшаться (Patience)."
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]))

;; ============================================================
;; MODEL STATE CLONING
;; ============================================================

(defn clone-params
  "Создает глубокую копию (срез) весов модели."
  [model]
  (let [params (fw/all-params model)]
    (mapv (fn [p]
            (let [n (:numel p)
                  d (double-array n)]
              (System/arraycopy (:data p) 0 d 0 n)
              (t2/tensor (vec d) (:shape p))))
          params)))

;; ============================================================
;; EARLY STOPPING LOGIC
;; ============================================================

(defn make-early-stopper
  "Создает конфигурацию и мутабельное состояние Early Stopping."
  [{:keys [patience min-delta] 
    :or {patience 5 min-delta 0.001}}]
  (atom {:patience patience
         :min-delta min-delta
         :best-loss Double/POSITIVE_INFINITY
         :counter 0
         :best-model-params nil
         :stop? false}))

(defn check-stop!
  "Проверяет validation loss текущей эпохи. 
   Возвращает true если сработал Early Stopping, иначе false.
   При улучшении val-loss автоматически сохраняет веса model в состояние."
  [stopper-atom val-loss model]
  (let [{:keys [patience min-delta best-loss counter]} @stopper-atom]
    (if (< val-loss (- best-loss min-delta))
      ;; Improvement: сбрасываем счетчик, сохраняем loss и веса
      (swap! stopper-atom assoc 
             :best-loss val-loss
             :counter 0
             :best-model-params (clone-params model))
      ;; No improvement: увеличиваем счетчик
      (let [new-counter (inc counter)]
        (swap! stopper-atom assoc :counter new-counter)
        (when (>= new-counter patience)
          (swap! stopper-atom assoc :stop? true))))
    (:stop? @stopper-atom)))

(defn get-best-model
  "Возвращает модель с восстановленными лучшими сохраненными весами.
   Если улучшений не было (например, первая же эпоха провалилась), 
   возвращает модель в текущем состоянии (оригинал)."
  [stopper-atom model]
  (if-let [best-params (:best-model-params @stopper-atom)]
    (fw/model-update model best-params)
    model))

;; ============================================================
;; DEMO
;; ============================================================

(defn generate-split-data
  "Генерирует зашумленный датасет и разбивает его на Train и Val."
  [N val-ratio]
  (let [x-raw (mapv #(vector (float (/ % N))) (range (- N) N))
        ;; Функция: sin(3*x) + шум
        y-raw (mapv #(vector (float (+ (Math/sin (* 3.0 (first %))) 
                                       (* 0.2 (- (Math/random) 0.5))))) ;; Noise
                    x-raw)
        
        ;; Shuffle data to avoid sequential bias
        dataset (shuffle (map vector x-raw y-raw))
        val-size (int (* val-ratio (count dataset)))
        
        val-data (take val-size dataset)
        train-data (drop val-size dataset)
        
        x-train (map first train-data)
        y-train (map second train-data)
        x-val (map first val-data)
        y-val (map second val-data)
        
        train-x-t (t2/tensor (flatten x-train) [(count x-train) 1])
        train-y-t (t2/tensor (flatten y-train) [(count y-train) 1])
        val-x-t (t2/tensor (flatten x-val) [(count x-val) 1])
        val-y-t (t2/tensor (flatten y-val) [(count y-val) 1])]
    
    {:train-x train-x-t :train-y train-y-t
     :val-x val-x-t :val-y val-y-t}))

(defn demo-early-stopping []
  (println "==========================================")
  (println " Demo 37: Early Stopping & Validation")
  (println "==========================================\n")
  
  (let [data-split (generate-split-data 100 0.2)
        xt (:train-x data-split)
        yt (:train-y data-split)
        xv (:val-x data-split)
        yv (:val-y data-split)
        
        model (fw/make-model [1 5 1] 4)
        lr 0.02
        epochs 1000
        
        ;; Stopper: patience = 30, т.е. ждём 30 эпох без улучшения
        stopper (make-early-stopper {:patience 30 :min-delta 0.0001})
        
        _ (println (format "Dataset: %d Train, %d Validation" (first (:shape xt)) (first (:shape xv))))
        _ (println "Training with patience=30...")
        
        final-model
        (loop [ep 1
               m model]
          (if (> ep epochs)
            (do (println "  Reached max epochs!") m)
            (let [;; --- TRAINING PASS ---
                  pred-t (fw/model-forward m xt)
                  loss-t-raw (t2/mse-loss pred-t yt)
                  loss-t-val (aget ^doubles (:data loss-t-raw) 0)]
              
              (t2/backward! loss-t-raw)
              
              (let [new-p (mapv #(t2/sgd-step! % lr 5.0) (fw/all-params m))
                    new-m (fw/model-update m new-p)
                    
                    ;; --- VALIDATION PASS ---
                    pred-v (fw/model-forward new-m xv)
                    loss-v-raw (t2/mse-loss pred-v yv)
                    loss-v-val (aget ^doubles (:data loss-v-raw) 0)]
                
                (when (zero? (mod ep 10))
                  (println (format "  Epoch %3d | Train Loss: %8.4f | Val Loss: %8.4f" 
                                   ep loss-t-val loss-v-val)))
                
                ;; Check early stopping
                (if (check-stop! stopper loss-v-val new-m)
                  (do
                    (println (format "\n=> ✨ Early Stopping Triggered at Epoch %d!" ep))
                    (println (format "   No improvement on Val Loss for %d epochs." (:patience @stopper)))
                    (println (format "   Best Validation Loss was: %.4f" (:best-loss @stopper)))
                    new-m)
                  ;; Continue training
                  (recur (inc ep) new-m))))))
        
        ;; Извлекаем лучшую модель
        best-model (get-best-model stopper final-model)
        
        ;; Сравниваем лосс лучшей модели и последней модели на валидации
        pred-final-v (fw/model-forward final-model xv)
        loss-final-v (aget ^doubles (:data (t2/mse-loss pred-final-v yv)) 0)
        
        pred-best-v (fw/model-forward best-model xv)
        loss-best-v (aget ^doubles (:data (t2/mse-loss pred-best-v yv)) 0)]
        
    (println "\nModel Checkpoint Restored.")
    (println (format "  Final Epoch Val Loss: %.5f" loss-final-v))
    (println (format "  Best Saved Val Loss:  %.5f" loss-best-v))
    
    (if (< loss-best-v loss-final-v)
      (println "  ✅ Overfitting prevented! Best model successfully rescued.")
      (println "  ℹ️ Model didn't overfit much after the best epoch."))
    (println)))
