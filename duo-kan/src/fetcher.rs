use anyhow::Result;
use async_trait::async_trait;

/// The unified Context Fetcher Interface (Mirroring Ignition's Abstract Provider)
#[async_trait]
pub trait ContextProvider: Send + Sync {
    fn name(&self) -> &str;
    
    /// Returns a list of (FilePath, FileContent)
    async fn fetch_files(&self, target_uri: &str) -> Result<Vec<(String, String)>>;
}

/// 1. Local Filesystem Provider (Fallback)
pub struct LocalFsProvider;

#[async_trait]
impl ContextProvider for LocalFsProvider {
    fn name(&self) -> &str { "LocalFs" }
    
    async fn fetch_files(&self, target_uri: &str) -> Result<Vec<(String, String)>> {
        if !target_uri.starts_with("file://") {
            anyhow::bail!("Not a local file URI");
        }
        let path = target_uri.trim_start_matches("file://");
        let content = std::fs::read_to_string(path)?;
        Ok(vec![(path.to_string(), content)])
    }
}

/// 2. GitLab API Provider (Cloud Source)
pub struct GitLabProvider {
    pub token: String,
}

#[async_trait]
impl ContextProvider for GitLabProvider {
    fn name(&self) -> &str { "GitLabAPI" }
    
    async fn fetch_files(&self, target_uri: &str) -> Result<Vec<(String, String)>> {
        if !target_uri.starts_with("gitlab://") {
            anyhow::bail!("Not a GitLab URI");
        }
        
        tracing::info!("📡 [{}] Fetching AST context from remote {}", self.name(), target_uri);
        
        // In reality: Reqwest to GitLab REST API. 
        // For Hackathon Demo: Yield the virtual memory tree.
        Ok(vec![
            ("src/test_api.rs".to_string(), include_str!("../../src/test_api.rs").to_string()),
            ("src/test_database.rs".to_string(), include_str!("../../src/test_database.rs").to_string()),
            ("src/test_frontend.rs".to_string(), include_str!("../../src/test_frontend.rs").to_string()),
        ])
    }
}

/// 3. Slack Thread Provider (ChatOps Context)
pub struct SlackProvider {
    pub bot_token: String,
}

#[async_trait]
impl ContextProvider for SlackProvider {
    fn name(&self) -> &str { "SlackAPI" }
    
    async fn fetch_files(&self, target_uri: &str) -> Result<Vec<(String, String)>> {
        if !target_uri.starts_with("slack://") {
            anyhow::bail!("Not a Slack URI");
        }
        tracing::info!("📡 [{}] Fetching context from thread {}", self.name(), target_uri);
        // Fallback or demo return:
        anyhow::bail!("Slack thread has no attached code snippets")
    }
}

/// 4. GitLab Issues Provider (Planning Context)
pub struct GitLabIssueProvider {
    pub token: String,
}

#[async_trait]
impl ContextProvider for GitLabIssueProvider {
    fn name(&self) -> &str { "GitLabIssue" }
    
    async fn fetch_files(&self, target_uri: &str) -> Result<Vec<(String, String)>> {
        if !target_uri.starts_with("gitlab://issues/") {
            anyhow::bail!("Not a GitLab Issue URI");
        }
        tracing::info!("📡 [{}] Fetching Issue context {}", self.name(), target_uri);
        // For Hackathon Demo
        Ok(vec![
            ("meta/issue.md".to_string(), "# Fix SQL Injection\nThe `raw_query` is unsafe. Migrate to `prepare_query`.".to_string()),
        ])
    }
}

/// The Universal Context Engine (Decoupling actors from storage)
pub struct ContextEngine {
    providers: Vec<Box<dyn ContextProvider>>,
}

impl ContextEngine {
    pub fn new() -> Self {
        Self { providers: Vec::new() }
    }
    
    pub fn register(&mut self, provider: Box<dyn ContextProvider>) {
        self.providers.push(provider);
    }
    
    /// Tries all registered providers sequentially until one succeeds (Ignition Fallback Pattern)
    pub async fn fetch(&self, uri: &str) -> Result<Vec<(String, String)>> {
        for provider in &self.providers {
            if let Ok(files) = provider.fetch_files(uri).await {
                tracing::info!("✅ [ContextEngine] Context acquired successfully via '{}' provider", provider.name());
                return Ok(files);
            }
        }
        anyhow::bail!("All ContextProviders failed to resolve the requested URI: {}", uri)
    }
}
