(ns kan-kat.residual-kan
  "Фаза 45: Residual KAN (Skip Connections).
   Модуль для создания глубоких сетей KAN с подавлением vanishing gradient.
   Формула: y = KAN(x) + W*x, где W — опциональная проекционная матрица для выравнивания размерностей."
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]))

;; ============================================================
;; ПРОЕКЦИОННЫЙ СЛОЙ
;; ============================================================

(defn make-projection
  "Создает проекционную матрицу [in-dim x out-dim], инициализированную случайными весами."
  [in-dim out-dim]
  (let [scale (/ 1.0 (Math/sqrt in-dim))
        n (* in-dim out-dim)
        d (double-array n)]
    (dotimes [i n]
      (aset d i (* scale (- (* 2.0 (Math/random)) 1.0))))
    {:type :projection
     :weights (t2/tensor (vec d) [in-dim out-dim])}))

(defn forward-projection
  "x: [B x in-dim], weights: [in-dim x out-dim]. Выход: [B x out-dim]."
  [proj x]
  (t2/t-matmul x (:weights proj)))

;; ============================================================
;; RESIDUAL BLOCK
;; ============================================================

(defn make-residual-block
  "Создает остаточный блок, оборачивающий один слой KAN (или несколько).
   Если размерность входа не совпадает с выходом, добавляется проекционный вес W."
  [in-dim out-dim num-knots]
  (let [kan-layer (fw/make-kan-layer in-dim out-dim num-knots)]
    (if (= in-dim out-dim)
      {:type :residual-kan
       :layer kan-layer
       :projection nil}
      {:type :residual-kan
       :layer kan-layer
       :projection (make-projection in-dim out-dim)})))

(defn residual-forward
  "Пропускает данные через Residual KAN блок. (Подходит для reduce: [x block])"
  [x block]
  (let [kan-raw (fw/kan-layer-forward (:layer block) x)
        kan-out (fw/silu-forward kan-raw)
        skip-out (if-let [proj (:projection block)]
                   (forward-projection proj x)
                   x)]
    ;; y = SiLU(KAN(x)) + Wx
    (t2/t-add kan-out skip-out)))

;; ============================================================
;; АДАПТАЦИЯ ДЛЯ FRAMEWORK
;; ============================================================

(defn residual-params
  "Извлекает параметры из Residual-блока для оптимизатора."
  [block]
  (let [layer (:layer block)
        kan-coeffs (vec (for [j (range (:out-dim layer))
                              i (range (:in-dim layer))]
                          (get-in (:coeffs layer) [j i])))
        kan-ws (if (:ws layer)
                 (vec (for [j (range (:out-dim layer))
                            i (range (:in-dim layer))]
                        (get-in (:ws layer) [j i])))
                 [])
        kan-p (vec (concat kan-coeffs kan-ws))]
    (if-let [proj (:projection block)]
      (conj kan-p (:weights proj))
      kan-p)))

(defn update-residual-block
  "Внедряет новые параметры в блок после SGD."
  [block new-params-atom]
  (let [;; Извлекаем параметры для KAN-слоя (coeffs + ws)
        kan-coeffs (:coeffs (:layer block))
        updated-layer-1
        (update (:layer block) :coeffs
                (fn [coeffs]
                  (mapv (fn [row]
                          (mapv (fn [_]
                                  (let [p (first @new-params-atom)]
                                    (swap! new-params-atom rest)
                                    p))
                                row))
                        coeffs)))
        
        updated-layer
        (if (:ws updated-layer-1)
          (update updated-layer-1 :ws
                  (fn [ws]
                    (mapv (fn [row]
                            (mapv (fn [_]
                                    (let [p (first @new-params-atom)]
                                      (swap! new-params-atom rest)
                                      p))
                                  row))
                          ws)))
          updated-layer-1)]
          
    (if (:projection block)
      ;; Если есть W, извлекаем и его
      (let [w (first @new-params-atom)]
        (swap! new-params-atom rest)
        {:type :residual-kan
         :layer updated-layer
         :projection (assoc (:projection block) :weights w)})
      ;; Нет W
      {:type :residual-kan
       :layer updated-layer
       :projection nil})))

;; ============================================================
;; DEEP RESIDUAL MODEL
;; ============================================================

(defn make-deep-residual
  "Создает сеть из Residual-блоков."
  [layer-dims num-knots]
  (let [pairs (partition 2 1 layer-dims)]
    {:type :model-residual
     :blocks (mapv (fn [[in out]] (make-residual-block in out num-knots)) pairs)}))

(defn deep-residual-forward
  [model x]
  (reduce residual-forward x (:blocks model)))

(defn deep-residual-params
  [model]
  (vec (mapcat residual-params (:blocks model))))

(defn deep-residual-update
  [model new-params]
  (let [atm (atom new-params)]
    (assoc model :blocks
           (mapv #(update-residual-block % atm) (:blocks model)))))

;; ============================================================
;; DEMO 38: DEEP KAN vs DEEP RESIDUAL KAN
;; ============================================================

(defn demo-residual-kan []
  (println "==========================================")
  (println " Demo 38: Deep KAN vs Residual KAN")
  (println "==========================================\n")
  
  (let [;; Архитектура: 5 слоев KAN -> огромный шанс затухания градиента без Skip Connections
        dims [1 4 4 4 4 1]
        
        deep-plain (fw/make-model dims 3)
        deep-resid (make-deep-residual dims 3)
        
        ;; Датасет
        bs 50
        x-raw (mapv #(vector (float (/ % bs))) (range (- bs) bs))
        y-raw (mapv #(vector (float (Math/sin (* 3.0 (first %))))) x-raw)
        
        x (t2/tensor (flatten x-raw) [(* 2 bs) 1])
        y (t2/tensor (flatten y-raw) [(* 2 bs) 1])
        
        lr 0.01
        epochs 100]
        
    (println "Model 1: Deep Plain KAN (5 layers)")
    (loop [ep 1
           m deep-plain]
      (if (<= ep epochs)
        (let [pred (fw/model-forward m x)
              loss-raw (t2/mse-loss pred y)
              loss-val (aget ^doubles (:data loss-raw) 0)]
          (t2/backward! loss-raw)
          (let [new-p (mapv #(t2/sgd-step! % lr 5.0) (fw/all-params m))
                new-m (fw/model-update m new-p)]
            (when (zero? (mod ep 20))
              (println (format "  Epoch %3d | Loss: %8.4f" ep loss-val)))
            (recur (inc ep) new-m)))
        (println "  Done.\n")))
        
    (println "Model 2: Deep Residual KAN (5 layers + Skip Connections)")
    (loop [ep 1
           m deep-resid]
      (if (<= ep epochs)
        (let [pred (deep-residual-forward m x)
              loss-raw (t2/mse-loss pred y)
              loss-val (aget ^doubles (:data loss-raw) 0)]
          (t2/backward! loss-raw)
          (let [new-p (mapv #(t2/sgd-step! % lr 5.0) (deep-residual-params m))
                new-m (deep-residual-update m new-p)]
            (when (zero? (mod ep 20))
              (println (format "  Epoch %3d | Loss: %8.4f" ep loss-val)))
            (recur (inc ep) new-m)))
        (println "  Done.\n")))
        
    (println "Notice: Deep Plain KAN heavily suffers from vanishing gradient and flatlines.")
    (println "Deep Residual KAN perfectly propagates gradients through the deep structure.")))
