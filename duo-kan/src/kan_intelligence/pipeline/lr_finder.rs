//! 📈 Leslie Smith LR Finder — Auto-Tuning Thresholds
//!
//! Inspired by Clojure_KAN `learning_rate_finder.clj`:
//!   - Exponentially increase learning rate from min to max
//!   - Track smoothed loss at each step
//!   - Find the steepest descent point → optimal LR
//!   - Stop early if loss diverges
//!
//! For Duo: Instead of tuning LR, we tune CrossoverDetector THRESHOLDS:
//!   - churn_threshold: % file churn to trigger alert
//!   - coverage_threshold: min test coverage %
//!   - confidence_threshold: min vuln confidence score
//!
//! We sweep each threshold exponentially and measure false_positive_rate
//! (our "loss"). The sweet spot is where FPR drops fastest.

use tracing::info;

// =============================================================================
// Threshold Finder Configuration
// =============================================================================

#[derive(Debug, Clone)]
pub struct FinderConfig {
    /// Minimum threshold to sweep from
    pub min_val: f64,
    /// Maximum threshold to sweep to
    pub max_val: f64,
    /// Number of steps in the sweep
    pub num_steps: usize,
    /// Exponential smoothing factor (0..1, higher = smoother)
    pub smoothing: f64,
    /// Divergence factor: stop if loss > best * diverge_factor
    pub diverge_factor: f64,
}

impl Default for FinderConfig {
    fn default() -> Self {
        Self {
            min_val: 0.01,
            max_val: 1.0,
            num_steps: 50,
            smoothing: 0.9,
            diverge_factor: 4.0,
        }
    }
}

// =============================================================================
// Sweep Result
// =============================================================================

#[derive(Debug, Clone)]
pub struct SweepPoint {
    pub threshold: f64,
    pub raw_loss: f64,
    pub smoothed_loss: f64,
    pub gradient: f64,
}

#[derive(Debug, Clone)]
pub struct FinderResult {
    pub name: String,
    pub points: Vec<SweepPoint>,
    /// The threshold at steepest descent
    pub optimal_threshold: f64,
    /// Steepest negative gradient
    pub steepest_gradient: f64,
    /// Index of optimal point
    pub optimal_idx: usize,
}

// =============================================================================
// Simulated Evaluation (would be real CrossoverDetector in production)
// =============================================================================

/// Simulated dataset of MRs with known properties.
#[derive(Debug, Clone)]
pub struct SimulatedMR {
    pub churn_rate: f64,         // 0..1
    pub test_coverage: f64,      // 0..1
    pub vuln_confidence: f64,    // 0..1
    pub is_truly_vulnerable: bool,
}

