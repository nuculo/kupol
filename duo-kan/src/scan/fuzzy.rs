//! Nucleus Fuzzy Matching — Probabilistic Scan Engine
//!
//! Вдохновлён `lib/kat/sampler.ml` из OCaml KAN.
//! Вместо бинарного match/no-match, каждое правило возвращает
//! вероятность (0.0 — 1.0). Fuzzy matcher ловит обфусцированный код,
//! опечатки в API-вызовах и нестандартное форматирование.

use std::collections::BinaryHeap;
use std::cmp::Ordering;

/// Fuzzy match результат
#[derive(Debug, Clone)]
pub struct FuzzyMatch {
    pub pattern: String,
    pub matched_text: String,
    pub similarity: f64,    // 0.0 — 1.0
    pub position: usize,    // позиция в строке
}

impl PartialEq for FuzzyMatch {
    fn eq(&self, other: &Self) -> bool { self.similarity == other.similarity }
}
impl Eq for FuzzyMatch {}
impl PartialOrd for FuzzyMatch {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.similarity.partial_cmp(&other.similarity)
    }
}
impl Ord for FuzzyMatch {
    fn cmp(&self, other: &Self) -> Ordering {
        self.similarity.partial_cmp(&other.similarity).unwrap_or(Ordering::Equal)
    }
}

/// Nucleus Fuzzy Scanner — "мягкий" сканер
pub struct NucleusFuzzyScanner {
    /// Паттерны для fuzzy matching
    patterns: Vec<FuzzyPattern>,
    /// Temperature: < 1.0 = более строгий, > 1.0 = более мягкий
    pub temperature: f64,
    /// Top-K: максимальное число кандидатов перед Top-P фильтрацией
    pub top_k: usize,
    /// Top-P (Nucleus): порог кумулятивной вероятности
    pub top_p: f64,
}

/// Паттерн для fuzzy matching
#[derive(Debug, Clone)]
pub struct FuzzyPattern {
    pub name: String,
    pub exact: String,
    pub severity: String,
    pub cwe: String,
}

impl NucleusFuzzyScanner {
    /// Создать сканер с параметрами sampling
    pub fn new(temperature: f64, top_k: usize, top_p: f64) -> Self {
        Self {
            patterns: Vec::new(),
            temperature,
            top_k,
            top_p,
        }
    }

    /// Production preset: строгий
    pub fn production() -> Self {
        let mut s = Self::new(0.5, 10, 0.9);
        s.load_default_patterns();
        s
    }

    /// Research preset: мягкий (ловит больше, но больше FP)
    pub fn research() -> Self {
        let mut s = Self::new(1.5, 20, 0.95);
        s.load_default_patterns();
        s
    }

    /// Добавить паттерн
    pub fn add_pattern(&mut self, name: &str, exact: &str, severity: &str, cwe: &str) {
        self.patterns.push(FuzzyPattern {
            name: name.to_string(),
            exact: exact.to_string(),
            severity: severity.to_string(),
            cwe: cwe.to_string(),
        });
    }

    /// Загрузить дефолтные паттерны OWASP
    fn load_default_patterns(&mut self) {
        self.add_pattern("eval-injection", "eval(", "CRITICAL", "CWE-78");
        self.add_pattern("exec-command", "exec(", "CRITICAL", "CWE-78");
        self.add_pattern("system-call", "system(", "CRITICAL", "CWE-78");
        self.add_pattern("innerHTML-xss", "innerHTML", "HIGH", "CWE-79");
        self.add_pattern("sql-select", "SELECT * FROM", "HIGH", "CWE-89");
        self.add_pattern("sql-drop", "DROP TABLE", "CRITICAL", "CWE-89");
        self.add_pattern("md5-weak", "md5(", "MEDIUM", "CWE-327");
        self.add_pattern("sha1-weak", "sha1(", "MEDIUM", "CWE-327");
        self.add_pattern("hardcoded-pass", "password =", "HIGH", "CWE-798");
        self.add_pattern("hardcoded-secret", "secret =", "HIGH", "CWE-798");
    }

