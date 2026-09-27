//! 🧬 Evolutionary NAS for Security Rules
//!
//! Inspired by Clojure_KAN `evolution.clj`:
//!   - Gradient descent optimizes NUMBERS (parameters).
//!   - Evolution optimizes STRUCTURE (rule type).
//!
//! In a single population, `Regex`, `AstPattern`, and `TaintFlow` rules
//! coexist and can MUTATE between generations.
//!
//! Algorithm:
//!   1. Initialize N random rules of diverse types
//!   2. Evaluate: fitness = accuracy - false_positive_rate
//!   3. Selection: tournament (top from random triple)
//!   4. Crossover: blend parameters of two parents (same type only)
//!   5. Mutation:
//!      a) Parametric — noise to thresholds (80%)
//!      b) Structural — change rule type entirely (20%)
//!   6. Elitism: best individual always survives

use rand::Rng;
use tracing::info;

// =============================================================================
// Rule Types (the "φ-types" of our security world)
// =============================================================================

/// The structural type of a security rule.
/// Evolution can mutate BETWEEN these types (structural mutation).
#[derive(Debug, Clone, PartialEq)]
pub enum RuleType {
    /// Fast regex-based pattern matching (e.g. `unwrap()`, `unsafe {}`)
    Regex {
        pattern: String,
        severity_weight: f64,
    },
    /// AST-level structural pattern (e.g. "function calls raw SQL without sanitizer")
    AstPattern {
        node_type: String,
        min_depth: usize,
        confidence_threshold: f64,
    },
    /// Data-flow taint analysis (e.g. "user input reaches DB query")
    TaintFlow {
        source_kind: String,
        sink_kind: String,
        max_hops: usize,
        decay_factor: f64,
    },
}

// =============================================================================
// Security Rule Genome (the "individual" in our population)
// =============================================================================

#[derive(Debug, Clone)]
pub struct SecurityRuleGenome {
    pub rule_type: RuleType,
    pub fitness: f64,
    pub true_positive_rate: f64,
    pub false_positive_rate: f64,
    pub generation_born: usize,
}

// =============================================================================
// Simulated Historical MR Dataset (ground truth)
// =============================================================================

/// A simulated historical MR with known vulnerability status.
#[derive(Debug, Clone)]
pub struct HistoricalMR {
    pub id: u64,
    pub has_unwrap: bool,
    pub has_unsafe: bool,
    pub has_raw_sql: bool,
    pub has_user_input_to_db: bool,
    pub taint_depth: usize,
    pub is_truly_vulnerable: bool, // ground truth
}

fn generate_training_dataset() -> Vec<HistoricalMR> {
    let mut rng = rand::rng();
    (0..100).map(|i| {
        let has_raw_sql = rng.random_bool(0.3);
        let has_user_input = rng.random_bool(0.25);
        let has_unsafe = rng.random_bool(0.15);
        let has_unwrap = rng.random_bool(0.4);
        let taint_depth = rng.random_range(0..8);
        
        // Ground truth: truly vulnerable if raw_sql + user_input, or unsafe with deep taint
        let is_vuln = (has_raw_sql && has_user_input) 
                    || (has_unsafe && taint_depth > 4);
        
        HistoricalMR {
            id: i,
            has_unwrap,
            has_unsafe,
            has_raw_sql,
            has_user_input_to_db: has_user_input,
            taint_depth,
            is_truly_vulnerable: is_vuln,
        }
    }).collect()
}

// =============================================================================
// Rule Evaluation (simulate running a rule against an MR)
// =============================================================================

