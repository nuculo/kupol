//! RAG External Memory — Vulnerability Knowledge Base
//!
//! Вдохновлён `lib/rag/` из OCaml KAN (фаза 36).
//! Вместо хардкода regex-правил, плагины обращаются к базе знаний CVE/CWE.
//! Используем TF-IDF embedding + cosine similarity для поиска ближайших
//! известных уязвимостей. In-memory индекс (аналог Qdrant posting lists).

use std::collections::HashMap;

/// Запись в базе знаний об уязвимости
#[derive(Debug, Clone)]
pub struct VulnEntry {
    pub cve_id: String,
    pub cwe_id: String,
    pub title: String,
    pub description: String,
    pub code_pattern: String,
    pub severity: String,
    /// TF-IDF вектор (вычисляется при индексации)
    vector: Vec<f64>,
}

/// Результат поиска
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub entry: VulnEntry,
    pub similarity: f64,
}

/// TF-IDF Vulnerability Knowledge Base
pub struct VulnKnowledgeBase {
    /// Индексированные записи
    entries: Vec<VulnEntry>,
    /// Словарь: token → IDF weight
    vocabulary: HashMap<String, f64>,
    /// Размерность вектора
    dim: usize,
    /// Порог cosine similarity для автоматического match
    pub match_threshold: f64,
}

