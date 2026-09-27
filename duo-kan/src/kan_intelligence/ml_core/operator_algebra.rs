//! 🔬 Operator Algebra for AST Transforms
//!
//! Inspired by Clojure_KAN `operator_kan.clj`:
//!   - Usual ML learns NUMBERS (parameters).
//!   - Operator KAN learns RULES (transformations on functions).
//!   - Commutator [A,B] = AB - BA checks algebraic structure.
//!
//! For Duo: AST transforms as first-class operators.
//!   - Rename, Inline, Extract, Uppercase, Prefix, Identity
//!   - Compose: (A∘B)(code) = A(B(code))
//!   - Commutator: [Rename,Extract] checks if refactoring order matters
//!   - Learn optimal linear combination of operators for a target transform

use std::sync::Arc;
use tracing::info;

// =============================================================================
// Code Fragment (simplified AST representation)
// =============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct CodeFragment {
    pub functions: Vec<FnDef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDef {
    pub name: String,
    pub body: Vec<String>,
    pub is_public: bool,
}

impl CodeFragment {
    pub fn similarity(&self, other: &CodeFragment) -> f64 {
        if self.functions.len() != other.functions.len() {
            return 0.0;
        }
        let mut matches = 0.0;
        let mut total = 0.0;
        for (a, b) in self.functions.iter().zip(other.functions.iter()) {
            total += 1.0;
            if a.name == b.name { matches += 0.3; }
            if a.is_public == b.is_public { matches += 0.2; }
            if a.body == b.body { matches += 0.5; }
        }
        if total == 0.0 { 1.0 } else { matches / total }
    }
}

// =============================================================================
// AST Operator (first-class transform: Code → Code)
// =============================================================================

#[derive(Clone)]
pub struct AstOperator {
    pub name: String,
    pub transform: Arc<dyn Fn(&CodeFragment) -> CodeFragment + Send + Sync>,
}

impl std::fmt::Debug for AstOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Op({})", self.name)
    }
}

impl AstOperator {
    pub fn apply(&self, code: &CodeFragment) -> CodeFragment {
        (self.transform)(code)
    }
}

// =============================================================================
// Elementary Operators
// =============================================================================

pub fn id_op() -> AstOperator {
    AstOperator {
        name: "Id".into(),
        transform: Arc::new(|code| code.clone()),
    }
}

pub fn rename_op() -> AstOperator {
    AstOperator {
        name: "Rename".into(),
        transform: Arc::new(|code| CodeFragment {
            functions: code.functions.iter().map(|f| FnDef {
                name: format!("{}_v2", f.name),
                body: f.body.clone(),
                is_public: f.is_public,
            }).collect(),
        }),
    }
}

pub fn uppercase_op() -> AstOperator {
    AstOperator {
        name: "Upper".into(),
        transform: Arc::new(|code| CodeFragment {
            functions: code.functions.iter().map(|f| FnDef {
                name: f.name.to_uppercase(),
                body: f.body.clone(),
                is_public: f.is_public,
            }).collect(),
        }),
    }
}

pub fn prefix_op() -> AstOperator {
    AstOperator {
        name: "Prefix".into(),
        transform: Arc::new(|code| CodeFragment {
            functions: code.functions.iter().map(|f| FnDef {
                name: format!("safe_{}", f.name),
                body: f.body.clone(),
                is_public: f.is_public,
            }).collect(),
        }),
    }
}

pub fn extract_op() -> AstOperator {
    AstOperator {
        name: "Extract".into(),
        transform: Arc::new(|code| CodeFragment {
            functions: code.functions.iter().map(|f| FnDef {
                name: f.name.clone(),
                body: f.body.clone(),
                is_public: true,
            }).collect(),
        }),
    }
}

pub fn inline_op() -> AstOperator {
    AstOperator {
        name: "Inline".into(),
        transform: Arc::new(|code| CodeFragment {
            functions: code.functions.iter().map(|f| FnDef {
                name: f.name.clone(),
                body: vec![f.body.join("; ")],
                is_public: f.is_public,
            }).collect(),
        }),
    }
}

pub fn secure_op() -> AstOperator {
    AstOperator {
        name: "Secure".into(),
        transform: Arc::new(|code| CodeFragment {
            functions: code.functions.iter().map(|f| {
                let mut new_body = vec!["// SECURITY: validated input".to_string()];
                new_body.extend(f.body.clone());
                new_body.push("// SECURITY: sanitized output".to_string());
                FnDef { name: f.name.clone(), body: new_body, is_public: f.is_public }
            }).collect(),
        }),
    }
}

// =============================================================================
// Operator Algebra
// =============================================================================

/// Compose: (A∘B)(code) = A(B(code))
pub fn op_compose(a: &AstOperator, b: &AstOperator) -> AstOperator {
    let a_t = a.transform.clone();
    let b_t = b.transform.clone();
    AstOperator {
        name: format!("{}∘{}", a.name, b.name),
        transform: Arc::new(move |code| a_t(&b_t(code))),
    }
}

/// Commutator distance: measures how much [A,B] ≠ 0.
/// Returns 0.0 if they commute perfectly, >0 if order matters.
pub fn commutator_distance(a: &AstOperator, b: &AstOperator, input: &CodeFragment) -> f64 {
    let ab = a.apply(&b.apply(input));
    let ba = b.apply(&a.apply(input));
    1.0 - ab.similarity(&ba)
}

// =============================================================================
// Operator Weight Training
// =============================================================================

