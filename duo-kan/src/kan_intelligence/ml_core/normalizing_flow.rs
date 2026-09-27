//! 🌊 Normalizing Flow for Anomaly Detection
//!
//! Inspired by Clojure_KAN `automorphic_kan.clj`:
//!   - Chain of invertible coupling layers: z ~ N(0,I) → x = f(z)
//!   - Log-det Jacobian for exact density estimation
//!   - Forward: z → x (generate), Inverse: x → z (encode)
//!
//! For Duo: Train the flow on "normal" MRs (file counts, churn, author
//! tenure, test coverage). At inference, compute log p(MR). If
//! log p(MR) < threshold → anomalous → possible Supply Chain Attack.

use tracing::info;

// =============================================================================
// MR Feature Vector
// =============================================================================

/// Features extracted from a Merge Request for density estimation.
#[derive(Debug, Clone)]
pub struct MRFeatures {
    pub files_changed: f64,
    pub lines_added: f64,
    pub lines_deleted: f64,
    pub author_tenure_days: f64,
    pub test_coverage_pct: f64,
    pub review_approvals: f64,
    pub is_dependency_update: f64,
    pub commit_count: f64,
}

impl MRFeatures {
    /// Convert to a flat vector for the flow.
    pub fn to_vec(&self) -> Vec<f64> {
        vec![
            self.files_changed,
            self.lines_added,
            self.lines_deleted,
            self.author_tenure_days,
            self.test_coverage_pct,
            self.review_approvals,
            self.is_dependency_update,
            self.commit_count,
        ]
    }

    pub fn dim() -> usize { 8 }
}

// =============================================================================
// Affine Coupling Layer
// =============================================================================

/// A single coupling layer of a Normalizing Flow.
///
/// Splits input into two halves: x = [x_a, x_b]
/// - x_a passes through unchanged
/// - x_b is transformed: y_b = x_b * exp(s(x_a)) + t(x_a)
///
/// The log-det Jacobian is simply sum(s(x_a)).
///
/// Here s and t are simple learned affine functions for the demo.
#[derive(Debug, Clone)]
pub struct CouplingLayer {
    /// Scale parameters (learned): one per second-half dimension
    pub s_weights: Vec<f64>,
    pub s_bias: Vec<f64>,
    /// Translation parameters (learned): one per second-half dimension
    pub t_weights: Vec<f64>,
    pub t_bias: Vec<f64>,
    /// Which half to transform (0 = second half, 1 = first half)
    pub mask_parity: usize,
    pub half_dim: usize,
}

impl CouplingLayer {
    pub fn new(dim: usize, parity: usize) -> Self {
        let half = dim / 2;
        // Initialize s close to 0 (identity-like), t close to 0
        let s_weights = vec![0.1; half];
        let s_bias = vec![0.0; half];
        let t_weights = vec![0.0; half];
        let t_bias = vec![0.0; half];
        Self { s_weights, s_bias, t_weights, t_bias, mask_parity: parity, half_dim: half }
    }

    /// Compute s(x_condition) and t(x_condition) — simple affine for demo.
    fn compute_st(&self, x_cond: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let s: Vec<f64> = x_cond.iter().enumerate()
            .map(|(i, &v)| (self.s_weights[i % self.half_dim] * v + self.s_bias[i % self.half_dim]).tanh())
            .collect();
        let t: Vec<f64> = x_cond.iter().enumerate()
            .map(|(i, &v)| self.t_weights[i % self.half_dim] * v + self.t_bias[i % self.half_dim])
            .collect();
        (s, t)
    }

    /// Forward pass: x → y, returns (y, log_det_jacobian)
    pub fn forward(&self, x: &[f64]) -> (Vec<f64>, f64) {
        let (x_a, x_b) = if self.mask_parity == 0 {
            (&x[..self.half_dim], &x[self.half_dim..])
        } else {
            (&x[self.half_dim..], &x[..self.half_dim])
        };

        let (s, t) = self.compute_st(x_a);

        let y_b: Vec<f64> = x_b.iter().enumerate()
            .map(|(i, &v)| v * s[i].exp() + t[i])
            .collect();

        let log_det: f64 = s.iter().sum();

        let mut y = vec![0.0; x.len()];
        if self.mask_parity == 0 {
            y[..self.half_dim].copy_from_slice(x_a);
            y[self.half_dim..].copy_from_slice(&y_b);
        } else {
            y[..self.half_dim].copy_from_slice(&y_b);
            y[self.half_dim..].copy_from_slice(x_a);
        }

        (y, log_det)
    }

