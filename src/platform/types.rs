use async_trait::async_trait;
use crate::repo::Repo;
use anyhow::Result;

#[async_trait]
pub trait Platform {
    async fn post(&self, content: &str) -> Result<()>;
    async fn content_by_repo(&self, repo: &Repo) -> Result<String>;
}