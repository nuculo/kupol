(ns kan-kat.recurrent-kan
  (:require [kan-kat.tensor-v2 :as t2]
            [kan-kat.kan-framework :as fw]))

(defn make-rnn-kan
  "Создает ячейку рекуррентной сети Колмогорова-Арнольда (RNN-KAN).
   Ячейка принимает конкатенированный вектор [x_t, h_{t-1}] и возвращает h_t.
   `layers-config` должен начинаться с (+ input-dim hidden-dim) и заканчиваться hidden-dim.
   Например: [ (+ 10 32), 16, 32 ] для input=10, hidden=32."
  [layers-config]
  (fw/make-model layers-config 3))

(defn rnn-kan-cell-forward
  "Выполняет один шаг рекуррентности для момента времени t.
   x-t: входной тензор фичей на шаге t, размерность [B, I]
   h-prev: скрытое состояние с предыдущего шага, размерность [B, H]
   Возвращает h-t: новое скрытое состояние [B, H]."
  [rnn-kan x-t h-prev]
  ;; 1. Конкатенируем x_t и h_{t-1} вдоль фичей: [B, I + H]
  (let [combined (t2/t-cat x-t h-prev)
        ;; 2. Пропускаем через нелинейную KAN-сеть для получения h_t: [B, H]
        h-t (fw/model-forward rnn-kan combined)]
    h-t))

(defn rnn-kan-forward
  "Обрабатывает последовательность во времени (Unrolling / BPTT).
   x-seq: Вектор/список из S входных тензоров, каждый размерности [B, I].
   h0: Начальное скрытое состояние [B, H].
   Возвращает вектор из S скрытых состояний [h_1, h_2, ..., h_S].
   Последний элемент — это итоговое представление последовательности."
  [rnn-kan x-seq h0]
  (loop [remaining x-seq
         h-prev h0
         h-history []]
    (if (empty? remaining)
      h-history
      (let [x-t (first remaining)
            h-t (rnn-kan-cell-forward rnn-kan x-t h-prev)]
        (recur (rest remaining) h-t (conj h-history h-t))))))

(defn rnn-params
  "Возвращает все обучаемые параметры RNN-KAN."
  [rnn-kan]
  (fw/all-params rnn-kan))

(defn rnn-update
  "Обновляет параметры RNN-KAN после шага оптимизатора."
  [rnn-kan new-params]
  (fw/model-update rnn-kan new-params))

;; ==========================================
;; Demo 40: RNN-KAN Sequence Prediction
;; ==========================================

(defn demo-rnn-kan []
  (println "\n==========================================")
  (println " Demo 40: Recurrent KAN (RNN-KAN) Sequence")
  (println "==========================================\n")
  
  (let [B 20       ; Batch size
        SeqL 5     ; Sequence Length
        I 2        ; Input features
        H 4        ; Hidden state features
        
        ;; Функция синтетики: каждый шаг последовательности - случайный шум,
        ;; Задачей сети будет "просуммировать" input[0] из каждого шага t,
        ;; чтобы предсказать это значение в самом конце, используя память.
        target-fn (fn [seq-data]
                    (reduce + (map #(aget ^doubles (:data %) 0) seq-data)))
        
        ;; Генерируем датасет: 20 батчей случайных последовательностей длины 5
        ;; На самом деле `x-seq` это список тензоров [B, I]
        x-seq-train (vec (for [t (range SeqL)]
                           (t2/randn [B I] 1.0)))
        
        ;; Y: для каждого из B элементов суммируем (x_t)[b, 0]
        y-data (double-array B)
        _ (dotimes [b B]
            (let [sum (loop [t 0 s 0.0]
                        (if (= t SeqL)
                          s
                          (let [xt (x-seq-train t)
                                xt-val (aget ^doubles (:data xt) (+ (* b I) 0))]
                            (recur (inc t) (+ s xt-val)))))]
              (aset y-data b sum)))
        Y-train (t2/->Tensor y-data [B 1] (t2/compute-stride [B 1]) B (atom nil) [] nil)
        
        ;; Начальное состояние h0 = 0 (нулевой тензор [B, H])
        h0 (t2/tensor (vec (repeat (* B H) 0.0)) [B H])
        
        ;; Архитектура: Вход [I + H] = 6, Скрытый [H] = 4
        rnn-kan (make-rnn-kan [ (+ I H) 8 H ])
        ;; Предсказание из скрытого слоя H -> 1
        head-kan (fw/make-model [ H 1 ] 3)]
        
    (println "Training RNN-KAN to sum features across time steps...")
    (println (format "Batch: %d | SeqLen: %d | Input: %d | Hidden: %d" B SeqL I H))
    
    (loop [ep 1
           rnn rnn-kan
           head head-kan]
      (if (> ep 50)
        (do
          (println "Training finished.")
          [rnn head])
        (let [;; 1. Unroll RNN over time
              h-history (rnn-kan-forward rnn x-seq-train h0)
              ;; 2. Take final hidden state h_S
              h-final (last h-history)
              ;; 3. Predict final target
              pred (fw/model-forward head h-final)
              ;; 4. Compute Loss
              loss-t (t2/mse-loss pred Y-train)
              loss-val (aget ^doubles (:data loss-t) 0)]
              
          ;; BPTT: Backpropagation Through Time
          (t2/backward! loss-t)
          
          ;; Extract Parameters
          (let [p-rnn (rnn-params rnn)
                p-head (fw/all-params head)
                all-p (concat p-rnn p-head)
                ;; SGD Update with gradient clipping
                new-all-p (mapv #(t2/sgd-step! % 0.005 1.0) all-p)
                
                new-p-rnn (vec (take (count p-rnn) new-all-p))
                new-p-head (vec (drop (count p-rnn) new-all-p))
                
                new-rnn (rnn-update rnn new-p-rnn)
                new-head (fw/model-update head new-p-head)]
                
            (when (zero? (mod ep 10))
              (println (format "  Epoch %3d | Loss: %8.4f" ep loss-val)))
            (recur (inc ep) new-rnn new-head)))))))
