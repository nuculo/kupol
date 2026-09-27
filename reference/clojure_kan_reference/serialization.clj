(ns kan-kat.serialization
  "Сериализация KAN моделей: save/load/checkpoint.
   
   ═══════════════════════════════════════════════════
   Фаза 39: Persistence для KAN Framework
   
   - save-model → EDN (Clojure native)
   - load-model → восстановление модели
   - Checkpoint каждые N эпох
   - Resume training с checkpoint
   ═══════════════════════════════════════════════════"
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as kf]
            [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.math :as math])
  (:import [java.time LocalDateTime]
           [java.time.format DateTimeFormatter]))

;; ============================================================
;; MODEL → EDN (serializable representation)
;; ============================================================

(defn model->edn
  "Конвертирует модель в serializable EDN map.
   Tensor data → vec, atom grad → nil."
  [model]
  {:format    :kan-framework-v1
   :timestamp (.format (LocalDateTime/now) (DateTimeFormatter/ofPattern "yyyy-MM-dd'T'HH:mm:ss"))
   :arch      (:arch model)
   :degree    (:degree model)
   :layers    (vec (for [layer (:layers model)]
                     {:in-dim   (:in-dim layer)
                      :out-dim  (:out-dim layer)
                      :degree   (:degree layer)
                      :n-coeffs (:n-coeffs layer)
                      :coeffs   (vec (for [row (:coeffs layer)]
                                       (vec (for [tensor row]
                                              {:data  (vec (:data tensor))
                                               :shape (vec (:shape tensor))}))))}))})

(defn edn->model
  "Восстанавливает модель из EDN map.
   vec → Tensor с double-array."
  [edn-data]
  (let [layers (vec (for [layer-data (:layers edn-data)]
                      {:in-dim   (:in-dim layer-data)
                       :out-dim  (:out-dim layer-data)
                       :degree   (:degree layer-data)
                       :n-coeffs (:n-coeffs layer-data)
                       :coeffs   (vec (for [row (:coeffs layer-data)]
                                        (vec (for [td row]
                                               (t2/tensor (:data td) (:shape td))))))}))]
    {:arch   (:arch edn-data)
     :degree (:degree edn-data)
     :layers layers}))

;; ============================================================
;; FILE I/O
;; ============================================================

(defn save-model
  "Сохраняет модель в EDN файл."
  [model path]
  (let [edn-data (model->edn model)
        content (pr-str edn-data)]
    (io/make-parents path)
    (spit path content)
    (println (format "    Model saved: %s (%d bytes)" path (count content)))
    path))

(defn load-model
  "Загружает модель из EDN файла."
  [path]
  (let [content (slurp path)
        edn-data (edn/read-string content)
        model (edn->model edn-data)]
    (println (format "    Model loaded: %s (arch=%s, degree=%d)"
                     path (pr-str (:arch model)) (:degree model)))
    model))

;; ============================================================
;; CHECKPOINT SYSTEM
;; ============================================================

(defn make-checkpoint-dir
  "Создаёт директорию для checkpoint-ов."
  [base-dir]
  (let [dir (io/file base-dir)]
    (.mkdirs dir)
    (.getPath dir)))

(defn save-checkpoint
  "Сохраняет checkpoint: модель + training state."
  [model epoch loss lr checkpoint-dir]
  (let [filename (format "checkpoint_epoch_%04d.edn" epoch)
        path (str checkpoint-dir "/" filename)
        edn-data (assoc (model->edn model)
                   :checkpoint {:epoch epoch
                                :loss loss
                                :lr lr})]
    (spit path (pr-str edn-data))
    (println (format "    Checkpoint saved: epoch %d, loss %.6f → %s" epoch loss filename))
    path))

(defn load-checkpoint
  "Загружает checkpoint: модель + training state."
  [path]
  (let [content (slurp path)
        edn-data (edn/read-string content)
        model (edn->model edn-data)
        checkpoint (:checkpoint edn-data)]
    (println (format "    Checkpoint loaded: epoch %d, loss %.6f"
                     (:epoch checkpoint) (:loss checkpoint)))
    {:model model
     :epoch (:epoch checkpoint)
     :loss (:loss checkpoint)
     :lr (:lr checkpoint)}))

(defn find-latest-checkpoint
  "Находит последний checkpoint в директории."
  [checkpoint-dir]
  (let [dir (io/file checkpoint-dir)
        files (when (.exists dir)
                (sort (filter #(.endsWith (.getName %) ".edn")
                              (.listFiles dir))))]
    (when (seq files)
      (.getPath (last files)))))

;; ============================================================
;; TRAINING С CHECKPOINTS
;; ============================================================

(defn train-with-checkpoints
  "Training loop с автоматическими checkpoints."
  [model x y epochs lr checkpoint-dir checkpoint-every
   & [{:keys [print-every] :or {print-every 10}}]]
  (make-checkpoint-dir checkpoint-dir)
  (loop [m model ep 0 history []]
    (if (= ep epochs)
      {:model m :history history}
      (let [[m2 loss] (kf/train-step m x y lr)
            history2 (conj history loss)]
        ;; Print
        (when (zero? (mod (inc ep) print-every))
          (println (format "    Epoch %3d | Loss: %.6f" (inc ep) loss)))
        ;; Checkpoint
        (when (and (pos? checkpoint-every)
                   (zero? (mod (inc ep) checkpoint-every)))
          (save-checkpoint m2 (inc ep) loss lr checkpoint-dir))
        (recur m2 (inc ep) history2)))))

