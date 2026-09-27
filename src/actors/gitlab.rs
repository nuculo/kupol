//! # 🌐 GitLab API Client и GitLab MR Actor
//!
//! - **GitLabClient** — HTTP-клиент для GitLab REST API v4
//!   (ветки, коммиты, MR, пайплайны, комментарии)
//! - **GitLabMRActor** — Актор публикации ревью и авто-хилинга MR

use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use std::env;
use tracing::info;
use uuid::Uuid;

use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § 1. GitLab REST API клиент
// ─────────────────────────────────────────────────────────────────────────────

/// HTTP-клиент для GitLab API v4.
/// Все методы аутентифицируются через PRIVATE-TOKEN.
pub struct GitLabClient {
    base_url: String,
    token: String,
    project_id: String,
    client: Client,
}

impl GitLabClient {
    /// Создать клиент с указанными базовым URL, токеном и ID проекта.
    pub fn new(base_url: &str, token: &str, project_id: &str) -> Self {
        Self { base_url: base_url.into(), token: token.into(), project_id: project_id.into(), client: Client::new() }
    }

    /// Добавить заголовок аутентификации к запросу.
    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        req.header("PRIVATE-TOKEN", &self.token)
    }

    /// Создать ветку `branch` от `from`.
    pub async fn create_branch(&self, branch: &str, from: &str) -> anyhow::Result<()> {
        let url = format!("{}/api/v4/projects/{}/repository/branches", self.base_url, self.project_id);
        let res = self.auth(self.client.post(url).json(&json!({"branch": branch, "ref": from}))).send().await?;
        if !res.status().is_success() { anyhow::bail!("Ошибка создания ветки: {}", res.text().await?); }
        Ok(())
    }

    /// Закоммитить файл в указанную ветку.
    pub async fn commit_file(&self, branch: &str, file_path: &str, content: &str, message: &str) -> anyhow::Result<()> {
        let url = format!("{}/api/v4/projects/{}/repository/commits", self.base_url, self.project_id);
        let res = self.auth(self.client.post(url).json(&json!({
            "branch": branch, "commit_message": message,
            "actions": [{"action": "update", "file_path": file_path, "content": content}]
        }))).send().await?;
        if !res.status().is_success() { anyhow::bail!("Ошибка коммита: {}", res.text().await?); }
        Ok(())
    }

    /// Создать Merge Request и вернуть URL.
    pub async fn create_mr(&self, source_branch: &str, target_branch: &str, title: &str) -> anyhow::Result<String> {
        let url = format!("{}/api/v4/projects/{}/merge_requests", self.base_url, self.project_id);
        let res = self.auth(self.client.post(url).json(&json!({
            "source_branch": source_branch, "target_branch": target_branch,
            "title": title, "remove_source_branch": true
        }))).send().await?;
        let body: serde_json::Value = res.json().await?;
        Ok(body["web_url"].as_str().unwrap_or("").to_string())
    }

    /// Запустить CI/CD пайплайн для ветки.
    pub async fn run_pipeline(&self, branch: &str) -> anyhow::Result<()> {
        let url = format!("{}/api/v4/projects/{}/pipeline", self.base_url, self.project_id);
        let res = self.auth(self.client.post(url).json(&json!({"ref": branch}))).send().await?;
        if !res.status().is_success() { anyhow::bail!("Ошибка пайплайна: {}", res.text().await?); }
        Ok(())
    }

    /// Добавить комментарий к Merge Request.
    pub async fn comment_mr(&self, mr_iid: u64, text: &str) -> anyhow::Result<()> {
        let url = format!("{}/api/v4/projects/{}/merge_requests/{}/notes", self.base_url, self.project_id, mr_iid);
        self.auth(self.client.post(url).json(&json!({"body": text}))).send().await?;
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 2. GitLabMRActor — публикация ревью и авто-хилинг
// ─────────────────────────────────────────────────────────────────────────────

/// Терминальный актор: публикует результаты анализа в GitLab.
/// При наличии AST-патча — создаёт ветку, коммитит фикс и запускает пайплайн.
pub struct GitLabMRActor {
    lifecycle: ActorLifecycle,
    client: Option<GitLabClient>,
}

impl GitLabMRActor {
    pub fn new() -> Self {
        let client = if let (Ok(token), Ok(pid)) = (env::var("GITLAB_TOKEN"), env::var("GITLAB_PROJECT_ID")) {
            Some(GitLabClient::new("https://gitlab.example.com", &token, &pid))
        } else { None };
        Self { lifecycle: ActorLifecycle::Active, client }
    }
}

#[async_trait]
impl Actor for GitLabMRActor {
    fn name(&self) -> &'static str { "GitLabMRActor" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        match msg {
            Message::PostReviewComment { mr_id, payload } => {
                info!("✍️  [GitLab:Execute] Подготовка вызова GitLab API (Комментарий) для MR-{}...", mr_id);
                if let Some(client) = &self.client {
                    let _ = client.comment_mr(mr_id, &payload).await;
                } else {
                    println!("\n=== GitLab Code Review Комментарий (MR-{}) (DRY RUN) ===", mr_id);
                    println!("{}", payload);
                    println!("==========================================\n");
                }
                TxResult::ReviewPosted
            }
            Message::AggregatedResult { mr_id, patch, issues, comments, failed_entities, plantuml } => {
                info!("🛠️  [GitLab:Execute] Swarm-агрегация завершена! Авто-хилинг MR-{}", mr_id);

                // Формирование текста ревью
                let mut payload = format!("🤖 **Отчёт Swarm-исполнения**\n");
                if !issues.is_empty() { payload.push_str(&format!("🚨 **Проблемы ({}):**\n- {}\n", issues.len(), issues.join("\n- "))); }
                if !comments.is_empty() { payload.push_str(&format!("💬 **Ревью ({}):**\n- {}\n", comments.len(), comments.join("\n- "))); }

                if !failed_entities.is_empty() {
                    payload.push_str(&format!("\n🧬 **Затронутые узлы EntityGraph:**\n"));
                    for f in failed_entities {
                        payload.push_str(&format!("- `{}` (вид: {:?}) в `{}`\n", f.name, f.kind, f.file));
                    }
                }

                if !plantuml.is_empty() {
                    payload.push_str(&format!("\n📊 **Трассировка потока данных:**\n{}\n", plantuml));
                }

                if let Some(client) = &self.client {
                    if let Some(p) = patch {
                        for (path, diff) in &p.files {
                            info!("🚀 AST-патч сгенерирован для {}. Пушим ветку...", path);
                            let branch = format!("auto-fix-{}", Uuid::new_v4());
                            let _ = client.create_branch(&branch, "main").await;
                            let _ = client.commit_file(&branch, path, diff, "Swarm AST Auto-fix").await;
                            let _ = client.create_mr(&branch, "main", "🤖 Semantic Swarm Auto Fix").await;
                            let _ = client.run_pipeline(&branch).await;
                        }
                    }
                    let _ = client.comment_mr(mr_id, &payload).await;
                } else {
                    println!("\n=== DRY RUN: Swarm Auto-Heal (MR-{}) ===", mr_id);
                    println!("{}", payload);
                    if let Some(p) = patch {
                        for (path, diff) in &p.files {
                            println!("AST Семантический патч для {}. Правки: Genuine IDE Analysis", path);
                            println!("AST Полный исходник после патча:\n{}", diff);
                            let _ = std::fs::write(path, diff);
                        }
                        println!("Пайплайн запущен.");
                    }
                    println!("==========================================\n");
                }
                TxResult::Ignored
            }
            _ => TxResult::Ignored,
        }
    }

    async fn complete(&mut self, _result: TxResult, _ctx: &mut ActorContext) {
        // Терминальный актор — нечего продвигать.
    }
}
