use serde::{Serialize, Deserialize};

/// Learnable Activation Function based on B-Splines (Graph KAN Edge Engine)
/// Implements the Cox-de Boor recursion algorithm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BSpline {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub coefs: Vec<f64>,
}

impl BSpline {
    pub fn new(degree: usize, knots: Vec<f64>, coefs: Vec<f64>) -> Self {
        Self { degree, knots, coefs }
    }

    /// Evaluates the spline at risk input `x`
    pub fn eval(&self, x: f64) -> f64 {
        let n = self.knots.len() - self.degree - 1;
        let mut v = vec![0.0; self.degree + 1];
        
        let mut x_clamped = x;
        if x_clamped < self.knots[self.degree] { x_clamped = self.knots[self.degree]; }
        if x_clamped > self.knots[n] { x_clamped = self.knots[n]; }

        let mut span = self.degree;
        for i in self.degree..n {
            if x_clamped >= self.knots[i] && x_clamped < self.knots[i+1] {
                span = i;
                break;
            }
        }
        if x_clamped == self.knots[n] { span = n - 1; }

        for i in 0..=self.degree {
            v[i] = self.coefs[span - self.degree + i];
        }

        for r in 1..=self.degree {
            for i in (r..=self.degree).rev() {
                let j = span - self.degree + i;
                let left = self.knots[j];
                let right = self.knots[j + self.degree - r + 1];
                let alpha = if right == left { 0.0 } else { (x_clamped - left) / (right - left) };
                v[i] = (1.0 - alpha) * v[i - 1] + alpha * v[i];
            }
        }
        v[self.degree]
    }

    /// Linear passthrough: y = x (Risk propagates normally)
    pub fn neutral() -> Self {
        Self::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 1.0])
    }

    /// Explodes the risk (Critical sub-systems, Core DB layers)
    /// Maps 0.5 -> 0.75 (amplifies intermediate risks)
    pub fn amplifier() -> Self {
        Self::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.8, 1.0])
    }

    /// Dampens the risk (Tests, mocks, isolated helpers)
    pub fn dampener() -> Self {
        Self::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.2, 1.0])
    }
}
