use std::convert::TryInto;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use reqwest;
use serde::Deserialize;
use unicode_segmentation::UnicodeSegmentation;
use anyhow::Result;
use log::info;
use redis::AsyncCommands;
use serde_json::{json, Value};

#[derive(Deserialize, Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, Eq))]
pub struct Repo {
    pub author: String,
    pub description: String,
    pub name: String,
    pub stars: usize,
}

impl Repo {
    pub fn get_url(&self) -> String {
        format!("https://github.com/{}/{}", self.author, self.name)
    }

    pub fn get_stars(&self) -> String {
        format!("⭐️{}", self.stars)
    }

    pub async fn get_desc(&self, max_length: usize) -> Result<String> {
        let url = self.get_url();
        let repo_content = read_url(&url).await.unwrap_or_default();
        let description = desc_by_ai(&repo_content)
            .await
            .unwrap_or_default();

        if description.graphemes(true).count() < max_length {
            Ok(description)
        } else {
            Ok(format!(
                "{} ...",
                description
                    .graphemes(true)
                    .take(max_length - 4)
                    .collect::<String>()
            ))
        }
    }
}

// 调用 r.jina.ai 接口读取 github repo 地址的内容
async fn read_url(url: &str) -> Result<String> {
    let url = format!("https://r.jina.ai/{}", url);
    let client = reqwest::Client::new();
    let resp = client.get(url).send().await?.text().await?;
    Ok(resp)
}

async fn desc_by_ai(content: &str) -> Result<String> {
    // Call the OpenAI API to generate a description based on the content
    // Replace the following placeholders with your OpenAI API credentials and endpoint
    let api_key = env::var("OPENAI_API_KEY")?;
    let endpoint = "https://api.openai-all.com/v1/chat/completions";

    let client = reqwest::Client::new();
    let resp = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "model": "gemini-1.5-pro",
            "messages": [
                {"role": "user", "content": format!("假设你是一名资深Go语言技术专家，精通Go语言的开源项目，请基于以下开源项目内容写一段简介内容，用中文回答：\n{}", content)}
            ],
        }))
        .send()
        .await?
        .json::<Value>()
        .await?;
    
    let description = resp["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    Ok(description)
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

            let stars_text = repo
                .select(&".mr-3 svg[aria-label='star']".try_into().unwrap())
                .next()
                .and_then(|e| e.parent())
                .and_then(scraper::ElementRef::wrap)
                .map(|e| {
                    e.text()
                        .fold(String::new(), |acc, s| acc + s)
                        .trim()
                        .replace(',', "")
                })
                .unwrap_or_default();
            let stars = stars_text.parse().unwrap_or(0);

            Some(Repo {
                author,
                description,
                name,
                stars,
            })
        })
        .collect();

    Ok(repos)
}

pub async fn fetch_repos() -> Result<Vec<Repo>> {
    info!("fetching repos...");
    let resp = reqwest::get("https://github.com/trending/go?since=daily")
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
