(ns kan-kat.classification-kan
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]
            [kan-kat.data-loader :as dl]))

;; ==========================================
;; Non-linear Datasets
;; ==========================================

(defn generate-circles [n noise]
  "Генерирует два концентрических круга для задачи бинарной классификации (0 и 1).
   Нелинейно-разделимая задача."
  (let [n-out (/ n 2)
        n-in (- n n-out)
        out-pts (vec (for [_ (range n-out)]
                       (let [angle (* (rand) 2.0 Math/PI)
                             r (+ 1.0 (* (rand) noise) (- (/ noise 2.0)))]
                         {"x" (* r (Math/cos angle))
                          "y" (* r (Math/sin angle))
                          "label" 0.0})))
        in-pts (vec (for [_ (range n-in)]
                      (let [angle (* (rand) 2.0 Math/PI)
                            r (+ 0.3 (* (rand) noise) (- (/ noise 2.0)))]
                        {"x" (* r (Math/cos angle))
                         "y" (* r (Math/sin angle))
                         "label" 1.0})))]
    (shuffle (into out-pts in-pts))))

;; ==========================================
;; Classification Metrics
;; ==========================================

(defn argmax [logits]
  "Возвращает индекс максимального логита вдоль оси классов."
  (let [N (nth (:shape logits) 0)
        C (nth (:shape logits) 1)
        ^doubles ldata (:data logits)
        preds (double-array N)]
    (dotimes [i N]
      (let [idx (loop [j 0 max-i 0 mx (- Double/MAX_VALUE)]
                  (if (= j C)
                    max-i
                    (let [v (aget ldata (+ (* i C) j))]
                      (if (> v mx)
                        (recur (inc j) j v)
                        (recur (inc j) max-i mx)))))]
        (aset preds i (double idx))))
    preds))

(defn accuracy [logits targets]
  "Считает долю правильных ответов (Accuracy)."
  (let [N (nth (:shape logits) 0)
        p-array (argmax logits)
        ^doubles t-array (:data targets)
        correct (loop [i 0 c 0]
                  (if (= i N)
                    c
                    (if (= (int (aget p-array i)) (int (aget t-array i)))
                      (recur (inc i) (inc c))
                      (recur (inc i) c))))]
    (/ (double correct) N)))

;; ==========================================
;; Demo 45: Classification Suite
;; ==========================================

(defn demo-classification-kan []
  (println "\n==========================================")
  (println " Demo 45: Classification KAN (Circles)")
  (println "==========================================\n")
  
  (let [N 2000
        dataset (generate-circles N 0.15)
        splits (dl/train-test-split dataset 0.2 true)
        train-data (:train splits)
        test-data (:test splits)
        
        features ["x" "y"]
        targets ["label"]
        
        ;; Обучение масштабатора только на Training Features
        x-scaler (dl/fit-minmax train-data features)
        
        ;; Лейблы масштабировать нельзя (0 и 1), поэтому Y-scaler не применяется
        train-x (dl/transform-minmax x-scaler train-data features)
        test-x (dl/transform-minmax x-scaler test-data features)
        
        X-train (dl/to-tensor train-x features)
        Y-train (dl/to-tensor train-data targets)
        X-test (dl/to-tensor test-x features)
        Y-test (dl/to-tensor test-data targets)
        
        ;; Архитектура KAN: 2D Вход -> 5 Скрытых -> 2 Класса Выход
        model (fw/make-model [2 5 2] 3)
        lr 0.05
        epochs 150]
        
    (println (format "Training KAN on Concentric Circles | Train: %d | Test: %d" (count train-data) (count test-data)))
    (loop [ep 1
           curr-model model]
      (if (> ep epochs)
        curr-model
        (let [;; Форвард проход
              logits-train (fw/model-forward curr-model X-train)
              loss-t-train (t2/t-cross-entropy logits-train Y-train)
              
              ;; Форвард тест
              logits-test (fw/model-forward curr-model X-test)
              loss-t-test (t2/t-cross-entropy logits-test Y-test)
              
              ;; Метрики
              acc-train (accuracy logits-train Y-train)
              acc-test (accuracy logits-test Y-test)
              
              loss-val-train (aget ^doubles (:data loss-t-train) 0)
              loss-val-test (aget ^doubles (:data loss-t-test) 0)]
              
          ;; Обратный проход по кросс-энтропии
          (t2/backward! loss-t-train)
          (let [params (fw/all-params curr-model)
                new-params (mapv #(t2/sgd-step! % lr 5.0) params)
                new-model (fw/model-update curr-model new-params)]
                
            (when (zero? (mod ep 25))
              (println (format "  Epoch %3d | Train Loss: %.4f (Acc: %5.1f%%) | Test Loss: %.4f (Acc: %5.1f%%)" 
                               ep loss-val-train (* 100.0 acc-train) loss-val-test (* 100.0 acc-test))))
            (recur (inc ep) new-model)))))))
