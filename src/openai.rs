use std::env;
use reqwest;
use serde_json::{json, Value};
use anyhow::Result;
use reqwest_middleware::ClientBuilder;
use reqwest_retry::policies::ExponentialBackoff;
use reqwest_retry::RetryTransientMiddleware;

pub async fn chat_completion(content: &str) -> Result<String> {
    // Call the OpenAI API to translate the content to Chinese
    // Replace the following placeholders with your OpenAI API credentials and endpoint
    let api_key = env::var("OPENAI_API_KEY")?;
    let api_base = env::var("OPENAI_API_BASE").unwrap_or(String::from("https://api.openai-all.com/v1"));
    let model = env::var("OPENAI_MODEL").unwrap_or(String::from("gemini-1.5-pro"));
    let url = format!("{}/chat/completions", api_base);

    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
    let client = ClientBuilder::new(reqwest::Client::new())
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build();

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "model": model,
            "messages": [
                {"role": "user", "content": content}
            ],
        }))
        .send()
        .await?
        .json::<Value>()
        .await?;

    let result = resp["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    Ok(result)
}

// 调用 r.jina.ai 接口读取 github repo 地址的内容
pub async fn read_url(url: &str) -> Result<String> {
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
    let client = ClientBuilder::new(reqwest::Client::new())
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build();

    let url = format!("https://r.jina.ai/{}", url);
    let resp = client.get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(resp)
}
