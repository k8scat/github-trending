use std::convert::TryInto;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use reqwest;
use serde::Deserialize;
use unicode_segmentation::UnicodeSegmentation;
use anyhow::{Context, Result};
use log::info;
use redis::AsyncCommands;
use crate::openai::{chat_completion, read_url};

#[derive(Deserialize, Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, Eq))]
pub struct Repo {
    pub author: String,
    pub description: String,
    pub name: String,
}

impl Repo {
    pub fn get_url(&self) -> String {
        format!("https://github.com/{}/{}", self.author, self.name)
    }

    pub async fn get_chinese_description(&self, max_length: usize) -> Result<String> {
        let prompt = format!("Translate into Chinese：{}", self.description);
        let translation = chat_completion(&prompt).await.context("While chat completion")?;
        Ok(truncate(&translation, max_length))
    }

    pub async fn get_content(&self, max_length: usize) -> Result<String> {
        let url = self.get_url();
        let repo_content = read_url(&url).await.context("While reading repo content")?;
        let prompt = format!("假设你是一名资深技术专家，精通各种开源项目，请基于以下开源项目内容写一段简介内容，用中文回答：{}", repo_content);
        let content = chat_completion(&prompt).await?;
        Ok(truncate(&content, max_length))
    }
}

fn truncate(content: &str, max_length: usize) -> String {
    if content.graphemes(true).count() < max_length {
        content.to_string()
    } else {
        format!(
            "{} ...",
            content
                .graphemes(true)
                .take(max_length - 4)
                .collect::<String>()
        )
    }
}

fn parse_trending(html: String) -> Result<Vec<Repo>> {
    // Reference: https://github.com/huchenme/github-trending-api/blob/cf898c27850be407fb3f8dd31a4d1c3256ec6e12/src/functions/utils/fetch.js#L30-L103

    let html = scraper::Html::parse_document(&html);
    let repos = html
        .select(&".Box article.Box-row".try_into().unwrap())
        .filter_map(|repo| {
            let title = repo
                .select(&".h3".try_into().unwrap())
                .next()?
                .text()
                .fold(String::new(), |acc, s| acc + s);
            let mut title_split = title.split('/');

            let author = title_split.next()?.trim().to_string();
            let name = title_split.next()?.trim().to_string();

            let description = repo
                .select(&"p.my-1".try_into().unwrap())
                .next()
                .map(|e| {
                    e.text()
                        .fold(String::new(), |acc, s| acc + s)
                        .trim()
                        .to_string()
                })
                .unwrap_or_default();

            Some(Repo {
                author,
                description,
                name,
            })
        })
        .collect();

    Ok(repos)
}

pub async fn fetch_repos() -> Result<Vec<Repo>> {
    let language = env::var("TRENDING_LANGUAGE").unwrap_or("go".to_string());
    info!("fetching {} repos...", language);

    let url = format!("https://github.com/trending/{}?since=daily", language);
    let resp = reqwest::get(&url)
        .await?
        .text()
        .await?;
    parse_trending(resp)
}

#[inline]
fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub async fn mark_posted_repo(
    conn: &mut redis::aio::Connection,
    repo: &Repo,
    ttl: usize,
) -> Result<()> {
    conn.set_ex(format!("{}/{}", repo.author, repo.name), now_ts(), ttl)
        .await?;
    Ok(())
}

pub async fn is_repo_posted(conn: &mut redis::aio::Connection, repo: &Repo) -> Result<bool> {
    Ok(conn
        .exists(format!("{}/{}", repo.author, repo.name))
        .await?)
}
