//! Top-K Security Sampling: Стохастический аудит с температурой
//!
//! Инспирировано `Generation.hs` (94 строки) — `sampleTopK`:
//! Вместо greedy argmax, выбираем из top-K кандидатов с температурой.
//!
//! Режимы:
//! - Temperature=0.1 → Exploitation (CI/CD, всегда ТОП файлы)
//! - Temperature=0.8 → Exploration  (ночной аудит, стохастический)
//! - Temperature=1.5 → Chaos        (Red Team, fuzzing, пентест)
//!
//! Exploration vs Exploitation trade-off для Code Security.

/// Предустановленные режимы сканирования
#[derive(Debug, Clone, Copy)]
pub enum ScanTemperature {
    /// CI/CD: почти детерминированное, всегда ТОП файлы
    CiCd,
    /// Ночной аудит: умеренная стохастичность
    NightAudit,
    /// Red Team: хаотическое исследование
    RedTeam,
    /// Пользовательская температура
    Custom(f64),
}

impl ScanTemperature {
    pub fn value(&self) -> f64 {
        match self {
            Self::CiCd => 0.1,
            Self::NightAudit => 0.8,
            Self::RedTeam => 1.5,
            Self::Custom(t) => *t,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::CiCd => "CI/CD (T=0.1)",
            Self::NightAudit => "Night Audit (T=0.8)",
            Self::RedTeam => "Red Team (T=1.5)",
            Self::Custom(_) => "Custom",
        }
    }
}

/// Файл-кандидат для сканирования с KAN-приоритетом
#[derive(Debug, Clone)]
pub struct ScanCandidate {
    pub path: String,
    pub kan_priority: f64,   // KAN-вычисленный приоритет (0.0 - 1.0)
    pub category: &'static str,
}

/// Результат sampling: какие файлы были выбраны
#[derive(Debug)]
pub struct SamplingResult {
    pub selected: Vec<ScanCandidate>,
    pub temperature: f64,
    pub top_k: usize,
    pub mode: &'static str,
}

/// Top-K Security Sampler
pub struct TopKSampler {
    rng_state: u64,
}

impl TopKSampler {
    pub fn new(seed: u64) -> Self {
        Self { rng_state: seed }
    }

    /// Простой PRNG (xorshift64)
    fn next_f64(&mut self) -> f64 {
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 7;
        self.rng_state ^= self.rng_state << 17;
        (self.rng_state as f64) / (u64::MAX as f64)
    }

    /// Top-K sampling с температурой (прямой порт из Generation.hs:sampleTopK)
    ///
    /// 1. Сортируем файлы по KAN-приоритету (убывание)
    /// 2. Берём top-K
    /// 3. Делим priority / temperature → scaled logits
    /// 4. softmax(scaled) → вероятности
    /// 5. Сэмплируем n файлов из top-K по вероятностям
    pub fn sample(
        &mut self,
        candidates: &[ScanCandidate],
        top_k: usize,
        temperature: ScanTemperature,
        n_select: usize,
    ) -> SamplingResult {
        let temp = temperature.value();

        // 1. Сортировка по priority (убывание)
        let mut sorted = candidates.to_vec();
        sorted.sort_by(|a, b| b.kan_priority.partial_cmp(&a.kan_priority).unwrap());

        // 2. Top-K
        let top: Vec<ScanCandidate> = sorted.into_iter().take(top_k).collect();

        // 3. Scale by temperature
        let scaled: Vec<f64> = top.iter()
            .map(|c| c.kan_priority / temp)
            .collect();

        // 4. Softmax
        let probs = softmax(&scaled);

        // 5. Sample n_select файлов (без повторений)
        let mut selected = Vec::new();
        let mut available: Vec<(ScanCandidate, f64)> = top.into_iter()
            .zip(probs.into_iter())
            .collect();

        for _ in 0..n_select.min(available.len()) {
            if available.is_empty() { break; }

            // Нормализуем оставшиеся вероятности
            let total: f64 = available.iter().map(|(_, p)| p).sum();
            if total <= 0.0 { break; }

            let r = self.next_f64() * total;
            let mut cumsum = 0.0;
            let mut pick_idx = 0;
            for (i, (_, p)) in available.iter().enumerate() {
                cumsum += p;
                if cumsum >= r {
                    pick_idx = i;
                    break;
                }
            }

            let (candidate, _) = available.remove(pick_idx);
            selected.push(candidate);
        }

        SamplingResult {
            selected,
            temperature: temp,
            top_k,
            mode: temperature.label(),
        }
    }
}

fn softmax(v: &[f64]) -> Vec<f64> {
    if v.is_empty() { return vec![]; }
    let max_v = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = v.iter().map(|x| (x - max_v).exp()).collect();
    let sum: f64 = exps.iter().sum();
    if sum == 0.0 { return vec![1.0 / v.len() as f64; v.len()]; }
    exps.iter().map(|e| e / sum).collect()
}
