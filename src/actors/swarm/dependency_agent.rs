//! # 📦 DependencyAgent — Аудит зависимостей (CVE-проверка)
//!
//! Агент проверки crate-зависимостей на известные уязвимости.
//! Парсит `Cargo.toml` из файлов MR и проверяет имена crate
//! по встроенной базе известных CVE.
//!
//! ## Встроенная база CVE (демо)
//!
//! | Crate | CVE | Описание |
//! |-------|-----|----------|
//! | `openssl` | RUSTSEC-2023-0072 | Уязвимость в x509 валидации |
//! | `hyper` | RUSTSEC-2024-0003 | HTTP Request Smuggling |
//! | `regex` | RUSTSEC-2022-0013 | ReDoS при определённых паттернах |
//! | `chrono` | RUSTSEC-2020-0159 | Localtime паника в многопоточности |
//! | `tokio` | RUSTSEC-2023-0005 | Race condition в JoinSet |

use async_trait::async_trait;
use tracing::info;

use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § DependencyAgent — аудит зависимостей
// ─────────────────────────────────────────────────────────────────────────────

/// Запись в базе известных уязвимостей.
struct CveEntry {
    crate_name: &'static str,
    advisory: &'static str,
    description: &'static str,
    severity: &'static str,
}

/// Агент аудита зависимостей: проверяет crate-зависимости из MR на CVE.
pub struct DependencyAgent {
    lifecycle: ActorLifecycle,
    /// Встроенная база уязвимостей (в реальной системе — из RustSec Advisory DB)
    cve_db: Vec<CveEntry>,
}

impl DependencyAgent {
    pub fn new() -> Self {
        let cve_db = vec![
            CveEntry { crate_name: "openssl", advisory: "RUSTSEC-2023-0072", description: "Уязвимость в x509 валидации сертификатов", severity: "🔴 CRITICAL" },
            CveEntry { crate_name: "hyper", advisory: "RUSTSEC-2024-0003", description: "HTTP Request Smuggling через Transfer-Encoding", severity: "🟠 HIGH" },
            CveEntry { crate_name: "regex", advisory: "RUSTSEC-2022-0013", description: "ReDoS при определённых регулярных выражениях", severity: "🟡 MEDIUM" },
            CveEntry { crate_name: "chrono", advisory: "RUSTSEC-2020-0159", description: "Паника localtime_r в многопоточном окружении", severity: "🟡 MEDIUM" },
            CveEntry { crate_name: "tokio", advisory: "RUSTSEC-2023-0005", description: "Race condition в JoinSet при отмене задач", severity: "🟠 HIGH" },
            CveEntry { crate_name: "serde_yaml", advisory: "RUSTSEC-2023-0066", description: "Устаревший: используйте serde_yml", severity: "🟡 MEDIUM" },
            CveEntry { crate_name: "atty", advisory: "RUSTSEC-2024-0001", description: "Устаревший: используйте is-terminal", severity: "⚪ LOW" },
        ];
        Self { lifecycle: ActorLifecycle::Active, cve_db }
    }

    /// Извлечь имена зависимостей из содержимого Cargo.toml.
    fn parse_dependencies(content: &str) -> Vec<String> {
        let mut deps = Vec::new();
        let mut in_deps_section = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // Находим секцию [dependencies]
            if trimmed == "[dependencies]" || trimmed.starts_with("[dependencies.") {
                in_deps_section = true;
                continue;
            }
            // Новая секция — выходим
            if trimmed.starts_with('[') && in_deps_section {
                in_deps_section = false;
                continue;
            }

            if in_deps_section {
                // Парсим формат: `crate_name = "version"` или `crate_name = { ... }`
                if let Some(name) = trimmed.split('=').next() {
                    let dep_name = name.trim().replace('-', "_");
                    if !dep_name.is_empty() && !dep_name.starts_with('#') {
                        deps.push(dep_name);
                    }
                }
            }
        }
        deps
    }
}

#[async_trait]
impl Actor for DependencyAgent {
    fn name(&self) -> &'static str { "DependencyAgent" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        if let Message::MergeRequestCreated { mr } = msg {
            info!("📦 [Dependency:Execute] Аудит зависимостей для MR-{}", mr.mr_id);

            let mut comments = Vec::new();
            let mut total_vulns = 0usize;

            // Проверяем Cargo.toml среди изменённых файлов (или используем корневой)
            let cargo_files: Vec<&String> = mr.changed_files.iter()
                .filter(|f| f.ends_with("Cargo.toml"))
                .collect();

            // Если Cargo.toml не в MR — проверяем корневой
            let files_to_check: Vec<String> = if cargo_files.is_empty() {
                vec!["Cargo.toml".to_string()]
            } else {
                cargo_files.into_iter().cloned().collect()
            };

            for cargo_path in &files_to_check {
                let content = std::fs::read_to_string(cargo_path).unwrap_or_default();
                let deps = Self::parse_dependencies(&content);

                if deps.is_empty() {
                    info!("📦 [Dependency:Execute] Cargo.toml не найден или пуст, используем демо-зависимости");
                    // Демо-зависимости для тестирования
                    let demo_deps = vec!["tokio", "serde", "chrono", "hyper", "regex"];
                    let mut report = format!("### 📦 Аудит зависимостей: `{}`\n\n", cargo_path);
                    report.push_str("| Crate | Advisory | Уровень | Описание |\n|-------|----------|---------|----------|\n");

                    for dep in &demo_deps {
                        for cve in &self.cve_db {
                            if cve.crate_name == *dep {
                                report.push_str(&format!(
                                    "| `{}` | `{}` | {} | {} |\n",
                                    cve.crate_name, cve.advisory, cve.severity, cve.description
                                ));
                                total_vulns += 1;
                            }
                        }
                    }

                    if total_vulns > 0 {
                        report.push_str(&format!("\n> 🚨 **Обнаружено {} уязвимых зависимостей.** Обновите затронутые crate.\n", total_vulns));
                    } else {
                        report.push_str("\n> ✅ **Все зависимости безопасны.** CVE не обнаружены.\n");
                    }
                    comments.push(report);
                    continue;
                }

                let mut report = format!("### 📦 Аудит зависимостей: `{}`\n\n", cargo_path);
                report.push_str("| Crate | Advisory | Уровень | Описание |\n|-------|----------|---------|----------|\n");

                for dep in &deps {
                    for cve in &self.cve_db {
                        if cve.crate_name == dep {
                            report.push_str(&format!(
                                "| `{}` | `{}` | {} | {} |\n",
                                cve.crate_name, cve.advisory, cve.severity, cve.description
                            ));
                            total_vulns += 1;
                        }
                    }
                }

                if total_vulns > 0 {
                    report.push_str(&format!("\n> 🚨 **Обнаружено {} уязвимых зависимостей.** Обновите затронутые crate.\n", total_vulns));
                } else {
                    report.push_str("\n> ✅ **Все зависимости безопасны.** CVE не обнаружены.\n");
                }
                comments.push(report);
            }

            info!("📦 [Dependency:Execute] Найдено {} CVE-уязвимостей", total_vulns);
            return TxResult::SwarmReview { mr_id: mr.mr_id, comments };
        }
        TxResult::Ignored
    }

    async fn complete(&mut self, result: TxResult, ctx: &mut ActorContext) {
        if let TxResult::SwarmReview { mr_id, comments } = result {
            info!("📦 [Dependency:Complete] Отправка отчёта зависимостей для MR-{}", mr_id);
            let _ = ctx.orchestrator_tx.send(Message::ReviewReport { mr_id, comments }).await;
        }
    }
}
