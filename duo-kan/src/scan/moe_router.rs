//! Sparse Security Router (Inverted Index MoE)
//!
//! Вдохновлён `sparse_router.ml` из OCaml KAN.
//! Вместо линейного `if content.contains(...)` для каждого плагина,
//! мы строим **Inverted Index** (Posting List): keyword → Vec<(plugin, weight)>.
//! При сканировании файла токенизируем содержимое, делаем lookup в индексе,
//! аккумулируем баллы и запускаем только Top-K плагинов.
//! Сложность маршрутизации: O(T) по токенам, вместо O(F × P).

use std::collections::HashMap;

/// Posting List Entry: (plugin_name, relevance_weight)
type PostingEntry = (&'static str, f64);

/// Sparse Security Index — инвертированный индекс для маршрутизации плагинов
pub struct SparseSecurityIndex {
    /// keyword → список (plugin_name, weight)
    inverted_index: HashMap<&'static str, Vec<PostingEntry>>,
    /// Расширения файлов → список (plugin_name, weight)
    extension_index: HashMap<&'static str, Vec<PostingEntry>>,
    /// Плагины, которые всегда активны (universal experts)
    always_on: Vec<&'static str>,
}

impl SparseSecurityIndex {
    /// Построить индекс один раз при старте приложения
    pub fn build() -> Self {
        let mut idx: HashMap<&'static str, Vec<PostingEntry>> = HashMap::new();
        let mut ext_idx: HashMap<&'static str, Vec<PostingEntry>> = HashMap::new();

        // ── SQL Injection Expert ──────────────────────────────────────────
        for kw in &["SELECT", "INSERT", "UPDATE", "DELETE", "query(", "db.", "raw_query", "execute(", "sql.raw"] {
            idx.entry(kw).or_default().push(("sql-injection", 1.0));
        }

        // ── Hardcoded Secrets Expert ──────────────────────────────────────
        for kw in &["sk-", "AKIA", "ghp_", "glpat-", "Bearer ", "password", "api_key", "secret_key", "private_key", "apiKey"] {
            idx.entry(kw).or_default().push(("hardcoded-secrets", 1.5));
        }
        for kw in &["token", "key", "auth", "secret"] {
            idx.entry(kw).or_default().push(("hardcoded-secrets", 0.5));
        }

        // ── Unsafe Code Expert ───────────────────────────────────────────
        for kw in &["unsafe {", "unsafe fn", "*mut ", "*const ", "transmute"] {
            idx.entry(kw).or_default().push(("unsafe-code", 1.5));
        }
        ext_idx.entry("rs").or_default().push(("unsafe-code", 0.8));
        ext_idx.entry("c").or_default().push(("unsafe-code", 0.8));
        ext_idx.entry("cpp").or_default().push(("unsafe-code", 0.8));

        // ── Deprecated API Expert ────────────────────────────────────────
        for kw in &["eval(", "exec(", "innerHTML", "dangerouslySetInnerHTML", "os.system(", "subprocess.call("] {
            idx.entry(kw).or_default().push(("deprecated-api", 1.2));
        }
        ext_idx.entry("js").or_default().push(("deprecated-api", 0.3));
        ext_idx.entry("ts").or_default().push(("deprecated-api", 0.3));
        ext_idx.entry("rs").or_default().push(("deprecated-api", 0.3));

        // ── Crypto Weakness Expert ───────────────────────────────────────
        for kw in &["MD5(", "md5(", "SHA1(", "sha1(", "DES.encrypt", "RC4", "Math.random()", "encrypt", "hash", "crypto"] {
            idx.entry(kw).or_default().push(("crypto-weakness", 1.0));
        }

        // ── Unwrap Panic Expert (Rust) ───────────────────────────────────
        for kw in &[".unwrap()", "panic!(", "unreachable!("] {
            idx.entry(kw).or_default().push(("unwrap-panic", 1.0));
        }
        ext_idx.entry("rs").or_default().push(("unwrap-panic", 0.5));

        // ── Input Validation Expert ──────────────────────────────────────
        for kw in &["from_utf8_unchecked", "from_raw_parts", "parse().unwrap()"] {
            idx.entry(kw).or_default().push(("input-validation", 1.2));
        }

        SparseSecurityIndex {
            inverted_index: idx,
            extension_index: ext_idx,
            always_on: vec!["todo-fixme", "input-validation"],
        }
    }

    /// Маршрутизация файла через Inverted Index.
    /// Возвращает Top-K плагинов, отсортированных по накопленному весу.
    pub fn route(&self, filename: &str, content: &str, top_k: usize) -> Vec<&'static str> {
        let mut scores: HashMap<&'static str, f64> = HashMap::new();

        // 1. Extension boost
        if let Some(ext) = filename.rsplit('.').next() {
            if let Some(postings) = self.extension_index.get(ext) {
                for &(plugin, weight) in postings {
                    *scores.entry(plugin).or_insert(0.0) += weight;
                }
            }
        }

        // 2. Content token scan — O(K × T) where K = avg posting list len
        for (&keyword, postings) in &self.inverted_index {
            if content.contains(keyword) {
                for &(plugin, weight) in postings {
                    *scores.entry(plugin).or_insert(0.0) += weight;
                }
            }
        }

        // 3. Always-on experts get baseline score
        for &plugin in &self.always_on {
            *scores.entry(plugin).or_insert(0.0) += 0.1;
        }

        // 4. Sort by score descending, take Top-K
        let mut ranked: Vec<(&str, f64)> = scores.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        ranked.into_iter()
            .take(top_k)
            .map(|(name, _)| name)
            .collect()
    }
}

/// Legacy compatibility wrapper (delegates to SparseSecurityIndex)
pub struct MoeRouter;

impl MoeRouter {
    pub fn assign_experts(filename: &str, content: &str) -> Vec<&'static str> {
        // Lazy-static would be ideal here; for now we build per-call
        // (still fast: ~50 HashMap inserts)
        let index = SparseSecurityIndex::build();
        index.route(filename, content, 6) // Top-6 experts
    }
}