    /// Inverse pass: y → x
    pub fn inverse(&self, y: &[f64]) -> Vec<f64> {
        let (y_a, y_b) = if self.mask_parity == 0 {
            (&y[..self.half_dim], &y[self.half_dim..])
        } else {
            (&y[self.half_dim..], &y[..self.half_dim])
        };

        let (s, t) = self.compute_st(y_a);

        let x_b: Vec<f64> = y_b.iter().enumerate()
            .map(|(i, &v)| (v - t[i]) * (-s[i]).exp())
            .collect();

        let mut x = vec![0.0; y.len()];
        if self.mask_parity == 0 {
            x[..self.half_dim].copy_from_slice(y_a);
            x[self.half_dim..].copy_from_slice(&x_b);
        } else {
            x[..self.half_dim].copy_from_slice(&x_b);
            x[self.half_dim..].copy_from_slice(y_a);
        }

        x
    }

    /// Simple gradient-free training: adjust s/t to make log p(data) higher.
    pub fn fit_step(&mut self, data: &[Vec<f64>], lr: f64) {
        let eps = 1e-4;

        // Finite-difference gradient for each parameter
        for i in 0..self.half_dim {
            // s_weights gradient
            self.s_weights[i] += eps;
            let loss_plus = self.avg_neg_log_density(data);
            self.s_weights[i] -= 2.0 * eps;
            let loss_minus = self.avg_neg_log_density(data);
            self.s_weights[i] += eps; // restore
            let grad = (loss_plus - loss_minus) / (2.0 * eps);
            self.s_weights[i] -= lr * grad.clamp(-5.0, 5.0);

            // t_weights gradient
            self.t_weights[i] += eps;
            let loss_plus = self.avg_neg_log_density(data);
            self.t_weights[i] -= 2.0 * eps;
            let loss_minus = self.avg_neg_log_density(data);
            self.t_weights[i] += eps;
            let grad = (loss_plus - loss_minus) / (2.0 * eps);
            self.t_weights[i] -= lr * grad.clamp(-5.0, 5.0);
        }
    }

    fn avg_neg_log_density(&self, data: &[Vec<f64>]) -> f64 {
        let n = data.len() as f64;
        data.iter().map(|x| {
            let (z, log_det) = self.forward(x);
            let log_pz: f64 = z.iter().map(|&zi| -0.5 * zi * zi - 0.9189).sum();
            -(log_pz + log_det)
        }).sum::<f64>() / n
    }
}

// =============================================================================
// Normalizing Flow (chain of coupling layers)
// =============================================================================

pub struct NormalizingFlow {
    pub layers: Vec<CouplingLayer>,
    pub dim: usize,
}

impl NormalizingFlow {
    /// Create a flow with `n_layers` alternating coupling layers.
    pub fn new(dim: usize, n_layers: usize) -> Self {
        let layers = (0..n_layers)
            .map(|i| CouplingLayer::new(dim, i % 2))
            .collect();
        Self { layers, dim }
    }

    /// Forward: x → z, accumulating log-det Jacobian.
    pub fn forward(&self, x: &[f64]) -> (Vec<f64>, f64) {
        let mut z = x.to_vec();
        let mut total_log_det = 0.0;
        for layer in &self.layers {
            let (z_new, ld) = layer.forward(&z);
            z = z_new;
            total_log_det += ld;
        }
        (z, total_log_det)
    }

    /// Inverse: z → x (go backwards through layers).
    pub fn inverse(&self, z: &[f64]) -> Vec<f64> {
        let mut x = z.to_vec();
        for layer in self.layers.iter().rev() {
            x = layer.inverse(&x);
        }
        x
    }

    /// Compute log p(x) = log p_z(f(x)) + log|det J|.
    pub fn log_prob(&self, x: &[f64]) -> f64 {
        let (z, log_det) = self.forward(x);
        // log p_z(z) for standard normal
        let log_pz: f64 = z.iter()
            .map(|&zi| -0.5 * zi * zi - 0.5 * (2.0 * std::f64::consts::PI).ln())
            .sum();
        log_pz + log_det
    }