pub fn train_operator_weights(
    ops: &[AstOperator],
    input: &CodeFragment,
    target: &CodeFragment,
    epochs: usize,
    lr: f64,
) -> Vec<f64> {
    let n = ops.len();
    let mut weights = vec![1.0 / n as f64; n];

    let sims: Vec<f64> = ops.iter()
        .map(|op| op.apply(input).similarity(target))
        .collect();

    for epoch in 1..=epochs {
        let w_sum: f64 = weights.iter().sum();
        let w_sim: f64 = weights.iter().zip(sims.iter()).map(|(w, s)| w * s).sum();
        let loss = 1.0 - w_sim / w_sum.max(1e-10);

        for i in 0..n {
            let grad = -(sims[i] * w_sum - w_sim) / (w_sum * w_sum).max(1e-10);
            weights[i] -= lr * grad;
            weights[i] = weights[i].max(0.0);
        }

        if epoch % (epochs / 5).max(1) == 0 {
            info!(
                "🔬 [OpAlgebra] Epoch {:>3} | Loss: {:.4} | Weights: [{}]",
                epoch, loss,
                weights.iter().map(|w| format!("{:.3}", w)).collect::<Vec<_>>().join(", ")
            );
        }
    }

    let sum: f64 = weights.iter().sum();
    if sum > 0.0 { for w in weights.iter_mut() { *w /= sum; } }
    weights
}

// =============================================================================
// Demo
// =============================================================================

pub fn demo_operator_algebra() {
    let input = CodeFragment {
        functions: vec![
            FnDef { name: "process_input".into(),  body: vec!["let x = read()".into(), "validate(x)".into()], is_public: false },
            FnDef { name: "run_query".into(),       body: vec!["db.exec(sql)".into()], is_public: false },
            FnDef { name: "send_response".into(),   body: vec!["res.json(data)".into()], is_public: true },
        ],
    };

    info!("🔬 [OpAlgebra] ════════════════════════════════════════");
    info!("🔬 [OpAlgebra] Part 1: Commutator Analysis [A,B] = AB - BA");
    info!("🔬 [OpAlgebra] ────────────────────────────────────────");

    let ops: Vec<(&str, AstOperator)> = vec![
        ("Id",      id_op()),
        ("Rename",  rename_op()),
        ("Upper",   uppercase_op()),
        ("Prefix",  prefix_op()),
        ("Extract", extract_op()),
        ("Inline",  inline_op()),
        ("Secure",  secure_op()),
    ];

    let mut commuting = Vec::new();
    let mut non_commuting = Vec::new();

    for i in 0..ops.len() {
        for j in (i+1)..ops.len() {
            let dist = commutator_distance(&ops[i].1, &ops[j].1, &input);
            let pair = format!("[{}, {}]", ops[i].0, ops[j].0);
            if dist < 0.01 {
                commuting.push(pair);
            } else {
                non_commuting.push((pair, dist));
            }
        }
    }

    info!("🔬 [OpAlgebra] ✅ Commuting pairs (order doesn't matter):");
    for p in &commuting {
        info!("🔬 [OpAlgebra]    {} = 0", p);
    }
    info!("🔬 [OpAlgebra] ⚠️  Non-commuting pairs (order MATTERS!):");
    for (p, d) in &non_commuting {
        info!("🔬 [OpAlgebra]    {} ≠ 0 (dist: {:.3})", p, d);
    }

    let re_dist = commutator_distance(&rename_op(), &extract_op(), &input);
    info!("🔬 [OpAlgebra] ────────────────────────────────────────");
    info!("🔬 [OpAlgebra] Key: [Rename, Extract] = {:.3}", re_dist);
    if re_dist < 0.01 {
        info!("🔬 [OpAlgebra]   → Commute! Rename∘Extract = Extract∘Rename");
    } else {
        info!("🔬 [OpAlgebra]   → DON'T commute! Refactoring order matters!");
    }

    // Part 2: Train operator weights
    info!("🔬 [OpAlgebra] ════════════════════════════════════════");
    info!("🔬 [OpAlgebra] Part 2: Learning Optimal Refactoring");
    info!("🔬 [OpAlgebra] ────────────────────────────────────────");

    let target = CodeFragment {
        functions: vec![
            FnDef { name: "safe_process_input".into(), body: vec!["let x = read()".into(), "validate(x)".into()], is_public: true },
            FnDef { name: "safe_run_query".into(),     body: vec!["db.exec(sql)".into()], is_public: true },
            FnDef { name: "safe_send_response".into(), body: vec!["res.json(data)".into()], is_public: true },
        ],
    };

    let all_ops: Vec<AstOperator> = ops.into_iter().map(|(_, o)| o).collect();
    let op_names = ["Id", "Rename", "Upper", "Prefix", "Extract", "Inline", "Secure"];

    let weights = train_operator_weights(&all_ops, &input, &target, 50, 0.5);

    info!("🔬 [OpAlgebra] Discovered operator weights:");
    for (name, w) in op_names.iter().zip(weights.iter()) {
        if *w > 0.01 {
            info!("🔬 [OpAlgebra]    {:>8}: {:.1}%", name, w * 100.0);
        }
    }

    // Verify composition: Prefix ∘ Extract should be the perfect transform
    let composed = op_compose(&prefix_op(), &extract_op());
    let result = composed.apply(&input);
    let sim = result.similarity(&target);
    info!("🔬 [OpAlgebra] ────────────────────────────────────────");
    info!("🔬 [OpAlgebra] Composed '{}' → similarity: {:.1}%", composed.name, sim * 100.0);
    info!("🔬 [OpAlgebra] ════════════════════════════════════════");
}
