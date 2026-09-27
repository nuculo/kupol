//! 🔧 LoRA Adaptation for Frozen Security Rules
//!
//! Inspired by Clojure_KAN `lora.clj`:
//!   - W_new = W_original + A·B, where rank r << dim
//!   - Only A (d×r) and B (r×d) are trained, W stays frozen
//!   - Massive parameter savings: 2·d·r vs d·d
//!
//! For Duo: When we freeze a security rule into an O(1) macro (from
//! `kan_symbolic.clj` / `hybrid_kan.clj`), but it gets edge cases wrong,
//! we DON'T unfreeze the entire rule. Instead we attach a tiny LoRA delta
//! that patches just the edge cases.

use tracing::info;

// =============================================================================
// Frozen Rule (the O(1) macro from Symbolic Discovery)
// =============================================================================

/// A frozen security rule: fast lookup, no trainable parameters.
/// This is the "W_original" from LoRA.
#[derive(Debug, Clone)]
pub struct FrozenRule {
    pub name: String,
    /// Feature weights (the frozen "formula")
    pub weights: Vec<f64>,
    /// Bias
    pub bias: f64,
    /// Decision threshold
    pub threshold: f64,
}

impl FrozenRule {
    /// Score an input feature vector: dot(weights, x) + bias
    pub fn score(&self, features: &[f64]) -> f64 {
        let dot: f64 = self.weights.iter().zip(features.iter())
            .map(|(w, x)| w * x)
            .sum();
        dot + self.bias
    }

    /// Binary prediction from the frozen rule
    pub fn predict(&self, features: &[f64]) -> bool {
        self.score(features) > self.threshold
    }

    /// Number of parameters (all frozen)
    pub fn num_params(&self) -> usize {
        self.weights.len() + 1 // weights + bias
    }
}

// =============================================================================
// LoRA Delta (the low-rank adaptation)
// =============================================================================

/// Low-Rank Adaptation: delta = A · B
/// where A is [dim × rank] and B is [rank × 1] (for single-output rules).
///
/// Total trainable params = dim * rank + rank = rank * (dim + 1)
/// vs full unfreeze = dim + 1
///
/// When rank << dim, LoRA is much cheaper to train and prevents
/// catastrophic forgetting of the frozen rule's knowledge.
#[derive(Debug, Clone)]
pub struct LoRADelta {
    /// A matrix: [dim × rank], flattened row-major
    pub a: Vec<f64>,
    /// B vector: [rank × 1]
    pub b: Vec<f64>,
    /// Dimensions
    pub dim: usize,
    pub rank: usize,
    /// Scaling factor (α/r from the paper)
    pub alpha: f64,
}

impl LoRADelta {
    /// Initialize: A ~ small random, B = zeros (starts as identity)
    pub fn new(dim: usize, rank: usize, alpha: f64) -> Self {
        let a: Vec<f64> = (0..dim * rank)
            .map(|i| ((i as f64 * 0.7 + 0.3).sin()) * 0.01)
            .collect();
        let b = vec![0.0; rank]; // B starts at zero → delta starts at zero

        Self { a, b, dim, rank, alpha }
    }

    /// Number of trainable parameters
    pub fn num_params(&self) -> usize {
        self.dim * self.rank + self.rank
    }

    /// Compute delta score: x · A · B · (α/r)
    pub fn delta_score(&self, features: &[f64]) -> f64 {
        // Step 1: x · A → [rank]
        let xa: Vec<f64> = (0..self.rank).map(|j| {
            features.iter().enumerate()
                .map(|(i, &x)| x * self.a[i * self.rank + j])
                .sum::<f64>()
        }).collect();

        // Step 2: xa · B → scalar
        let xab: f64 = xa.iter().zip(self.b.iter())
            .map(|(a, b)| a * b)
            .sum();

        // Scale by α/r
        xab * self.alpha / self.rank as f64
    }

    /// Flatten all params into a single vector for optimization
    pub fn params_flat(&self) -> Vec<f64> {
        let mut p = self.a.clone();
        p.extend(&self.b);
        p
    }

    /// Inject flat params back
    pub fn set_params(&mut self, params: &[f64]) {
        let n_a = self.dim * self.rank;
        self.a.copy_from_slice(&params[..n_a]);
        self.b.copy_from_slice(&params[n_a..]);
    }
}

// =============================================================================
// LoRA-Adapted Rule
// =============================================================================

