(ns kan-kat.data-loader
  (:require [clojure.java.io :as io]
            [clojure.string :as str]
            [kan-kat.tensor-v2 :as t2]))

(defn try-parse-double [s]
  (try (Double/parseDouble s)
       (catch Exception _ nil)))

(defn load-csv
  "Загружает CSV/TSV файл в виде вектора словарей, где ключами являются 
   названия колонок-заголовков, а значениями - распарсенные double-числа.
   Игнорируются ряды, где присутствуют не-числовые данные или пустые строки.
   
   Параметры:
   `file-path`: Строка-путь до файла.
   `separator`: Строковый разделитель (по умолчанию \",\")."
  ([file-path] (load-csv file-path ","))
  ([file-path separator]
   (with-open [rdr (io/reader file-path)]
     (let [lines (line-seq rdr)
           header (mapv str/trim (str/split (first lines) (re-pattern separator)))
           data-lines (rest lines)]
       (reduce (fn [acc line]
                 (if (str/blank? line)
                   acc
                   (let [row-strs (str/split line (re-pattern separator))
                         parsed (mapv #(try-parse-double (str/trim %)) row-strs)]
                     (if (some nil? parsed)
                       acc ; Пропускаем строчки с битыми/грязными данными
                       (conj acc (zipmap header parsed))))))
               []
               data-lines)))))

(defn train-test-split
  "Разделяет вектор словарей (датасет) на тренировочную и тестовую выборки.
   
   `dataset`: Вектор входных данных.
   `test-ratio`: Доля данных, отходящих в test (например, 0.2).
   `shuffle?`: Булево значение, если true - датасет перемешивается перед сплитом."
  [dataset test-ratio shuffle?]
  (let [data (if shuffle? (shuffle dataset) dataset)
        n (count data)
        test-amount (max 1 (int (* n test-ratio)))
        train-amount (- n test-amount)]
    {:train (vec (take train-amount data))
     :test (vec (drop train-amount data))}))

(defn- extract-columns [dataset cols]
  (mapv (fn [row]
          (mapv #(get row %) cols))
        dataset))

(defn to-tensor
  "Конвертирует `dataset` (список мап) в тензор `tensor_v2` формы [N, len(cols)].
   Извлекает только выбранные колонки."
  [dataset cols]
  (let [N (count dataset)
        C (count cols)
        flat-data (double-array (* N C))]
    (dotimes [i N]
      (let [row (nth dataset i)]
        (dotimes [j C]
          (aset flat-data (+ (* i C) j) (double (get row (nth cols j)))))))
    (t2/tensor (vec flat-data) [N C])))

;; ==========================================
;; Normalization Scalers
;; ==========================================

(defn fit-minmax
  "Обучает Min-Max scaler на `dataset` по указанным признакам `cols`.
   Возвращает словарь `{:mins [...] :maxs [...]}`."
  [dataset cols]
  (let [n (count dataset)
        c (count cols)
        raw-data (extract-columns dataset cols)
        ;; Инициализируем экстремумы первой строкой
        mins (double-array (first raw-data))
        maxs (double-array (first raw-data))]
    (doseq [i (range 1 n)]
      (let [row (nth raw-data i)]
        (dotimes [j c]
          (let [v (nth row j)]
            (when (< v (aget mins j)) (aset mins j v))
            (when (> v (aget maxs j)) (aset maxs j v))))))
    {:mins (vec mins)
     :maxs (vec maxs)}))

(defn transform-minmax
  "Применяет обученный `scaler` ({:mins :maxs}) к `dataset`.
   Возвращает новый датасет с отмасштабированными колонками."
  [scaler dataset cols]
  (let [mins (:mins scaler)
        maxs (:maxs scaler)
        c (count cols)]
    (mapv (fn [row]
            (reduce (fn [acc j]
                      (let [col-name (nth cols j)
                            v (get row col-name)
                            mn (nth mins j)
                            mx (nth maxs j)
                            range-val (- mx mn)
                            scaled (if (zero? range-val)
                                     0.0
                                     (/ (- v mn) range-val))]
                        (assoc acc col-name scaled)))
                    row
                    (range c)))
          dataset)))

(defn fit-standard
  "Обучает Standard scaler (Z-score: mean=0, std=1) на `dataset`.
   Возвращает `{:means [...] :stds [...]}`."
  [dataset cols]
  (let [n (count dataset)
        c (count cols)
        raw-data (extract-columns dataset cols)
        sums (double-array c)
        sum-sqs (double-array c)]
    ;; Вычисляем суммы и суммы квадратов в один проход
    (doseq [row raw-data]
      (dotimes [j c]
        (let [v (nth row j)]
          (aset sums j (+ (aget sums j) v))
          (aset sum-sqs j (+ (aget sum-sqs j) (* v v))))))
          
    (let [means (vec (map #(/ % n) sums))
          stds (vec (map-indexed 
                     (fn [j sum-sq]
                       (let [mean (nth means j)
                             variance (- (/ sum-sq n) (* mean mean))]
                         (Math/sqrt (max 0.0 variance))))
                     sum-sqs))]
      {:means means
       :stds stds})))

(defn transform-standard
  "Применяет обученный `scaler` ({:means :stds}) к `dataset`.
   Возвращает датасет с Z-нормализованными интервалами."
  [scaler dataset cols]
  (let [means (:means scaler)
        stds (:stds scaler)
        c (count cols)]
    (mapv (fn [row]
            (reduce (fn [acc j]
                      (let [col-name (nth cols j)
                            v (get row col-name)
                            m (nth means j)
                            s (nth stds j)
                            scaled (if (zero? s)
                                     0.0
                                     (/ (- v m) s))]
                        (assoc acc col-name scaled)))
                    row
                    (range c)))
          dataset)))

;; ==========================================
;; Demo 43: CSV Loader & Splitter Tracker
;; ==========================================

(defn demo-data-loader []
  (println "\n==========================================")
  (println " Demo 43: Data Loader & Preprocessing")
  (println "==========================================\n")
  
  (let [file-path "dummy_dataset.csv"]
    (println "Generating dummy dataset:" file-path)
    ;; Создаем фиктивный CSV с квартирами: Площадь, Комнаты, Возраст здания, Цена
    (with-open [w (io/writer file-path)]
      (.write w "Area,Rooms,Age,Price\n")
      (dotimes [_ 100]
        (let [area (+ 30 (rand 150))
              rooms (+ 1 (rand-int 4))
              age (rand-int 50)
              price (+ (* area 1000) (* rooms 5000) (- 100000 (* age 2000)) (- 5000 (rand 10000)))]
          (.write w (format "%.1f,%d,%d,%.1f\n" (double area) (int rooms) (int age) (double price))))))
          
    (println "1. Loading CSV into map records...")
    (let [dataset (load-csv file-path ",")]
      (println (format "   Loaded %d rows successfully." (count dataset)))
      
      (println "2. Train-Test Splitting (80/20, shuffle=true)...")
      (let [splits (train-test-split dataset 0.2 true)
            train-data (:train splits)
            test-data (:test splits)]
        (println (format "   Train: %d rows | Test: %d rows" (count train-data) (count test-data)))
        
        (println "3. Fitting Min-Max Scaler on TRAINING data (features: Area, Age)...")
        (let [feature-cols ["Area" "Age"]
              target-cols ["Price"]
              mm-scaler (fit-minmax train-data feature-cols)]
          
          (println "   Scaler specs:" mm-scaler)
          
          (println "4. Transforming TEST data via memory references avoiding Leakage...")
          (let [scaled-test (transform-minmax mm-scaler test-data feature-cols)]
            (println "   Raw test row 0:   " (select-keys (first test-data) feature-cols))
            (println "   Scaled test row 0:" (select-keys (first scaled-test) feature-cols))
            
            (println "5. Casting scaled dataset mappings identically to tensor_v2...")
            (let [X-tensor (to-tensor scaled-test feature-cols)
                  Y-tensor (to-tensor scaled-test target-cols)]
              (println "   X-tensor shape:" (:shape X-tensor))
              (println "   Y-tensor shape:" (:shape Y-tensor)))))))
              
    ;; Очистка
    (io/delete-file file-path true)))