(defn resume-training
  "Продолжает обучение с последнего checkpoint."
  [checkpoint-dir x y additional-epochs lr
   & [{:keys [checkpoint-every print-every]
       :or {checkpoint-every 20 print-every 10}}]]
  (let [latest (find-latest-checkpoint checkpoint-dir)]
    (if-not latest
      (println "    No checkpoint found!")
      (let [{:keys [model epoch loss]} (load-checkpoint latest)]
        (println (format "    Resuming from epoch %d (loss %.6f)" epoch loss))
        (train-with-checkpoints model x y additional-epochs lr
                                checkpoint-dir checkpoint-every
                                {:print-every print-every})))))

;; ============================================================
;; MODEL COMPARISON
;; ============================================================

(defn compare-models
  "Сравнивает два модели: параметры, архитектура."
  [model1 model2]
  (let [p1 (kf/all-params model1)
        p2 (kf/all-params model2)
        diffs (mapv (fn [t1 t2]
                      (let [^doubles d1 (:data t1)
                            ^doubles d2 (:data t2)
                            n (alength d1)]
                        (loop [i 0 max-diff 0.0]
                          (if (= i n) max-diff
                            (recur (inc i)
                                   (max max-diff (abs (- (aget d1 i) (aget d2 i)))))))))
                    p1 p2)
        max-diff (apply max diffs)]
    {:arch-match (= (:arch model1) (:arch model2))
     :degree-match (= (:degree model1) (:degree model2))
     :n-params (count p1)
     :max-diff max-diff
     :exact (< max-diff 1e-15)}))

;; ============================================================
;; DEMO
;; ============================================================

(defn demo-serialization []
  (println "  ╔══════════════════════════════════════╗")
  (println "  ║  Model Serialization                 ║")
  (println "  ║  save · load · checkpoint · resume   ║")
  (println "  ╚══════════════════════════════════════╝")

  (let [tmpdir "/tmp/kan-checkpoints"]

    ;; Part 1: Save/Load roundtrip
    (println "\n  Part 1: Save/Load roundtrip")
    (let [model (kf/make-model [2 4 1] 4)
          path "/tmp/kan-model-test.edn"]
      (save-model model path)
      (let [loaded (load-model path)
            cmp (compare-models model loaded)]
        (println (format "    Arch match:   %s" (:arch-match cmp)))
        (println (format "    Degree match: %s" (:degree-match cmp)))
        (println (format "    Params:       %d" (:n-params cmp)))
        (println (format "    Max diff:     %.2e" (:max-diff cmp)))
        (println (format "    Exact:        %s" (if (:exact cmp) "✅" "❌")))
        ;; Cleanup
        (io/delete-file path true)))

    ;; Part 2: Train → Save → Load → Verify same predictions
    (println "\n  Part 2: Train → Save → Load → Verify")
    (let [n 30
          xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
          ys (mapv math/sin xs)
          x-tensor (t2/tensor xs [n 1])
          y-tensor (t2/tensor ys [n])
          model (kf/make-model [1 1] 4)
          ;; Train
          result (loop [m model ep 0]
                   (if (= ep 30)
                     m
                     (let [[m2 _] (kf/train-step m x-tensor y-tensor 0.01)]
                       (recur m2 (inc ep)))))
          ;; Save
          path "/tmp/kan-trained.edn"
          _ (save-model result path)
          ;; Load
          loaded (load-model path)
          ;; Compare predictions
          pred1 (kf/model-forward result x-tensor)
          pred2 (kf/model-forward loaded x-tensor)
          ^doubles d1 (:data pred1)
          ^doubles d2 (:data pred2)
          max-err (loop [i 0 mx 0.0]
                    (if (= i (alength d1)) mx
                      (recur (inc i) (max mx (abs (- (aget d1 i) (aget d2 i)))))))]
      (println (format "    Prediction error: %.2e %s"
                       max-err (if (< max-err 1e-10) "✅ exact" "❌")))
      (io/delete-file path true))

    ;; Part 3: Training with checkpoints
    (println "\n  Part 3: Training with checkpoints (every 10 epochs)")
    (let [n 30
          xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
          ys (mapv math/sin xs)
          model (kf/make-model [1 1] 4)
          result (train-with-checkpoints model
                   (t2/tensor xs [n 1]) (t2/tensor ys [n])
                   30 0.01 tmpdir 10
                   {:print-every 10})]
      (println (format "    Final loss: %.6f" (last (:history result))))
      (println (format "    Checkpoints saved in: %s" tmpdir)))

    ;; Part 4: Resume from checkpoint
    (println "\n  Part 4: Resume training from checkpoint")
    (let [n 30
          xs (vec (for [i (range n)] (- (* 4.0 (/ i (dec n))) 2.0)))
          ys (mapv math/sin xs)
          result (resume-training tmpdir
                   (t2/tensor xs [n 1]) (t2/tensor ys [n])
                   20 0.01
                   {:checkpoint-every 10 :print-every 10})]
      (when result
        (println (format "    Final loss: %.6f" (last (:history result))))))

    ;; Part 5: List checkpoints
    (println "\n  Part 5: Checkpoint files")
    (let [dir (io/file tmpdir)
          files (when (.exists dir)
                  (sort (map #(.getName %) (.listFiles dir))))]
      (doseq [f files]
        (println (format "    %s" f))))

    ;; Cleanup
    (let [dir (io/file tmpdir)]
      (when (.exists dir)
        (doseq [f (.listFiles dir)]
          (io/delete-file f true))
        (io/delete-file dir true)))

    ;; Part 6: Summary
    (println "\n  Part 6: Serialization summary")
    (println "    ✅ save-model → EDN (Clojure native)")
    (println "    ✅ load-model → exact reconstruction")
    (println "    ✅ Roundtrip: model == load(save(model))")
    (println "    ✅ Checkpoint every N epochs")
    (println "    ✅ Resume training from latest checkpoint")
    (println "    ✅ Model comparison (params diff)")))
