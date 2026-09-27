(ns kan-kat.fp16
  "Утилиты для работы с половинной точностью (FP16).
   
   В Java 21+ появилась нативная поддержка Float16 (IEEE 754 half-precision).
   FP16 хранится в памяти как 16-битный `short`.
   
   Диапазон: ~5.96e-8 до 65504
   Точность: 11 бит сигнификанд (~3 десятичные цифры)
   
   Конвертация:
   Float.floatToFloat16(float) -> short
   Float.float16ToFloat(short) -> float"
  (:refer-clojure :exclude [float]))

;; ============================================================
;; КОНВЕРТАЦИЯ
;; ============================================================

(defn f32->f16
  "Конвертирует float (32-bit) в FP16 short (16-bit)."
  [^Float f]
  (java.lang.Float/floatToFloat16 f))

(defn f16->f32
  "Конвертирует FP16 short (16-bit) во float (32-bit)."
  ^Float [^Short s]
  (java.lang.Float/float16ToFloat s))

(defn d64->f16
  "Конвертирует double (64-bit) в FP16 short."
  [^Double d]
  (java.lang.Float/floatToFloat16 (unchecked-float d)))

(defn f16->d64
  "Конвертирует FP16 short в double (64-bit)."
  ^Double [^Short s]
  (unchecked-double (java.lang.Float/float16ToFloat s)))

;; ============================================================
;; МАССИВЫ
;; ============================================================

(defn doubles->shorts
  "Конвертирует массив doubles (FP64) в массив shorts (FP16)."
  ^shorts [^doubles d-arr]
  (let [n (alength d-arr)
        s-arr (short-array n)]
    (dotimes [i n]
      (aset s-arr i (d64->f16 (aget d-arr i))))
    s-arr))

(defn shorts->doubles
  "Конвертирует массив shorts (FP16) обратно в массив doubles (FP64)."
  ^doubles [^shorts s-arr]
  (let [n (alength s-arr)
        d-arr (double-array n)]
    (dotimes [i n]
      (aset d-arr i (f16->d64 (aget s-arr i))))
    d-arr))

;; ============================================================
;; GRADIENT SCALING
;; ============================================================

(defn scale-loss
  "Умножает ошибку (loss) на scale фактор перед backward pass,
   чтобы предотвратить обнуление микро-градиентов (underflow) в FP16."
  ^Double [^Double loss ^Double scale]
  (* loss scale))

(defn unscale-gradients-f16!
  "Делит градиенты (в FP16 массиве) на scale фактор
   после backward pass. Мутирует массив напрямую."
  [^shorts grad-arr ^Double scale]
  (let [n (alength grad-arr)
        f-scale (unchecked-float (/ 1.0 scale))]
    (dotimes [i n]
      (let [val (f16->f32 (aget grad-arr i))]
        (aset grad-arr i (f32->f16 (* val f-scale)))))))
