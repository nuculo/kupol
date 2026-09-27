(ns kan-kat.regression-bench
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]
            [kan-kat.data-loader :as dl]
            [clojure.java.io :as io]))

;; ==========================================
;; Benchmark Functions (2D -> 1D)
;; ==========================================

(defn rosenbrock [x y]
  ;; f(x,y) = (1-x)^2 + 100(y - x^2)^2
  ;; Global min: (1, 1) -> 0
  (+ (Math/pow (- 1.0 x) 2)
     (* 100.0 (Math/pow (- y (* x x)) 2))))

(defn rastrigin [x y]
  ;; f(x) = 10d + sum(x_i^2 - 10cos(2*pi*x_i))
  ;; Global min: (0, 0) -> 0
  (+ 20.0
     (- (* x x) (* 10.0 (Math/cos (* 2.0 Math/PI x))))
     (- (* y y) (* 10.0 (Math/cos (* 2.0 Math/PI y))))))

(defn ackley [x y]
  ;; Global min: (0, 0) -> 0
  (let [part1 (* -0.2 (Math/sqrt (* 0.5 (+ (* x x) (* y y)))))
        part2 (* 0.5 (+ (Math/cos (* 2.0 Math/PI x)) (Math/cos (* 2.0 Math/PI y))))]
    (+ (- (* -20.0 (Math/exp part1)))
       (- (Math/exp part2))
       20.0
       Math/E)))

(defn himmelblau [x y]
  ;; f(x,y) = (x^2 + y - 11)^2 + (x + y^2 - 7)^2
  ;; Four identical local minima -> 0
  (+ (Math/pow (- (+ (* x x) y) 11.0) 2)
     (Math/pow (- (+ x (* y y)) 7.0) 2)))

;; ==========================================
;; Data Generation
;; ==========================================

(defn generate-benchmark-dataset
  "Генерирует N случайных точек в кубе [min-val, max-val]x[min-val, max-val]
   и вычисляет значение функции f.
   Возвращает список словарей для data_loader."
  [f N min-val max-val]
  (let [range-val (- max-val min-val)]
    (vec (for [_ (range N)]
           (let [x (+ min-val (* (rand) range-val))
                 y (+ min-val (* (rand) range-val))
                 z (f x y)]
             {"x" x "y" y "z" z})))))

(defn train-and-evaluate-kan
  "Полный пайплайн обучения KAN на сгенерированном датасете `dataset`."
  [dataset name epochs lr]
  (let [;; 1. Разбиение (80/20)
        splits (dl/train-test-split dataset 0.2 true)
        train-data (:train splits)
        test-data (:test splits)
        
        ;; 2. Обучаем Scaler только на Train для X и Z
        features ["x" "y"]
        targets ["z"]
        x-scaler (dl/fit-minmax train-data features)
        y-scaler (dl/fit-minmax train-data targets)
        
        ;; 3. Трансформируем Train и Test 
        train-x-map (dl/transform-minmax x-scaler train-data features)
        train-y-map (dl/transform-minmax y-scaler train-data targets)
        test-x-map (dl/transform-minmax x-scaler test-data features)
        test-y-map (dl/transform-minmax y-scaler test-data targets)
        
        ;; 4. Конвертируем в tensor_v2
        X-train (dl/to-tensor train-x-map features)
        Y-train (dl/to-tensor train-y-map targets)
        X-test (dl/to-tensor test-x-map features)
        Y-test (dl/to-tensor test-y-map targets)
        
        ;; 5. Инициализация KAN [2 -> 4 -> 1]
        model (fw/make-model [2 4 1] 3)]
        
    (println "\n  -- Benchmark:" name "--")
    (loop [ep 1
           curr-model model]
      (if (> ep epochs)
        curr-model
        (let [;; Forward Train
              pred-train (fw/model-forward curr-model X-train)
              loss-t-train (t2/mse-loss pred-train Y-train)
              
              ;; Forward Test (Без autograd)
              pred-test (fw/model-forward curr-model X-test)
              loss-t-test (t2/mse-loss pred-test Y-test)
              
              loss-val-train (aget ^doubles (:data loss-t-train) 0)
              loss-val-test (aget ^doubles (:data loss-t-test) 0)]
              
          ;; Backward & Update только по Train
          (t2/backward! loss-t-train)
          (let [params (fw/all-params curr-model)
                new-params (mapv #(t2/sgd-step! % lr 5.0) params)
                new-model (fw/model-update curr-model new-params)]
                
            (when (zero? (mod ep 20))
              (println (format "    Epoch %3d | Train Loss: %8.4f | Test Loss: %8.4f" 
                               ep loss-val-train loss-val-test)))
            (recur (inc ep) new-model)))))))

;; ==========================================
;; Demo 44: Benchmarking Suite
;; ==========================================

(defn demo-regression-bench []
  (println "\n==========================================")
  (println " Demo 44: Regression Benchmark")
  (println "==========================================\n")
  
  (let [N 2000
        Epochs 150
        LR 0.05]
        
    (println "Generating datasets (2000 points each)...")
    ;; Rosenbrock range is typically narrow due to rapid scaling
    (let [rosen-data (generate-benchmark-dataset rosenbrock N -2.0 2.0)
          rastr-data (generate-benchmark-dataset rastrigin N -5.12 5.12)
          ackle-data (generate-benchmark-dataset ackley N -5.0 5.0)
          himme-data (generate-benchmark-dataset himmelblau N -5.0 5.0)]
          
      (train-and-evaluate-kan rosen-data "Rosenbrock (Valley)" Epochs LR)
      (train-and-evaluate-kan rastr-data "Rastrigin (Local Minima)" Epochs LR)
      (train-and-evaluate-kan ackle-data "Ackley (Flat outer, deep hole)" Epochs LR)
      (train-and-evaluate-kan himme-data "Himmelblau (4 Global Minima)" Epochs LR))))
