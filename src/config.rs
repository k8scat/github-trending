use std::fs::File;
use std::io::Read;
use serde::Deserialize;
use anyhow::Result;
use crate::repo::Repo;
use super::platform::zsxq;

#[derive(Deserialize)]
pub struct Config {
    pub interval: IntervalConfig,
    pub redis: RedisConfig,
    pub denylist: DenylistConfig,
    pub zsxq: Option<zsxq::Zsxq>,
}

#[derive(Deserialize)]
pub struct IntervalConfig {
    pub post_ttl: usize,
    pub fetch_interval: u64,
    pub post_interval: u64,
}

#[derive(Deserialize)]
pub struct RedisConfig {
    pub url: String,
}

#[derive(Deserialize, Debug)]
pub struct DenylistConfig {
    pub names: Vec<String>,
    pub authors: Vec<String>,
    pub descriptions: Vec<String>,
}

impl DenylistConfig {
    pub fn contains(&self, repo: &Repo) -> bool {
        self.names.contains(&repo.name)
            || self.authors.contains(&repo.author)
            || self
            .descriptions
            .iter()
            .map(|description| {
                repo.description
                    .to_lowercase()
                    .contains(&description.to_lowercase())
            })
            .any(|b| b)
    }
}

pub fn read_file(path: &str) -> Result<Config> {
    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(toml::from_str(&content)?)
}