fn generate_dataset() -> Vec<SimulatedMR> {
    (0..200).map(|i| {
        let t = i as f64 / 200.0;
        // Diverse distributions for each feature
        let churn = ((t * 13.7).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
        let coverage = ((t * 9.3 + 1.5).cos() * 0.4 + 0.5).clamp(0.05, 0.95);
        let confidence = ((t * 11.1 + 0.7).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
        // Ground truth: ~20% vulnerable rate
        let is_vuln = (churn > 0.6 && coverage < 0.4 && confidence > 0.5)
                    || (churn > 0.8 && confidence > 0.7);
        SimulatedMR { churn_rate: churn, test_coverage: coverage, vuln_confidence: confidence, is_truly_vulnerable: is_vuln }
    }).collect()
}

/// Evaluate CrossoverDetector at a given threshold triplet.
/// Returns false_positive_rate (our "loss" — we want to minimize it).
fn evaluate_fp_rate(
    dataset: &[SimulatedMR],
    churn_thresh: f64,
    coverage_thresh: f64,
    confidence_thresh: f64,
) -> f64 {
    let mut fp = 0usize;
    let mut tn = 0usize;

    for mr in dataset {
        let crossover_fires = mr.churn_rate > churn_thresh
            && mr.test_coverage < coverage_thresh
            && mr.vuln_confidence > confidence_thresh;

        if !mr.is_truly_vulnerable {
            if crossover_fires { fp += 1; } else { tn += 1; }
        }
    }

    let total_neg = (fp + tn) as f64;
    if total_neg == 0.0 { 0.0 } else { fp as f64 / total_neg }
}

/// Evaluate true_positive_rate at a given threshold.
fn evaluate_tp_rate(
    dataset: &[SimulatedMR],
    churn_thresh: f64,
    coverage_thresh: f64,
    confidence_thresh: f64,
) -> f64 {
    let mut tp = 0usize;
    let mut fn_ = 0usize;

    for mr in dataset {
        let crossover_fires = mr.churn_rate > churn_thresh
            && mr.test_coverage < coverage_thresh
            && mr.vuln_confidence > confidence_thresh;

        if mr.is_truly_vulnerable {
            if crossover_fires { tp += 1; } else { fn_ += 1; }
        }
    }

    let total_pos = (tp + fn_) as f64;
    if total_pos == 0.0 { 0.0 } else { tp as f64 / total_pos }
}

// =============================================================================
// Core: Threshold Sweep (Leslie Smith algorithm)
// =============================================================================

/// Sweep a single threshold exponentially and find the steepest descent.
///
/// `loss_fn` takes a threshold value and returns a loss (e.g. FP rate).
pub fn find_optimal_threshold<F>(
    name: &str,
    config: &FinderConfig,
    loss_fn: F,
) -> FinderResult
where
    F: Fn(f64) -> f64,
{
    let log_min = config.min_val.ln();
    let log_max = config.max_val.ln();
    let step_size = (log_max - log_min) / config.num_steps as f64;

    let mut points: Vec<SweepPoint> = Vec::with_capacity(config.num_steps);
    let mut smoothed_loss = 0.0;
    let mut best_loss = f64::MAX;

    for i in 0..config.num_steps {
        let threshold = (log_min + step_size * i as f64).exp();
        let raw_loss = loss_fn(threshold);

        // Exponential moving average
        if i == 0 {
            smoothed_loss = raw_loss;
        } else {
            smoothed_loss = config.smoothing * smoothed_loss + (1.0 - config.smoothing) * raw_loss;
        }

        if smoothed_loss < best_loss {
            best_loss = smoothed_loss;
        }

        // Early stopping: divergence
        if smoothed_loss > best_loss * config.diverge_factor && i > 5 {
            break;
        }

        let gradient = if i > 0 {
            smoothed_loss - points.last().unwrap().smoothed_loss
        } else {
            0.0
        };

        points.push(SweepPoint {
            threshold,
            raw_loss,
            smoothed_loss,
            gradient,
        });
    }

    // Find steepest descent (most negative gradient)
    let (optimal_idx, steepest) = points.iter().enumerate()
        .min_by(|(_, a), (_, b)| a.gradient.partial_cmp(&b.gradient).unwrap())
        .map(|(i, p)| (i, p.gradient))
        .unwrap_or((0, 0.0));

    let optimal_threshold = points.get(optimal_idx).map(|p| p.threshold).unwrap_or(config.min_val);

    FinderResult {
        name: name.to_string(),
        points,
        optimal_threshold,
        steepest_gradient: steepest,
        optimal_idx,
    }
}

// =============================================================================
// Visualization (ASCII chart)
// =============================================================================

fn ascii_chart(result: &FinderResult) {
    let max_loss = result.points.iter()
        .map(|p| p.smoothed_loss)
        .fold(0.0_f64, f64::max)
        .max(0.01);
    let width = 40;

    info!("📈 [LRFinder] ┌{:─>width$}┐", "", width = width + 2);
    for (i, p) in result.points.iter().enumerate() {
        let bar_len = ((p.smoothed_loss / max_loss) * width as f64) as usize;
        let bar: String = "█".repeat(bar_len.min(width));
        let marker = if i == result.optimal_idx { " ◀ OPTIMAL" } else { "" };
        info!("📈 [LRFinder] │ {:.3} │{:<width$}│{}",
            p.threshold, bar, marker, width = width);
    }
    info!("📈 [LRFinder] └{:─>width$}┘", "", width = width + 2);
}

// =============================================================================
// Demo
// =============================================================================

pub fn demo_lr_finder() {
    let dataset = generate_dataset();
    let truly_vuln = dataset.iter().filter(|m| m.is_truly_vulnerable).count();
    let total = dataset.len();

    info!("📈 [LRFinder] ════════════════════════════════════════");
    info!("📈 [LRFinder] Leslie Smith Threshold Finder for CrossoverDetector");
    info!("📈 [LRFinder] Dataset: {} MRs ({} truly vulnerable, {:.1}%)",
        total, truly_vuln, truly_vuln as f64 / total as f64 * 100.0);
    info!("📈 [LRFinder] ────────────────────────────────────────");

    // Fixed defaults for the other two thresholds while sweeping one
    let default_churn = 0.5;
    let default_coverage = 0.3;
    let default_confidence = 0.6;

    // ── Sweep 1: Churn Threshold ──
    let churn_config = FinderConfig {
        min_val: 0.05,
        max_val: 0.95,
        num_steps: 30,
        ..Default::default()
    };
    let churn_result = find_optimal_threshold("churn_threshold", &churn_config, |thresh| {
        evaluate_fp_rate(&dataset, thresh, default_coverage, default_confidence)
    });

    info!("📈 [LRFinder] ── Sweeping: churn_threshold ──");
    ascii_chart(&churn_result);
    info!("📈 [LRFinder]   Optimal churn_threshold: {:.3} (gradient: {:.4})",
        churn_result.optimal_threshold, churn_result.steepest_gradient);

    // ── Sweep 2: Coverage Threshold ──
    let coverage_config = FinderConfig {
        min_val: 0.05,
        max_val: 0.95,
        num_steps: 30,
        ..Default::default()
    };
    let coverage_result = find_optimal_threshold("coverage_threshold", &coverage_config, |thresh| {
        evaluate_fp_rate(&dataset, default_churn, thresh, default_confidence)
    });

    info!("📈 [LRFinder] ── Sweeping: coverage_threshold ──");
    ascii_chart(&coverage_result);
    info!("📈 [LRFinder]   Optimal coverage_threshold: {:.3} (gradient: {:.4})",
        coverage_result.optimal_threshold, coverage_result.steepest_gradient);

    // ── Sweep 3: Confidence Threshold ──
    let confidence_config = FinderConfig {
        min_val: 0.05,
        max_val: 0.95,
        num_steps: 30,
        ..Default::default()
    };
    let confidence_result = find_optimal_threshold("confidence_threshold", &confidence_config, |thresh| {
        evaluate_fp_rate(&dataset, default_churn, default_coverage, thresh)
    });

    info!("📈 [LRFinder] ── Sweeping: confidence_threshold ──");
    ascii_chart(&confidence_result);
    info!("📈 [LRFinder]   Optimal confidence_threshold: {:.3} (gradient: {:.4})",
        confidence_result.optimal_threshold, confidence_result.steepest_gradient);

    // ── Final combined evaluation ──
    info!("📈 [LRFinder] ════════════════════════════════════════");
    info!("📈 [LRFinder] Combined Optimal CrossoverDetector Config:");
    info!("📈 [LRFinder]   churn_threshold:      {:.3}", churn_result.optimal_threshold);
    info!("📈 [LRFinder]   coverage_threshold:   {:.3}", coverage_result.optimal_threshold);
    info!("📈 [LRFinder]   confidence_threshold: {:.3}", confidence_result.optimal_threshold);

    let fp_before = evaluate_fp_rate(&dataset, default_churn, default_coverage, default_confidence);
    let tp_before = evaluate_tp_rate(&dataset, default_churn, default_coverage, default_confidence);
    let fp_after = evaluate_fp_rate(&dataset,
        churn_result.optimal_threshold,
        coverage_result.optimal_threshold,
        confidence_result.optimal_threshold);
    let tp_after = evaluate_tp_rate(&dataset,
        churn_result.optimal_threshold,
        coverage_result.optimal_threshold,
        confidence_result.optimal_threshold);

    info!("📈 [LRFinder] ────────────────────────────────────────");
    info!("📈 [LRFinder] Before auto-tuning: FPR={:.1}% TPR={:.1}%", fp_before * 100.0, tp_before * 100.0);
    info!("📈 [LRFinder] After  auto-tuning: FPR={:.1}% TPR={:.1}%", fp_after * 100.0, tp_after * 100.0);
    let fp_improvement = if fp_before > 0.0 { ((fp_before - fp_after) / fp_before) * 100.0 } else { 0.0 };
    info!("📈 [LRFinder] FPR reduction: {:.1}%", fp_improvement);
    info!("📈 [LRFinder] ════════════════════════════════════════");
}