/// The composed rule: score = frozen_score + lora_delta_score
pub struct LoRAAdaptedRule {
    pub frozen: FrozenRule,
    pub lora: LoRADelta,
}

impl LoRAAdaptedRule {
    pub fn new(frozen: FrozenRule, rank: usize, alpha: f64) -> Self {
        let dim = frozen.weights.len();
        let lora = LoRADelta::new(dim, rank, alpha);
        Self { frozen, lora }
    }

    /// Adapted score = frozen + delta
    pub fn score(&self, features: &[f64]) -> f64 {
        self.frozen.score(features) + self.lora.delta_score(features)
    }

    /// Adapted prediction
    pub fn predict(&self, features: &[f64]) -> bool {
        self.score(features) > self.frozen.threshold
    }

    /// Train LoRA on edge cases using finite-difference SGD.
    /// The frozen rule stays FROZEN — only A and B are updated.
    pub fn train_lora(
        &mut self,
        data: &[(Vec<f64>, bool)], // (features, ground_truth_is_vuln)
        epochs: usize,
        lr: f64,
    ) {
        let eps = 1e-5;

        for epoch in 1..=epochs {
            let mut params = self.lora.params_flat();
            let n = params.len();

            // Compute loss
            let loss = self.compute_loss(data);

            // Finite-difference gradient for each LoRA param
            for i in 0..n {
                let old = params[i];

                params[i] = old + eps;
                self.lora.set_params(&params);
                let loss_plus = self.compute_loss(data);

                params[i] = old - eps;
                self.lora.set_params(&params);
                let loss_minus = self.compute_loss(data);

                params[i] = old; // restore
                self.lora.set_params(&params);

                let grad = (loss_plus - loss_minus) / (2.0 * eps);
                params[i] -= lr * grad.clamp(-5.0, 5.0);
            }

            self.lora.set_params(&params);

            if epoch % (epochs / 5).max(1) == 0 {
                let new_loss = self.compute_loss(data);
                let (acc, tp, fp, fn_) = self.evaluate(data);
                info!(
                    "🔧 [LoRA] Epoch {:>3} | Loss: {:.4} → {:.4} | Acc: {:.0}% | TP:{} FP:{} FN:{}",
                    epoch, loss, new_loss, acc * 100.0, tp, fp, fn_
                );
            }
        }
    }

    fn compute_loss(&self, data: &[(Vec<f64>, bool)]) -> f64 {
        data.iter().map(|(features, is_vuln)| {
            let score = self.score(features);
            let target = if *is_vuln { 1.0 } else { -1.0 };
            // Hinge-like loss: max(0, 1 - target * score)
            let margin = 1.0 - target * score;
            margin.max(0.0)
        }).sum::<f64>() / data.len() as f64
    }

    fn evaluate(&self, data: &[(Vec<f64>, bool)]) -> (f64, usize, usize, usize) {
        let mut tp = 0; let mut fp = 0; let mut tn = 0; let mut fn_ = 0;
        for (features, is_vuln) in data {
            let pred = self.predict(features);
            match (pred, *is_vuln) {
                (true, true) => tp += 1,
                (true, false) => fp += 1,
                (false, true) => fn_ += 1,
                (false, false) => tn += 1,
            }
        }
        let acc = (tp + tn) as f64 / (tp + fp + tn + fn_) as f64;
        (acc, tp, fp, fn_)
    }
}

// =============================================================================
// Demo
// =============================================================================

