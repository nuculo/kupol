(ns kan-kat.evolution
  "Эволюционная оптимизация φ-функций для KAN.
   
   ═══════════════════════════════════════════════════
   ИДЕЯ
   ═══════════════════════════════════════════════════
   
   Градиентный спуск оптимизирует ЧИСЛА (параметры).
   Эволюция оптимизирует СТРУКТУРУ (тип φ).
   
   Благодаря PhiFunction protocol в одной популяции
   живут BSplinePhi, PolyPhi, RationalPhi — и тип
   может МУТИРОВАТЬ между поколениями.
   
   ═══════════════════════════════════════════════════
   АЛГОРИТМ
   ═══════════════════════════════════════════════════
   
   1. Инициализация: N случайных φ разных типов
   2. Оценка: fitness = -MSE на обучающих данных
   3. Селекция: турнирная (top-k из случайной тройки)
   4. Скрещивание: blend параметров двух родителей
   5. Мутация: 
      a) параметрическая — шум к coeffs (80% случаев)
      b) структурная — смена типа φ (20% случаев)
   6. Элитизм: лучшая особь всегда выживает
   ═══════════════════════════════════════════════════"
  (:require [clojure.math :as math]
            [kan-kat.phi-protocol :as phi]))

;; ============================================================
;; ТИПЫ φ ДЛЯ ЭВОЛЮЦИИ
;; ============================================================

(def phi-types
  "Доступные типы φ с опциями."
  [[:bspline  {:order 3 :grid-size 5 :grid-range [-3.0 3.0]}]
   [:bspline  {:order 3 :grid-size 8 :grid-range [-3.0 3.0]}]
   [:poly     {:degree 3}]
   [:poly     {:degree 5}]
   [:rational {:p-deg 3 :q-deg 2}]
   [:rational {:p-deg 4 :q-deg 3}]])

;; ============================================================
;; ИНИЦИАЛИЗАЦИЯ ПОПУЛЯЦИИ
;; ============================================================

(defn random-individual
  "Создаёт случайную особь (φ-функцию случайного типа)."
  []
  (let [[phi-type opts] (rand-nth phi-types)]
    {:phi  (phi/make-phi phi-type opts)
     :type phi-type
     :opts opts}))

(defn init-population
  "Создаёт популяцию из n случайных особей."
  [n]
  (mapv (fn [_] (random-individual)) (range n)))

;; ============================================================
;; FITNESS (оценка качества)
;; ============================================================

(defn evaluate-fitness
  "Оценка fitness особи: -MSE на обучающих данных.
   data = [[x y] ...], φ приближает y ≈ a·φ(x)+b.
   
   Используем LSQ fit (как в symbolic.clj) для fair comparison."
  [individual data]
  (let [f (fn [x] (phi/phi-forward (:phi individual) x))
        n (count data)
        ;; LSQ: y ≈ a·φ(x)+b
        fxs (mapv (fn [[x _]] (f x)) data)
        ys  (mapv second data)
        sum-f  (reduce + 0.0 fxs)
        sum-y  (reduce + 0.0 ys)
        sum-ff (reduce + 0.0 (map * fxs fxs))
        sum-fy (reduce + 0.0 (map * fxs ys))
        det   (- (* sum-ff n) (* sum-f sum-f))
        a     (if (< (abs det) 1e-12) 1.0
                (/ (- (* n sum-fy) (* sum-f sum-y)) det))
        b     (if (< (abs det) 1e-12) (/ sum-y n)
                (/ (- (* sum-ff sum-y) (* sum-f sum-fy)) det))
        ;; MSE
        mse   (/ (reduce + 0.0
                   (map (fn [[xi yi]]
                          (let [pred (+ (* a (f xi)) b)
                                err  (- yi pred)]
                            (* err err)))
                        data))
                 n)]
    (assoc individual :fitness (- mse) :mse mse :a a :b b)))

;; ============================================================
;; СЕЛЕКЦИЯ (турнирная)
;; ============================================================

(defn tournament-select
  "Турнирная селекция: берём k случайных, выбираем лучшего."
  [population k]
  (let [contestants (take k (shuffle population))]
    (apply max-key :fitness contestants)))

;; ============================================================
;; СКРЕЩИВАНИЕ (blend параметров)
;; ============================================================