    /// Train the flow on data to maximize log p(data).
    pub fn fit(&mut self, data: &[Vec<f64>], epochs: usize, lr: f64) {
        for epoch in 1..=epochs {
            // Train each layer sequentially
            for layer_idx in 0..self.layers.len() {
                // Transform data through preceding layers
                let transformed: Vec<Vec<f64>> = data.iter().map(|x| {
                    let mut z = x.clone();
                    for l in 0..layer_idx {
                        let (z_new, _) = self.layers[l].forward(&z);
                        z = z_new;
                    }
                    z
                }).collect();

                self.layers[layer_idx].fit_step(&transformed, lr);
            }

            if epoch % (epochs / 5).max(1) == 0 {
                let avg_logp = data.iter().map(|x| self.log_prob(x)).sum::<f64>() / data.len() as f64;
                info!(
                    "🌊 [Flow] Epoch {:>3} | Avg log p(x): {:.2}",
                    epoch, avg_logp
                );
            }
        }
    }
}

// =============================================================================
// Anomaly Detection
// =============================================================================

#[derive(Debug)]
pub enum AnomalyVerdict {
    Normal { log_prob: f64 },
    Suspicious { log_prob: f64, reason: String },
    Critical { log_prob: f64, reason: String },
}

impl std::fmt::Display for AnomalyVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnomalyVerdict::Normal { log_prob } =>
                write!(f, "✅ Normal (log p = {:.2})", log_prob),
            AnomalyVerdict::Suspicious { log_prob, reason } =>
                write!(f, "⚠️  Suspicious (log p = {:.2}): {}", log_prob, reason),
            AnomalyVerdict::Critical { log_prob, reason } =>
                write!(f, "🚨 CRITICAL (log p = {:.2}): {}", log_prob, reason),
        }
    }
}

pub fn classify_mr(flow: &NormalizingFlow, mr: &MRFeatures, threshold_warn: f64, threshold_crit: f64) -> AnomalyVerdict {
    let features = mr.to_vec();
    let log_p = flow.log_prob(&features);

    if log_p < threshold_crit {
        AnomalyVerdict::Critical {
            log_prob: log_p,
            reason: detect_anomaly_reason(mr),
        }
    } else if log_p < threshold_warn {
        AnomalyVerdict::Suspicious {
            log_prob: log_p,
            reason: detect_anomaly_reason(mr),
        }
    } else {
        AnomalyVerdict::Normal { log_prob: log_p }
    }
}

fn detect_anomaly_reason(mr: &MRFeatures) -> String {
    let mut reasons = Vec::new();
    if mr.files_changed > 50.0 { reasons.push("massive file count"); }
    if mr.author_tenure_days < 7.0 { reasons.push("new author (<7 days)"); }
    if mr.test_coverage_pct < 10.0 { reasons.push("near-zero test coverage"); }
    if mr.is_dependency_update > 0.5 && mr.lines_added > 500.0 { reasons.push("suspicious dep update with large diff"); }
    if mr.review_approvals < 1.0 { reasons.push("no review approvals"); }
    if mr.commit_count > 20.0 { reasons.push("excessive commits"); }
    if reasons.is_empty() { "statistical outlier".into() } else { reasons.join(", ") }
}

// =============================================================================
// Normalize features to [0, 1] range
// =============================================================================

fn normalize(data: &[Vec<f64>]) -> (Vec<Vec<f64>>, Vec<f64>, Vec<f64>) {
    let dim = data[0].len();
    let mut mins = vec![f64::MAX; dim];
    let mut maxs = vec![f64::MIN; dim];

    for row in data {
        for (i, &v) in row.iter().enumerate() {
            if v < mins[i] { mins[i] = v; }
            if v > maxs[i] { maxs[i] = v; }
        }
    }

    let normed: Vec<Vec<f64>> = data.iter().map(|row| {
        row.iter().enumerate().map(|(i, &v)| {
            let range = (maxs[i] - mins[i]).max(1e-10);
            (v - mins[i]) / range
        }).collect()
    }).collect();

    (normed, mins, maxs)
}

fn normalize_single(x: &[f64], mins: &[f64], maxs: &[f64]) -> Vec<f64> {
    x.iter().enumerate().map(|(i, &v)| {
        let range = (maxs[i] - mins[i]).max(1e-10);
        (v - mins[i]) / range
    }).collect()
}

// =============================================================================
// Demo
// =============================================================================

