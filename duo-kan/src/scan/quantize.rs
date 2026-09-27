//! Scalar Quantization (INT8 Compressed Engine)
//!
//! Инспирировано Qdrant `encoded_vectors_u8.rs`.
//! Нейронные градеры (DpoScorer, MoeRouter) по умолчанию хранят веса в f64.
//! Если модель разрастается (миллионы параметров), потребление RAM становится проблемой
//! (сотни мегабайт, что недопустимо для легковесных local pre-commit hooks).
//!
//! Мы используем Асимметричную Скалярную Квантизацию (Asymmetric Affine Quantization),
//! чтобы сжать веса из 64-бит `f64` в 8-бит `i8`, получая 8x компрессию.

use serde::{Serialize, Deserialize};

/// Метаданные квантизации, необходимые для обратного восстановления (де-квантизации) 
/// или для вычисления Dot Product прямо в INT8.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantizationMeta {
    pub scale: f64,
    pub zero_point: i8,
    pub min_val: f64,
    pub max_val: f64,
}

/// Квантизованный (сжатый) тензор
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantizedTensor {
    pub data: Vec<i8>,
    pub meta: QuantizationMeta,
}

impl QuantizedTensor {
    /// Оценка размера тензора в байтах
    pub fn memory_size_bytes(&self) -> usize {
        self.data.len() + std::mem::size_of::<QuantizationMeta>()
    }
}

pub struct Quantizer;

impl Quantizer {
    /// Выполняет асимметричную скалярную квантизацию f64 -> i8 (8x сжатие)
    pub fn quantize(weights: &[f64]) -> QuantizedTensor {
        if weights.is_empty() {
            return QuantizedTensor {
                data: vec![],
                meta: QuantizationMeta {
                    scale: 1.0,
                    zero_point: 0,
                    min_val: 0.0,
                    max_val: 0.0,
                },
            };
        }

        // 1. Находим Min и Max алгоритмом
        let mut min_val = f64::MAX;
        let mut max_val = f64::MIN;
        for &w in weights {
            if w < min_val { min_val = w; }
            if w > max_val { max_val = w; }
        }

        // Если все числа одинаковые, предотвращаем деление на ноль
        if (max_val - min_val).abs() < f64::EPSILON {
            max_val = min_val + 1.0;
        }

        // 2. Вычисляем Scale (Шаг) для диапазона [-128, 127]
        let q_min = -128.0;
        let q_max = 127.0;
        let scale = (max_val - min_val) / (q_max - q_min);

        // 3. Вычисляем Zero Point
        let zero_point_f = q_min - (min_val / scale);
        // Зажимаем Zero Point в допустимые рамки INT8
        let zero_point = zero_point_f.clamp(q_min, q_max).round() as i8;

        // 4. Квантизируем сам массив (Mapping)
        let mut quantized_data = Vec::with_capacity(weights.len());
        for &w in weights {
            // q = round(w / scale + zero_point)
            let q_f = (w / scale).round() + zero_point as f64;
            let q_i8 = q_f.clamp(q_min, q_max) as i8;
            quantized_data.push(q_i8);
        }

        QuantizedTensor {
            data: quantized_data,
            meta: QuantizationMeta {
                scale,
                zero_point,
                min_val,
                max_val,
            },
        }
    }

    /// Восстанавливает INT8 массив обратно в f64.
    /// Внимание: Восстановление идёт с небольшой потерей точности (precision loss),
    /// но макро-структура и соотношения весов сохраняются (что достаточно для нейросетей).
    pub fn dequantize(tensor: &QuantizedTensor) -> Vec<f64> {
        let mut f_data = Vec::with_capacity(tensor.data.len());
        let scale = tensor.meta.scale;
        let z = tensor.meta.zero_point as f64;

        for &q in &tensor.data {
            // w = (q - zero_point) * scale
            let w = (q as f64 - z) * scale;
            f_data.push(w);
        }
        f_data
    }

    /// Сравнение размеров O(1)
    pub fn unquantized_size_bytes(len: usize) -> usize {
        len * std::mem::size_of::<f64>()
    }
}