    /// Вычислить fuzzy similarity между pattern и substring
    fn fuzzy_similarity(pattern: &str, candidate: &str) -> f64 {
        if pattern == candidate {
            return 1.0;
        }

        let pat_lower = pattern.to_lowercase();
        let cand_lower = candidate.to_lowercase();

        // Exact (case-insensitive)
        if pat_lower == cand_lower {
            return 0.98;
        }

        // Содержит (substring)
        if cand_lower.contains(&pat_lower) {
            return 0.90;
        }

        // Levenshtein-based similarity
        let distance = Self::levenshtein(&pat_lower, &cand_lower);
        let max_len = pat_lower.len().max(cand_lower.len());
        if max_len == 0 { return 0.0; }

        let sim = 1.0 - (distance as f64 / max_len as f64);
        sim.max(0.0)
    }

    /// Levenshtein distance (edit distance)
    fn levenshtein(a: &str, b: &str) -> usize {
        let a_chars: Vec<char> = a.chars().collect();
        let b_chars: Vec<char> = b.chars().collect();
        let n = a_chars.len();
        let m = b_chars.len();

        let mut dp = vec![vec![0usize; m + 1]; n + 1];
        for i in 0..=n { dp[i][0] = i; }
        for j in 0..=m { dp[0][j] = j; }

        for i in 1..=n {
            for j in 1..=m {
                let cost = if a_chars[i-1] == b_chars[j-1] { 0 } else { 1 };
                dp[i][j] = (dp[i-1][j] + 1)
                    .min(dp[i][j-1] + 1)
                    .min(dp[i-1][j-1] + cost);
            }
        }
        dp[n][m]
    }

    /// Сканировать строку — вернуть все fuzzy matches после Nucleus filtering
    pub fn scan_line(&self, line: &str) -> Vec<FuzzyMatch> {
        let mut all_matches: BinaryHeap<FuzzyMatch> = BinaryHeap::new();

        // Для каждого паттерна проверяем каждое возможное окно в строке
        for pat in &self.patterns {
            let pat_len = pat.exact.len();
            if pat_len == 0 || line.len() < pat_len { continue; }

            let mut best_sim = 0.0_f64;
            let mut best_pos = 0;
            let mut best_text = String::new();

            // Sliding window: проверяем каждую подстроку длины pat_len ± 3
            for window_size in pat_len.saturating_sub(3)..=(pat_len + 5).min(line.len()) {
                for start in 0..=(line.len().saturating_sub(window_size)) {
                    let candidate = &line[start..start + window_size];
                    let sim = Self::fuzzy_similarity(&pat.exact, candidate);
                    if sim > best_sim {
                        best_sim = sim;
                        best_pos = start;
                        best_text = candidate.to_string();
                    }
                }
            }

            // Temperature scaling
            let scaled_sim = (best_sim / self.temperature).min(1.0);

            if scaled_sim > 0.3 { // Minimum threshold
                all_matches.push(FuzzyMatch {
                    pattern: pat.name.clone(),
                    matched_text: best_text,
                    similarity: scaled_sim,
                    position: best_pos,
                });
            }
        }

        // Top-K фильтрация
        let mut sorted: Vec<FuzzyMatch> = Vec::new();
        while let Some(m) = all_matches.pop() {
            sorted.push(m);
            if sorted.len() >= self.top_k { break; }
        }

        // Top-P (Nucleus) фильтрация
        let total_sim: f64 = sorted.iter().map(|m| m.similarity).sum();
        if total_sim == 0.0 { return Vec::new(); }

        let mut cumulative = 0.0;
        let mut nucleus: Vec<FuzzyMatch> = Vec::new();

        for m in sorted {
            let prob = m.similarity / total_sim;
            cumulative += prob;
            nucleus.push(m);
            if cumulative >= self.top_p {
                break;
            }
        }

        nucleus
    }

    /// Количество паттернов
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }
}
