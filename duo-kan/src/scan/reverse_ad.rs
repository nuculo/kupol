//! Reverse-Mode AD для Backprop через Security Scorer
//!
//! Инспирировано `ReverseAD.hs` (95 строк):
//! Вместо N forward pass-ов (forward-mode AD), делаем 1 forward + 1 backward.
//!
//! Цепочка:  ScanInput → LoRA(x + x×A×B) → Scorer(×W) → softmax → Loss
//!
//! Backward (аналитический chain rule):
//!   dL/dLogits  = probs - one_hot(target)
//!   dL/dLoraOut = dL/dLogits × W^T
//!   dL/dA       = x^T × (dL/dLoraOut × B^T)
//!   dL/dB       = (x×A)^T × dL/dLoraOut
//!
//! Результат: O(T×d×r) вместо O(N×T²) — все N градиентов за один проход.

/// Матричные операции (минимальный набор для chain rule)
fn mat_mul(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let rows_a = a.len();
    let cols_b = if b.is_empty() { 0 } else { b[0].len() };
    let inner = if a.is_empty() { 0 } else { a[0].len() };
    let mut result = vec![vec![0.0; cols_b]; rows_a];
    for i in 0..rows_a {
        for j in 0..cols_b {
            let mut sum = 0.0;
            for k in 0..inner {
                sum += a[i][k] * b[k][j];
            }
            result[i][j] = sum;
        }
    }
    result
}

fn transpose(m: &[Vec<f64>]) -> Vec<Vec<f64>> {
    if m.is_empty() { return vec![]; }
    let rows = m.len();
    let cols = m[0].len();
    let mut result = vec![vec![0.0; rows]; cols];
    for i in 0..rows {
        for j in 0..cols {
            result[j][i] = m[i][j];
        }
    }
    result
}

fn mat_add(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    a.iter().zip(b.iter())
        .map(|(ra, rb)| ra.iter().zip(rb.iter()).map(|(x, y)| x + y).collect())
        .collect()
}

fn softmax(v: &[f64]) -> Vec<f64> {
    let max_v = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = v.iter().map(|x| (x - max_v).exp()).collect();
    let sum: f64 = exps.iter().sum();
    exps.iter().map(|e| e / sum).collect()
}

/// Результат backward pass: потери + градиенты LoRA-матриц A и B
#[derive(Debug)]
pub struct BackwardResult {
    pub loss: f64,
    pub grad_a: Vec<Vec<f64>>,  // dL/dA — [dim, rank]
    pub grad_b: Vec<Vec<f64>>,  // dL/dB — [rank, dim]
    pub total_params: usize,
}

/// Аналитический Reverse-Mode AD: Forward + Backward за один проход.
///
/// Прямо портированно из `ReverseAD.hs:loraBackward`:
/// ```
/// loraBackward :: [[Double]] -> [[Double]] -> [[Double]] -> [[Double]] -> [Int]
///              -> (Double, [[Double]], [[Double]])
/// ```
pub fn lora_backward(
    x: &[Vec<f64>],        // frozen input [seq, dim]
    lora_a: &[Vec<f64>],   // LoRA A [dim, rank]
    lora_b: &[Vec<f64>],   // LoRA B [rank, dim]
    head_w: &[Vec<f64>],   // Scorer head [dim, n_classes]
    targets: &[usize],     // target labels [seq]
) -> BackwardResult {
    // ═══ FORWARD PASS ═══
    let tmp1 = mat_mul(x, lora_a);               // [seq, rank]
    let tmp2 = mat_mul(&tmp1, lora_b);            // [seq, dim]
    let lora_out = mat_add(x, &tmp2);             // [seq, dim]
    let logits = mat_mul(&lora_out, head_w);      // [seq, n_classes]
    let probs: Vec<Vec<f64>> = logits.iter()
        .map(|row| softmax(row))
        .collect();

    // Loss: mean cross-entropy
    let seq_len = targets.len();
    let loss: f64 = probs.iter().zip(targets.iter())
        .map(|(p, &t)| -(p[t].max(1e-10)).ln())
        .sum::<f64>() / seq_len as f64;

    // ═══ BACKWARD PASS ═══
    // dL/dLogits = (probs - one_hot) / seq_len
    let d_logits: Vec<Vec<f64>> = probs.iter().zip(targets.iter())
        .map(|(p, &t)| {
            p.iter().enumerate()
                .map(|(i, &pi)| (pi - if i == t { 1.0 } else { 0.0 }) / seq_len as f64)
                .collect()
        })
        .collect();

    // dL/dLoraOut = dL/dLogits × Head^T
    let head_t = transpose(head_w);
    let d_lora_out = mat_mul(&d_logits, &head_t);  // [seq, dim]

    // dL/dtmp2 = dL/dLoraOut (since lora_out = x + tmp2)
    let d_tmp2 = &d_lora_out;                       // [seq, dim]

    // dL/dtmp1 = dTmp2 × B^T
    let b_t = transpose(lora_b);
    let d_tmp1 = mat_mul(d_tmp2, &b_t);             // [seq, rank]

    // dL/dA = x^T × dTmp1
    let x_t = transpose(x);
    let grad_a = mat_mul(&x_t, &d_tmp1);            // [dim, rank]

    // dL/dB = tmp1^T × dTmp2
    let tmp1_t = transpose(&tmp1);
    let grad_b = mat_mul(&tmp1_t, d_tmp2);           // [rank, dim]

    let total_params = grad_a.len() * grad_a[0].len() + grad_b.len() * grad_b[0].len();

    BackwardResult { loss, grad_a, grad_b, total_params }
}