fn rule_predicts_vulnerable(rule: &RuleType, mr: &HistoricalMR) -> bool {
    match rule {
        RuleType::Regex { pattern, severity_weight } => {
            // Simulate: regex matches certain keywords
            let score = if pattern.contains("unwrap") && mr.has_unwrap { 0.6 } else { 0.0 }
                      + if pattern.contains("unsafe") && mr.has_unsafe { 0.8 } else { 0.0 }
                      + if pattern.contains("sql") && mr.has_raw_sql { 0.7 } else { 0.0 };
            score * severity_weight > 0.5
        }
        RuleType::AstPattern { node_type, min_depth: _, confidence_threshold } => {
            let score = if node_type.contains("sql") && mr.has_raw_sql { 0.9 }
                      else if node_type.contains("unsafe") && mr.has_unsafe { 0.85 }
                      else { 0.1 };
            score > *confidence_threshold
        }
        RuleType::TaintFlow { source_kind, sink_kind, max_hops, decay_factor } => {
            let is_tainted = source_kind.contains("user") && mr.has_user_input_to_db
                          && sink_kind.contains("db") && mr.has_raw_sql;
            let hop_ok = mr.taint_depth <= *max_hops;
            let decayed_score = if is_tainted && hop_ok {
                1.0 * decay_factor.powf(mr.taint_depth as f64)
            } else {
                0.0
            };
            decayed_score > 0.3
        }
    }
}

// =============================================================================
// Fitness Evaluation
// =============================================================================

pub fn evaluate_fitness(genome: &mut SecurityRuleGenome, dataset: &[HistoricalMR]) {
    let mut tp = 0usize;
    let mut fp = 0usize;
    let mut tn = 0usize;
    let mut fn_ = 0usize;

    for mr in dataset {
        let predicted = rule_predicts_vulnerable(&genome.rule_type, mr);
        match (predicted, mr.is_truly_vulnerable) {
            (true, true) => tp += 1,
            (true, false) => fp += 1,
            (false, true) => fn_ += 1,
            (false, false) => tn += 1,
        }
    }

    let total_positive = (tp + fn_) as f64;
    let total_negative = (fp + tn) as f64;

    genome.true_positive_rate = if total_positive > 0.0 { tp as f64 / total_positive } else { 0.0 };
    genome.false_positive_rate = if total_negative > 0.0 { fp as f64 / total_negative } else { 0.0 };

    // Fitness = TPR - FPR (maximize detection, minimize false alarms)
    genome.fitness = genome.true_positive_rate - genome.false_positive_rate;
}

// =============================================================================
// Population Initialization
// =============================================================================

fn random_rule_type(rng: &mut impl Rng) -> RuleType {
    let patterns = ["unwrap", "unsafe", "sql", "unwrap|unsafe", "sql|unsafe", "unwrap|sql"];
    let node_types = ["sql_call", "unsafe_block", "raw_query", "exec_stmt"];
    let sources = ["user_input", "http_param", "env_var"];
    let sinks = ["db_query", "exec_cmd", "file_write"];

    match rng.random_range(0..3) {
        0 => RuleType::Regex {
            pattern: patterns[rng.random_range(0..patterns.len())].to_string(),
            severity_weight: 0.3 + rng.random::<f64>() * 1.4,
        },
        1 => RuleType::AstPattern {
            node_type: node_types[rng.random_range(0..node_types.len())].to_string(),
            min_depth: rng.random_range(1..5),
            confidence_threshold: 0.3 + rng.random::<f64>() * 0.6,
        },
        _ => RuleType::TaintFlow {
            source_kind: sources[rng.random_range(0..sources.len())].to_string(),
            sink_kind: sinks[rng.random_range(0..sinks.len())].to_string(),
            max_hops: rng.random_range(2..8),
            decay_factor: 0.5 + rng.random::<f64>() * 0.5,
        },
    }
}

pub fn init_population(size: usize) -> Vec<SecurityRuleGenome> {
    let mut rng = rand::rng();
    (0..size).map(|_| SecurityRuleGenome {
        rule_type: random_rule_type(&mut rng),
        fitness: 0.0,
        true_positive_rate: 0.0,
        false_positive_rate: 0.0,
        generation_born: 0,
    }).collect()
}