impl VulnKnowledgeBase {
    /// Создать пустую базу знаний
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            vocabulary: HashMap::new(),
            dim: 0,
            match_threshold: 0.65,
        }
    }

    /// Создать предзаполненную базу с известными CVE/CWE паттернами
    pub fn with_owasp_top10() -> Self {
        let mut kb = Self::new();

        let raw_entries = vec![
            ("CVE-2024-SQL-001", "CWE-89", "SQL Injection via String Concatenation", "CRITICAL",
             "execute SELECT FROM WHERE concatenation user input query string",
             "db.execute(\"SELECT * FROM users WHERE id=\" + user_input)"),
            ("CVE-2024-XSS-001", "CWE-79", "Reflected XSS via innerHTML", "HIGH",
             "innerHTML document write user input response body html injection",
             "element.innerHTML = user_input"),
            ("CVE-2024-CMD-001", "CWE-78", "OS Command Injection via system()", "CRITICAL",
             "system exec command shell process spawn user input os injection",
             "std::process::Command::new(user_input).output()"),
            ("CVE-2024-PATH-001", "CWE-22", "Path Traversal via unvalidated input", "HIGH",
             "path traversal directory parent dotdot file read open join user",
             "fs::read_to_string(format!(\"data/{}\", user_path))"),
            ("CVE-2024-DESER-001", "CWE-502", "Unsafe Deserialization", "CRITICAL",
             "deserialize untrusted serde bincode pickle marshal from_bytes unsafe",
             "serde_json::from_str::<AdminConfig>(&untrusted_data)"),
            ("CVE-2024-CRYPTO-001", "CWE-327", "Weak Cryptographic Algorithm (MD5/SHA1)", "MEDIUM",
             "md5 sha1 weak hash digest crypto deprecated algorithm",
             "let hash = md5::compute(password)"),
            ("CVE-2024-HARDCODE-001", "CWE-798", "Hardcoded Credentials", "HIGH",
             "password secret key token api hardcoded credential embedded literal",
             "let password = \"admin123\"; let api_key = \"sk-...\""),
            ("CVE-2024-SSRF-001", "CWE-918", "Server-Side Request Forgery", "HIGH",
             "reqwest get url fetch http request user controlled remote server ssrf",
             "reqwest::get(&user_provided_url).await"),
            ("CVE-2024-XXE-001", "CWE-611", "XML External Entity Injection", "HIGH",
             "xml parse external entity dtd doctype expansion billion laughs",
             "xml::parse_with_external_entities(untrusted_xml)"),
            ("CVE-2024-LOG-001", "CWE-117", "Log Injection / Log4Shell style", "MEDIUM",
             "log info warn error format user input interpolation injection jndi",
             "log::info!(\"User login: {}\", untrusted_username)"),
            ("CVE-2024-RACE-001", "CWE-362", "Race Condition / TOCTOU", "MEDIUM",
             "race condition toctou time check use file exists concurrent thread lock",
             "if path.exists() { fs::write(path, data) }"),
            ("CVE-2024-OVERFLOW-001", "CWE-190", "Integer Overflow", "HIGH",
             "integer overflow wrap unchecked add multiply arithmetic cast as u32 i32",
             "let total = (price as u32) * (quantity as u32)"),
        ];

        // Построить vocabulary из всех описаний
        let mut token_doc_freq: HashMap<String, usize> = HashMap::new();
        let total_docs = raw_entries.len();

        for (_, _, _, _, description, _) in &raw_entries {
            let tokens: std::collections::HashSet<String> = Self::tokenize(description)
                .into_iter().collect();
            for token in tokens {
                *token_doc_freq.entry(token).or_insert(0) += 1;
            }
        }

        // IDF: log(N / df)
        let vocabulary: HashMap<String, f64> = token_doc_freq.iter()
            .map(|(token, df)| {
                let idf = (total_docs as f64 / *df as f64).ln() + 1.0;
                (token.clone(), idf)
            })
            .collect();

        let token_list: Vec<String> = {
            let mut v: Vec<String> = vocabulary.keys().cloned().collect();
            v.sort();
            v
        };
        let dim = token_list.len();

        // Индексировать каждую запись
        let entries: Vec<VulnEntry> = raw_entries.into_iter()
            .map(|(cve, cwe, title, severity, desc, pattern)| {
                let vector = Self::embed_with_vocab(desc, &vocabulary, &token_list, dim);
                VulnEntry {
                    cve_id: cve.to_string(),
                    cwe_id: cwe.to_string(),
                    title: title.to_string(),
                    description: desc.to_string(),
                    code_pattern: pattern.to_string(),
                    severity: severity.to_string(),
                    vector,
                }
            })
            .collect();

        kb.entries = entries;
        kb.vocabulary = vocabulary;
        kb.dim = dim;
        kb
    }

    /// Токенизация строки → lowercase words
    fn tokenize(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| s.len() >= 2)
            .map(String::from)
            .collect()
    }

    /// TF-IDF embedding с данным словарём
    fn embed_with_vocab(
        text: &str,
        vocabulary: &HashMap<String, f64>,
        token_list: &[String],
        dim: usize,
    ) -> Vec<f64> {
        let tokens = Self::tokenize(text);
        let mut tf: HashMap<String, f64> = HashMap::new();
        for t in &tokens {
            *tf.entry(t.clone()).or_insert(0.0) += 1.0;
        }
        let total = tokens.len() as f64;

        let mut vector = vec![0.0; dim];
        for (i, token) in token_list.iter().enumerate() {
            if let Some(&count) = tf.get(token) {
                let tf_val = count / total;
                let idf_val = vocabulary.get(token).copied().unwrap_or(1.0);
                vector[i] = tf_val * idf_val;
            }
        }

        // L2 нормализация
        let norm: f64 = vector.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 0.0 {
            for v in &mut vector {
                *v /= norm;
            }
        }
        vector
    }

    /// Cosine Similarity между двумя нормализованными векторами
    fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    /// Поиск Top-K ближайших CVE к данной строке кода
    pub fn search(&self, query: &str, top_k: usize) -> Vec<SearchResult> {
        let token_list: Vec<String> = {
            let mut v: Vec<String> = self.vocabulary.keys().cloned().collect();
            v.sort();
            v
        };

        let query_vec = Self::embed_with_vocab(query, &self.vocabulary, &token_list, self.dim);

        let mut results: Vec<SearchResult> = self.entries.iter()
            .map(|entry| {
                let sim = Self::cosine_similarity(&query_vec, &entry.vector);
                SearchResult {
                    entry: entry.clone(),
                    similarity: sim,
                }
            })
            .filter(|r| r.similarity > 0.0)
            .collect();

        results.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        results.truncate(top_k);
        results
    }

    /// Проверить строку кода и вернуть автоматические findings (similarity > threshold)
    pub fn auto_match(&self, code_line: &str) -> Vec<SearchResult> {
        self.search(code_line, 3)
            .into_iter()
            .filter(|r| r.similarity >= self.match_threshold)
            .collect()
    }

    /// Количество записей в базе
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Размерность вектора
    pub fn dim(&self) -> usize {
        self.dim
    }
}
