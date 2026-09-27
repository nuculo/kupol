//! Differentiable Policy: KAN как RL-агент для маршрутизации сканирования
//!
//! Инспирировано `LogisticsKAN.hs` (251 строка):
//! State vector → KAN → softmax → argmax → выбор действия.
//! Тренировка: минимизация `total_cost` через дифференциируемую симуляцию.
//!
//! В контексте Duo Agents:
//! - State = [queue_sizes, thread_util, surge_mults, gravity_scores]
//! - Action = KAN(State) → softmax → select_file_for_scan
//! - Reward = -(total_scan_time + missed_vulnerabilities)
//!
//! KAN учится оптимальному порядку проверки файлов без ручных правил.

/// State vector для RL-агента сканирования
#[derive(Debug, Clone)]
pub struct ScanState {
    pub queue_sizes: Vec<f64>,      // Размеры очередей по категориям
    pub thread_utilization: f64,    // Загрузка потоков (0.0 - 1.0)
    pub surge_multipliers: Vec<f64>,// Surge-множители из SurgeScanQueue
    pub gravity_scores: Vec<f64>,   // Gravity scores из GravityAttention
}

impl ScanState {
    /// Упаковать состояние в плоский вектор для KAN
    pub fn to_vector(&self) -> Vec<f64> {
        let mut v = self.queue_sizes.clone();
        v.push(self.thread_utilization);
        v.extend_from_slice(&self.surge_multipliers);
        v.extend_from_slice(&self.gravity_scores);
        v
    }
}

/// Простая KAN-политика: линейный слой + SiLU + softmax
/// (Упрощённая версия, но показывает дифференциируемый pipeline)
pub struct KanPolicy {
    pub weights: Vec<Vec<f64>>,  // [n_actions, state_dim]
    pub biases: Vec<f64>,        // [n_actions]
    pub n_actions: usize,
    pub state_dim: usize,
}

impl KanPolicy {
    pub fn new(state_dim: usize, n_actions: usize) -> Self {
        // Xavier initialization
        let scale = (2.0 / (state_dim + n_actions) as f64).sqrt();
        let mut weights = Vec::new();
        let mut seed = 42u64;
        for _ in 0..n_actions {
            let mut row = Vec::new();
            for _ in 0..state_dim {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let r = (seed as f64 / u64::MAX as f64 - 0.5) * 2.0 * scale;
                row.push(r);
            }
            weights.push(row);
        }
        Self {
            weights,
            biases: vec![0.0; n_actions],
            n_actions,
            state_dim,
        }
    }

    /// Forward: state → action probabilities (softmax с маскировкой)
    /// mask[i] = true → действие допустимо, false → замаскировано (-inf)
    pub fn forward_masked(&self, state: &[f64], mask: &[bool]) -> Vec<f64> {
        let mut logits = Vec::with_capacity(self.n_actions);
        for i in 0..self.n_actions {
            if i < mask.len() && !mask[i] {
                logits.push(f64::NEG_INFINITY); // Маскируем недопустимые
                continue;
            }
            let mut sum = self.biases[i];
            for j in 0..self.state_dim.min(state.len()) {
                sum += self.weights[i][j] * state[j];
            }
            // SiLU activation: x * sigmoid(x)
            let sigmoid = 1.0 / (1.0 + (-sum).exp());
            logits.push(sum * sigmoid);
        }
        softmax(&logits)
    }

    /// Forward без маски (все действия допустимы)
    pub fn forward(&self, state: &[f64]) -> Vec<f64> {
        let mask = vec![true; self.n_actions];
        self.forward_masked(state, &mask)
    }

    /// Выбрать действие с маской (argmax среди допустимых)
    pub fn choose_action_masked(&self, state: &[f64], mask: &[bool]) -> usize {
        let probs = self.forward_masked(state, mask);
        probs.iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    /// Выбрать действие (argmax)
    pub fn choose_action(&self, state: &[f64]) -> usize {
        let probs = self.forward(state);
        probs.iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}

fn softmax(v: &[f64]) -> Vec<f64> {
    let max_v = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = v.iter().map(|x| (x - max_v).exp()).collect();
    let sum: f64 = exps.iter().sum();
    if sum == 0.0 {
        return vec![1.0 / v.len() as f64; v.len()]; // Uniform fallback
    }
    exps.iter().map(|e| e / sum).collect()
}

/// Дифференциируемая симуляция: запускает N шагов сканирования
/// и возвращает total_cost (чем меньше — тем лучше)
pub struct ScanSimulator {
    pub files: Vec<SimFile>,
    pub total_cost: f64,
    pub vulns_found: usize,
    pub vulns_missed: usize,
    pub steps_taken: usize,
}

#[derive(Debug, Clone)]
pub struct SimFile {
    pub name: String,
    pub risk: f64,           // Истинный риск (unknown to policy)
    pub has_vuln: bool,      // Есть ли уязвимость
    pub scanned: bool,       // Был ли просканирован
    pub wait_time: u32,      // Сколько тиков ждёт в очереди
}

impl ScanSimulator {
    pub fn new(files: Vec<SimFile>) -> Self {
        Self {
            files,
            total_cost: 0.0,
            vulns_found: 0,
            vulns_missed: 0,
            steps_taken: 0,
        }
    }

    /// Запустить симуляцию с заданной политикой на N шагов
    pub fn run(&mut self, policy: &KanPolicy, max_steps: usize) {
        for _ in 0..max_steps {
            // Маска: true = файл ещё НЕ просканирован
            let mask: Vec<bool> = self.files.iter().map(|f| !f.scanned).collect();
            if mask.iter().all(|&m| !m) { break; } // Все файлы просканированы

            // Построить state vector
            let state = self.build_state();
            if state.is_empty() { break; }

            // KAN policy выбирает файл (с маской!)
            let action = policy.choose_action_masked(&state, &mask);
            let file_idx = action.min(self.files.len() - 1);

            // "Сканируем" выбранный файл
            if !self.files[file_idx].scanned {
                self.files[file_idx].scanned = true;
                if self.files[file_idx].has_vuln {
                    self.vulns_found += 1;
                }
                // Cost = wait_time файла (чем дольше ждал — тем хуже)
                self.total_cost += self.files[file_idx].wait_time as f64;
            }

            // Увеличиваем wait_time для непросканированных
            for f in self.files.iter_mut() {
                if !f.scanned {
                    f.wait_time += 1;
                }
            }

            self.steps_taken += 1;
        }

        // Штраф за пропущенные уязвимости
        for f in &self.files {
            if f.has_vuln && !f.scanned {
                self.vulns_missed += 1;
                self.total_cost += 100.0; // Большой штраф
            }
        }
    }

    fn build_state(&self) -> Vec<f64> {
        let unscanned: Vec<&SimFile> = self.files.iter().filter(|f| !f.scanned).collect();
        if unscanned.is_empty() { return vec![]; }

        // State: [risk_per_file..., avg_wait, pct_scanned]
        let mut state = Vec::new();
        for f in &self.files {
            state.push(if f.scanned { 0.0 } else { f.risk });
        }
        let avg_wait = self.files.iter()
            .filter(|f| !f.scanned)
            .map(|f| f.wait_time as f64)
            .sum::<f64>() / unscanned.len().max(1) as f64;
        state.push(avg_wait / 10.0); // Нормализация
        let pct = self.files.iter().filter(|f| f.scanned).count() as f64 / self.files.len() as f64;
        state.push(pct);
        state
    }
}
