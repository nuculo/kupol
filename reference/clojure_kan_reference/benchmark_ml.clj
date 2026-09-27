(ns kan-kat.benchmark-ml
  (:require [tech.v3.dataset :as ds]
            [tech.v3.dataset.modelling :as ds-mod]
            [scicloj.ml.core :as ml]
            [scicloj.metamorph.ml :as mm]
            [scicloj.ml.smile.regression] ; Loads Smile models (Random Forest)
            [kan-kat.financial-timeseries-kan :as fts]))

(defn prep-techml-dataset
  "Конвертирует `X` (матрица фичей) и `Y` (вектор таргетов) из формата kan-kat 
   в формат tech.v3.dataset.
   - `x-data`: список или вектор векторов фичей.
   - `y-data`: вектор таргетов (соответствует x).
   - `feature-names`: вектор имен фичей (опционально, иначе f1, f2...)"
  [x-data y-data & [feature-names]]
  (let [n-features (count (first x-data))
        f-names (or feature-names (mapv #(str "f" %) (range n-features)))
        ;; Создаем мапу колонок для фичей
        features-map (into {}
                           (for [i (range n-features)]
                             [(keyword (nth f-names i)) (mapv #(nth % i) x-data)]))
        ;; Добавляем таргет колонку
        full-map (assoc features-map :target (vec y-data))]
    (ds/->dataset full-map)))

(defn train-random-forest
  "Обучает Random Forest Regression модель используя scicloj.ml.
   - `train-ds`: датасет tech.v3.dataset.
   Возвращает обученную модель в виде вектора [pipeline-fn fitted-ctx]."
  [train-ds]
  ;; Устанавливаем целевую колонку прямо в данных данных
  (let [train-ds-target (ds-mod/set-inference-target train-ds :target)
        pipe-fn (ml/pipeline
                 (mm/model {:model-type :smile.regression/random-forest}))
        ;; Обучаем, передавая данные и режим :fit в функцию пайплайна
        fitted-ctx (pipe-fn {:metamorph/data train-ds-target
                             :metamorph/mode :fit})]
    [pipe-fn fitted-ctx]))

(defn predict-ml
  "Применяет обученную модель к тестовому датасету.
   Возвращает вектор предсказаний (doubles)."
  [[pipe-fn fitted-ctx] test-ds]
  ;; Применяем на тестовых данных в режиме :transform
  ;; Убедимся, что для предсказания данные тоже размечены если того требует пайплайн
  (let [test-ds-target (ds-mod/set-inference-target test-ds :target)
        pred-ctx (pipe-fn (merge fitted-ctx
                                 {:metamorph/data test-ds-target
                                  :metamorph/mode :transform}))
        pred-ds (if (map? pred-ctx) (:metamorph/data pred-ctx) pred-ctx)
        preds (if pred-ds
                (or (get pred-ds "target")
                    (get pred-ds :target)
                    (get pred-ds "target-predict")
                    (get pred-ds (first (ds/column-names pred-ds)))
                    (vec (repeat (ds/row-count pred-ds) 0.0)))
                (vec (repeat (ds/row-count test-ds) 0.0)))]
    (vec preds)))