// =============================================================================
// Tournament Selection
// =============================================================================

fn tournament_select<'a>(population: &'a [SecurityRuleGenome], k: usize) -> &'a SecurityRuleGenome {
    let mut rng = rand::rng();
    let mut best: Option<&SecurityRuleGenome> = None;
    for _ in 0..k {
        let idx = rng.random_range(0..population.len());
        let candidate = &population[idx];
        if best.is_none() || candidate.fitness > best.unwrap().fitness {
            best = Some(candidate);
        }
    }
    best.unwrap()
}

// =============================================================================
// Crossover (blend parameters if same type)
// =============================================================================

fn crossover(p1: &SecurityRuleGenome, p2: &SecurityRuleGenome, gen: usize) -> SecurityRuleGenome {
    let mut rng = rand::rng();
    let alpha: f64 = 0.3 + rng.random::<f64>() * 0.4;

    let child_rule = match (&p1.rule_type, &p2.rule_type) {
        // Same type: blend parameters
        (RuleType::Regex { pattern: pat1, severity_weight: w1 },
         RuleType::Regex { pattern: _, severity_weight: w2 }) => {
            RuleType::Regex {
                pattern: pat1.clone(),
                severity_weight: alpha * w1 + (1.0 - alpha) * w2,
            }
        }
        (RuleType::AstPattern { node_type: n1, min_depth: d1, confidence_threshold: c1 },
         RuleType::AstPattern { node_type: _, min_depth: d2, confidence_threshold: c2 }) => {
            RuleType::AstPattern {
                node_type: n1.clone(),
                min_depth: if rng.random_bool(0.5) { *d1 } else { *d2 },
                confidence_threshold: alpha * c1 + (1.0 - alpha) * c2,
            }
        }
        (RuleType::TaintFlow { source_kind: s1, sink_kind: sk1, max_hops: h1, decay_factor: d1 },
         RuleType::TaintFlow { source_kind: _, sink_kind: _, max_hops: h2, decay_factor: d2 }) => {
            RuleType::TaintFlow {
                source_kind: s1.clone(),
                sink_kind: sk1.clone(),
                max_hops: if rng.random_bool(0.5) { *h1 } else { *h2 },
                decay_factor: alpha * d1 + (1.0 - alpha) * d2,
            }
        }
        // Different types: take the fitter parent
        _ => {
            if p1.fitness >= p2.fitness { p1.rule_type.clone() } else { p2.rule_type.clone() }
        }
    };

    SecurityRuleGenome {
        rule_type: child_rule,
        fitness: 0.0,
        true_positive_rate: 0.0,
        false_positive_rate: 0.0,
        generation_born: gen,
    }
}

// =============================================================================
// Mutation
// =============================================================================

fn mutate_params(genome: &mut SecurityRuleGenome) {
    let mut rng = rand::rng();
    let mut noise = || (rng.random::<f64>() - 0.5) * 0.2;

    match &mut genome.rule_type {
        RuleType::Regex { severity_weight, .. } => {
            *severity_weight = (*severity_weight + noise()).clamp(0.1, 2.0);
        }
        RuleType::AstPattern { confidence_threshold, min_depth, .. } => {
            *confidence_threshold = (*confidence_threshold + noise()).clamp(0.1, 0.95);
            if rng.random_bool(0.3) {
                *min_depth = (*min_depth as i32 + rng.random_range(-1..=1)).max(1) as usize;
            }
        }
        RuleType::TaintFlow { max_hops, decay_factor, .. } => {
            *decay_factor = (*decay_factor + noise()).clamp(0.3, 0.99);
            if rng.random_bool(0.3) {
                *max_hops = (*max_hops as i32 + rng.random_range(-1..=1)).max(1) as usize;
            }
        }
    }
}

fn mutate_type(genome: &mut SecurityRuleGenome) {
    let mut rng = rand::rng();
    genome.rule_type = random_rule_type(&mut rng);
}

