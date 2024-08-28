use std::iter::FromIterator;
use std::str::FromStr;
use url::form_urlencoded;
use async_trait::async_trait;
use super::types::Platform;
use crate::repo::Repo;
use anyhow::{anyhow, Result};
use log::info;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_LENGTH: usize = 10000;

#[derive(Deserialize, Clone)]
pub struct Zsxq {
    cookie: String,
    group_id: String,
}

#[async_trait]
impl Platform for Zsxq {
    async fn content_by_repo(&self, repo: &Repo) -> Result<String> {
        let stars = repo.get_stars();
        let url = repo.get_url();
        let tags = vec![tag("Go"), tag("开源项目"), tag("项目推荐")].join(" ");
        let length_left = MAX_LENGTH - (repo.name.len() + stars.len() + url.len() + tags.len());
        let description = repo.get_desc(length_left)
            .await
            .unwrap_or_default();
        info!("{}/{} description: {}", repo.author, repo.name, description);
        Ok(format!("{} Go语言项目推荐 {}：{}\n\n{}\n\n{}", stars, repo.name, description, url, tags))
    }

    async fn post(&self, content: &str) -> Result<()> {
        let url = format!("https://api.zsxq.com/v2/groups/{}/topics", self.group_id);
        let data = json!({
        "req_data": {
            "type": "topic",
            "text": content,
            "image_ids": [],
            "file_ids": [],
            "mentioned_user_ids": []
        }
    });
        let client = reqwest::Client::builder()
            .timeout(core::time::Duration::from_secs(60))
            .default_headers(HeaderMap::from_iter(vec![(
                HeaderName::from_str("cookie")?,
                HeaderValue::from_str(&self.cookie)?,
            )]))
            .build()?;
        let resp_str = client
            .request(Method::POST, url)
            .json(&data)
            .send()
            .await?
            .text()
            .await?;

        let resp: Value = serde_json::from_str(resp_str.as_str())?;
        match resp["succeeded"].as_bool() {
            None => Err(anyhow!("post zsxq failed: {}", resp_str)),
            Some(b) => {
                if b {
                    Ok(())
                } else {
                    Err(anyhow!("post zsxq failed: {}, error: {}", resp_str, resp["error"].to_string()))
                }
            }
        }
    }
}

fn urlencode(input: &str) -> String {
    form_urlencoded::byte_serialize(input.as_bytes()).collect()
}

fn tag(name: &str) -> String {
    format!("<e type=\"hashtag\" hid=\"0\" title=\"%23{}%23\" />", urlencode(name))
}