(defn crossover
  "Скрещивание двух особей одного типа: blend параметров.
   Если типы разные — возвращаем лучшего родителя."
  [parent1 parent2]
  (if (= (:type parent1) (:type parent2))
    (let [p1 (phi/phi-params (:phi parent1))
          p2 (phi/phi-params (:phi parent2))
          ;; Blend: child = α·p1 + (1-α)·p2
          alpha (+ 0.3 (* 0.4 (rand)))
          child-params (mapv (fn [a b] (+ (* alpha a) (* (- 1.0 alpha) b)))
                             p1 p2)
          child-phi (phi/phi-update (:phi parent1) child-params)]
      {:phi child-phi :type (:type parent1) :opts (:opts parent1)})
    ;; Разные типы: берём лучшего
    (if (> (:fitness parent1) (:fitness parent2)) parent1 parent2)))

;; ============================================================
;; МУТАЦИЯ
;; ============================================================

(defn mutate-params
  "Параметрическая мутация: добавляем Gaussian шум."
  [individual sigma]
  (let [params (phi/phi-params (:phi individual))
        noisy  (mapv (fn [p] (+ p (* sigma (- (rand) 0.5) 2.0))) params)
        new-phi (phi/phi-update (:phi individual) noisy)]
    (assoc individual :phi new-phi)))

(defn mutate-type
  "Структурная мутация: полная смена типа φ!"
  [_individual]
  (random-individual))

(defn mutate
  "Мутация: 80% параметрическая, 20% структурная."
  [individual sigma]
  (if (< (rand) 0.2)
    (mutate-type individual)
    (mutate-params individual sigma)))

;; ============================================================
;; ЭВОЛЮЦИОННЫЙ ЦИКЛ
;; ============================================================

(defn evolve
  "Эволюция популяции φ для приближения target function.
   
   data         — [[x y] ...] обучающие данные
   pop-size     — размер популяции (рек. 20-30)
   generations  — число поколений (рек. 30-50)
   sigma        — сила мутации (рек. 0.1-0.3)
   
   Возвращает лучшую особь."
  [data pop-size generations sigma]
  (loop [pop  (init-population pop-size)
         gen  1]
    (if (> gen generations)
      ;; Финальная оценка
      (let [evaluated (mapv #(evaluate-fitness % data) pop)]
        (apply max-key :fitness evaluated))
      ;; Эволюционный шаг
      (let [;; 1. Оценка
            evaluated (mapv #(evaluate-fitness % data) pop)
            ;; 2. Элитизм: лучшая особь
            elite (apply max-key :fitness evaluated)
            ;; 3. Новое поколение
            new-pop
            (into [elite] ;; элитизм: лучшая проходит без изменений
                  (map (fn [_]
                         (let [p1 (tournament-select evaluated 3)
                               p2 (tournament-select evaluated 3)
                               child (crossover p1 p2)]
                           (mutate child sigma)))
                       (range (dec pop-size))))]
        (when (zero? (mod gen 5))
          (println (format "  Gen %3d | Best: %-10s | MSE: %.6f | a=%.3f b=%.3f"
                           gen (name (:type elite))
                           (:mse elite) (:a elite) (:b elite))))
        (recur new-pop (inc gen))))))

;; ============================================================
;; УДОБНАЯ ОБЁРТКА
;; ============================================================

(defn evolve-for-function
  "Эволюция φ для приближения f(x) на интервале [lo, hi].
   
   f           — целевая функция
   [lo hi]     — интервал
   n-samples   — количество точек
   pop-size    — размер популяции
   generations — поколения
   
   Возвращает лучшую особь с формулой."
  [f [lo hi] n-samples pop-size generations]
  (let [data (mapv (fn [_]
                     (let [x (+ lo (* (rand) (- hi lo)))]
                       [x (f x)]))
                   (range n-samples))
        best (evolve data pop-size generations 0.2)]
    (println)
    (println "╔══════════════════════════════════════════╗")
    (println "║  Evolution Result                        ║")
    (println "╚══════════════════════════════════════════╝")
    (println (str "  Winner:  " (name (:type best))))
    (println (format "  Formula: %.4f·φ(x) + %.4f" (:a best) (:b best)))
    (println (format "  MSE:     %.6f" (:mse best)))
    (println (format "  Params:  %d" (count (phi/phi-params (:phi best)))))
    (println)
    best))