fn mutate(genome: &mut SecurityRuleGenome) {
    let mut rng = rand::rng();
    if rng.random::<f64>() < 0.2 {
        // 20%: Structural mutation — full type change!
        mutate_type(genome);
    } else {
        // 80%: Parametric mutation — tweak thresholds
        mutate_params(genome);
    }
}

// =============================================================================
// Rule Type Display Name
// =============================================================================

fn rule_type_name(rule: &RuleType) -> &'static str {
    match rule {
        RuleType::Regex { .. } => "Regex",
        RuleType::AstPattern { .. } => "AstPattern",
        RuleType::TaintFlow { .. } => "TaintFlow",
    }
}

// =============================================================================
// Evolution Loop
// =============================================================================

pub fn evolve(pop_size: usize, generations: usize) -> SecurityRuleGenome {
    let dataset = generate_training_dataset();
    let mut population = init_population(pop_size);

    info!("🧬 [Evolution] Initialized population of {} genomes across 3 rule types", pop_size);
    info!("🧬 [Evolution] Training dataset: {} historical MRs", dataset.len());

    for gen in 1..=generations {
        // 1. Evaluate fitness for all
        for genome in population.iter_mut() {
            evaluate_fitness(genome, &dataset);
        }

        // 2. Sort by fitness (descending) — elitism: keep the best
        population.sort_by(|a, b| b.fitness.partial_cmp(&a.fitness).unwrap());
        let elite = population[0].clone();

        // 3. Report
        if gen % 5 == 0 || gen == 1 {
            info!(
                "🧬 [Evolution] Gen {:>3} | Best: {:<10} | Fitness: {:.4} | TPR: {:.2} | FPR: {:.2}",
                gen,
                rule_type_name(&elite.rule_type),
                elite.fitness,
                elite.true_positive_rate,
                elite.false_positive_rate
            );
        }

        // 4. Create next generation
        let mut next_gen = vec![elite.clone()]; // Elitism: best always survives

        while next_gen.len() < pop_size {
            let p1 = tournament_select(&population, 3);
            let p2 = tournament_select(&population, 3);
            let mut child = crossover(p1, p2, gen);
            mutate(&mut child);
            next_gen.push(child);
        }

        population = next_gen;
    }

    // Final evaluation
    for genome in population.iter_mut() {
        evaluate_fitness(genome, &dataset);
    }
    population.sort_by(|a, b| b.fitness.partial_cmp(&a.fitness).unwrap());

    let winner = population[0].clone();
    info!("🧬 [Evolution] ════════════════════════════════════════");
    info!("🧬 [Evolution] 🏆 Winner: {} (born gen {})", rule_type_name(&winner.rule_type), winner.generation_born);
    info!("🧬 [Evolution]    Fitness:  {:.4}", winner.fitness);
    info!("🧬 [Evolution]    TPR:      {:.2}%", winner.true_positive_rate * 100.0);
    info!("🧬 [Evolution]    FPR:      {:.2}%", winner.false_positive_rate * 100.0);
    match &winner.rule_type {
        RuleType::Regex { pattern, severity_weight } => {
            info!("🧬 [Evolution]    Pattern:  '{}', weight={:.3}", pattern, severity_weight);
        }
        RuleType::AstPattern { node_type, min_depth, confidence_threshold } => {
            info!("🧬 [Evolution]    Node:     '{}', depth≥{}, conf>{:.3}", node_type, min_depth, confidence_threshold);
        }
        RuleType::TaintFlow { source_kind, sink_kind, max_hops, decay_factor } => {
            info!("🧬 [Evolution]    Flow:     {} → {} (≤{} hops, decay={:.3})", source_kind, sink_kind, max_hops, decay_factor);
        }
    }
    info!("🧬 [Evolution] ════════════════════════════════════════");

    winner
}