pub fn demo_lora_rules() {
    info!("🔧 [LoRA] ════════════════════════════════════════");
    info!("🔧 [LoRA] LoRA Adaptation for Frozen Security Rules");
    info!("🔧 [LoRA] ────────────────────────────────────────");

    // Step 1: Create a frozen rule (from Symbolic Discovery)
    // This rule detects SQL injection: score = 0.8*has_sql + 0.6*has_user_input - 0.3
    let frozen = FrozenRule {
        name: "SQL_Injection_Frozen".into(),
        weights: vec![0.8, 0.6, 0.0, 0.0, 0.0],
        //             sql   user  auth  test  deps
        bias: -0.3,
        threshold: 0.5,
    };

    info!("🔧 [LoRA] Frozen rule: '{}' ({} params, all frozen)",
        frozen.name, frozen.num_params());

    // Step 2: Test on normal data — frozen rule works well
    let normal_data: Vec<(Vec<f64>, bool)> = vec![
        (vec![1.0, 1.0, 0.0, 0.5, 0.0], true),   // SQL + user input → vuln ✓
        (vec![0.0, 0.0, 1.0, 0.8, 0.0], false),   // auth only → safe ✓
        (vec![1.0, 0.0, 0.0, 0.5, 0.0], false),    // SQL but no user input → safe ✓
        (vec![0.0, 1.0, 0.0, 0.3, 0.0], false),    // user input but no SQL → safe ✓
    ];

    let (acc_normal, _, _, _) = {
        let adapted = LoRAAdaptedRule::new(frozen.clone(), 2, 1.0);
        adapted.evaluate(&normal_data)
    };
    info!("🔧 [LoRA] Frozen rule accuracy on normal data: {:.0}%", acc_normal * 100.0);

    // Step 3: Edge cases where frozen rule FAILS
    let edge_cases: Vec<(Vec<f64>, bool)> = vec![
        // ORM-wrapped SQL that's actually safe (frozen says vuln, but it's not!)
        (vec![1.0, 1.0, 1.0, 0.9, 0.0], false),  // SQL+user but auth=1.0 + test=0.9 → SAFE
        (vec![1.0, 1.0, 1.0, 0.8, 0.0], false),  // same pattern, safe
        // Stored procedure injection (frozen misses: no direct SQL keyword)
        (vec![0.2, 0.8, 0.0, 0.1, 0.0], true),   // low SQL but high user input + low test → VULN
        (vec![0.3, 0.9, 0.0, 0.05, 0.0], true),   // similar edge case
        // Dependency injection (frozen has no dep awareness)
        (vec![0.0, 0.0, 0.0, 0.2, 1.0], true),   // deps=1.0, low test → VULN (supply chain)
        (vec![0.0, 0.0, 0.0, 0.1, 0.8], true),   // deps=0.8, very low test → VULN
    ];

    let all_data: Vec<(Vec<f64>, bool)> = normal_data.iter()
        .chain(edge_cases.iter())
        .cloned()
        .collect();

    let mut adapted = LoRAAdaptedRule::new(frozen.clone(), 3, 4.0);
    let (acc_before, tp_b, fp_b, fn_b) = adapted.evaluate(&all_data);
    info!("🔧 [LoRA] ────────────────────────────────────────");
    info!("🔧 [LoRA] Before LoRA (frozen only): Acc={:.0}% | TP:{} FP:{} FN:{}",
        acc_before * 100.0, tp_b, fp_b, fn_b);
    info!("🔧 [LoRA] Frozen params: {} (immutable)", frozen.num_params());
    info!("🔧 [LoRA] LoRA params:   {} (trainable, rank=3)", adapted.lora.num_params());
    info!("🔧 [LoRA] ────────────────────────────────────────");

    // Step 4: Train LoRA on edge cases
    info!("🔧 [LoRA] Training LoRA delta on edge cases...");
    adapted.train_lora(&all_data, 200, 0.3);

    // Step 5: Evaluate after LoRA
    let (acc_after, tp_a, fp_a, fn_a) = adapted.evaluate(&all_data);
    info!("🔧 [LoRA] ────────────────────────────────────────");
    info!("🔧 [LoRA] After LoRA: Acc={:.0}% | TP:{} FP:{} FN:{}",
        acc_after * 100.0, tp_a, fp_a, fn_a);

    // Step 6: Verify frozen weights unchanged
    let frozen_unchanged = adapted.frozen.weights == frozen.weights
        && adapted.frozen.bias == frozen.bias;
    info!("🔧 [LoRA] Frozen weights unchanged: {} {}",
        frozen_unchanged, if frozen_unchanged { "✅" } else { "❌" });

    // Show LoRA delta magnitude
    let delta_norm: f64 = adapted.lora.a.iter().map(|x| x * x).sum::<f64>()
        + adapted.lora.b.iter().map(|x| x * x).sum::<f64>();
    info!("🔧 [LoRA] Delta ||A||² + ||B||² = {:.6} (small = good)", delta_norm.sqrt());

    let improvement = ((acc_after - acc_before) / acc_before.max(0.01)) * 100.0;
    info!("🔧 [LoRA] Improvement: {:.1}% accuracy gain with just {} extra params!",
        improvement, adapted.lora.num_params());
    info!("🔧 [LoRA] ════════════════════════════════════════");
}
