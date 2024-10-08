use url::form_urlencoded;
use async_trait::async_trait;
use super::types::Platform;
use crate::repo::Repo;
use anyhow::{anyhow, Context, Result};
use reqwest_middleware::ClientBuilder;
use reqwest_retry::policies::ExponentialBackoff;
use reqwest_retry::RetryTransientMiddleware;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_LENGTH: usize = 10000;

#[derive(Deserialize, Clone)]
pub struct Zsxq {
    cookie: String,
    group_id: String,
    tags: Option<Vec<String>>,
}

#[async_trait]
impl Platform for Zsxq {
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

        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        let resp_str = client.post(url)
            .timeout(core::time::Duration::from_secs(60))
            .json(&data)
            .header("cookie", &self.cookie)
            .send()
            .await?
            .error_for_status()?
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

    async fn content_by_repo(&self, repo: &Repo) -> Result<String> {
        let url = repo.get_url();
        let tags = self.tags.clone().unwrap_or(vec![]).iter().map(|val| {
            tag(val)
        }).collect::<Vec<String>>().join(" ");
        let length_left = MAX_LENGTH - (url.len() + tags.len());
        let content = repo.get_content(length_left).await.context("While getting repo content")?;
        Ok(format!("{}\n\n{}\n\n{}", content, url, tags))
    }
}

fn urlencode(input: &str) -> String {
    form_urlencoded::byte_serialize(input.as_bytes()).collect()
}

fn tag(name: &str) -> String {
    format!("<e type=\"hashtag\" hid=\"0\" title=\"%23{}%23\" />", urlencode(name))
}