pub fn demo_normalizing_flow() {
    info!("🌊 [Flow] ════════════════════════════════════════");
    info!("🌊 [Flow] Normalizing Flow: Supply Chain Attack Detection");
    info!("🌊 [Flow] ────────────────────────────────────────");

    // Generate "normal" MR training data
    let normal_mrs: Vec<MRFeatures> = (0..60).map(|i| {
        let jitter = (i as f64 * 0.1).sin() * 0.3;
        MRFeatures {
            files_changed: 3.0 + jitter * 5.0,
            lines_added: 50.0 + jitter * 40.0,
            lines_deleted: 20.0 + jitter * 15.0,
            author_tenure_days: 180.0 + jitter * 60.0,
            test_coverage_pct: 75.0 + jitter * 10.0,
            review_approvals: 2.0 + (jitter * 1.5).round(),
            is_dependency_update: 0.0,
            commit_count: 3.0 + jitter.abs() * 2.0,
        }
    }).collect();

    let train_data: Vec<Vec<f64>> = normal_mrs.iter().map(|m| m.to_vec()).collect();
    let (normed_data, mins, maxs) = normalize(&train_data);

    // Build and train flow
    let mut flow = NormalizingFlow::new(MRFeatures::dim(), 4);
    info!("🌊 [Flow] Training on {} normal MRs (4 coupling layers)...", normal_mrs.len());
    flow.fit(&normed_data, 25, 0.01);

    // Compute thresholds from training data
    let train_logps: Vec<f64> = normed_data.iter().map(|x| flow.log_prob(x)).collect();
    let mean_logp = train_logps.iter().sum::<f64>() / train_logps.len() as f64;
    let std_logp = (train_logps.iter().map(|&l| (l - mean_logp).powi(2)).sum::<f64>() / train_logps.len() as f64).sqrt();
    let threshold_warn = mean_logp - 2.0 * std_logp;
    let threshold_crit = mean_logp - 3.0 * std_logp;

    info!("🌊 [Flow] ────────────────────────────────────────");
    info!("🌊 [Flow] Thresholds: warn < {:.2}, critical < {:.2}", threshold_warn, threshold_crit);

    // Test MRs
    let test_mrs = vec![
        ("Normal bugfix",        MRFeatures { files_changed: 4.0, lines_added: 60.0, lines_deleted: 25.0, author_tenure_days: 200.0, test_coverage_pct: 80.0, review_approvals: 2.0, is_dependency_update: 0.0, commit_count: 3.0 }),
        ("Normal feature",       MRFeatures { files_changed: 8.0, lines_added: 120.0, lines_deleted: 30.0, author_tenure_days: 365.0, test_coverage_pct: 70.0, review_approvals: 3.0, is_dependency_update: 0.0, commit_count: 5.0 }),
        ("Suspicious dep update",MRFeatures { files_changed: 25.0, lines_added: 800.0, lines_deleted: 5.0, author_tenure_days: 30.0, test_coverage_pct: 15.0, review_approvals: 1.0, is_dependency_update: 1.0, commit_count: 12.0 }),
        ("🚨 Supply Chain Attack", MRFeatures { files_changed: 150.0, lines_added: 5000.0, lines_deleted: 2.0, author_tenure_days: 2.0, test_coverage_pct: 0.0, review_approvals: 0.0, is_dependency_update: 1.0, commit_count: 1.0 }),
        ("🚨 Typosquatting",      MRFeatures { files_changed: 80.0, lines_added: 3000.0, lines_deleted: 0.0, author_tenure_days: 1.0, test_coverage_pct: 5.0, review_approvals: 0.0, is_dependency_update: 1.0, commit_count: 30.0 }),
    ];

    info!("🌊 [Flow] ────────────────────────────────────────");
    info!("🌊 [Flow] Scanning {} merge requests...", test_mrs.len());

    // We need to create a "normalized" flow for consistency
    for (label, mr) in &test_mrs {
        let normed = normalize_single(&mr.to_vec(), &mins, &maxs);
        let logp = flow.log_prob(&normed);

        let verdict = if logp < threshold_crit {
            AnomalyVerdict::Critical { log_prob: logp, reason: detect_anomaly_reason(mr) }
        } else if logp < threshold_warn {
            AnomalyVerdict::Suspicious { log_prob: logp, reason: detect_anomaly_reason(mr) }
        } else {
            AnomalyVerdict::Normal { log_prob: logp }
        };

        info!("🌊 [Flow]   {:30} → {}", label, verdict);
    }

    // Roundtrip verification
    info!("🌊 [Flow] ────────────────────────────────────────");
    let test_vec = normalize_single(&normal_mrs[0].to_vec(), &mins, &maxs);
    let (z, _) = flow.forward(&test_vec);
    let reconstructed = flow.inverse(&z);
    let max_err: f64 = test_vec.iter().zip(reconstructed.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    info!("🌊 [Flow] Invertibility check: max |x - f⁻¹(f(x))| = {:.2e} {}",
        max_err, if max_err < 1e-10 { "✅" } else { "⚠️" });
    info!("🌊 [Flow] ════════════════════════════════════════");
}
