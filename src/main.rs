use std::{
    convert::TryInto, env, fs::File, io::Read, iter::FromIterator, sync::Arc, time::{SystemTime, UNIX_EPOCH}
};

use anyhow::{anyhow, Context, Ok, Result};
use atrium_api::{app::bsky, client::AtpServiceClient, com::atproto};
use bytes::Bytes;
use log::{error, info};
use once_cell::sync::Lazy;
use redis::AsyncCommands;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Method,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::str::FromStr;
use teloxide::prelude::*;
use time::OffsetDateTime;
use twitter_v2::{authorization::Oauth1aToken, TwitterApi};
use unicode_segmentation::UnicodeSegmentation;
use url::Url;

const TWEET_LENGTH: usize = 280;
const TOOT_LENGTH: usize = 500;
const BLUESKY_POST_LENGTH: usize = 300;
const MASTODON_FIXED_URL_LENGTH: usize = 23;
// const SMALL_COMMERCIAL_AT: &str = "﹫";
const ZSXQ_LENGTH: usize = 10000;
const WEIBO_LENGTH: usize = 5000;
const TELEGRAM_BOT_LENGTH: usize = 4096;

#[derive(Deserialize)]
struct IntervalConfig {
    post_ttl: usize,
    fetch_interval: u64,
    post_interval: u64,
}

#[derive(Deserialize)]
struct RedisConfig {
    url: String,
}

#[derive(Deserialize, Clone)]
struct TwitterConfig {
    consumer_key: String,
    consumer_secret: String,
    token: String,
    secret: String,
}

#[derive(Deserialize, Clone)]
struct MastodonConfig {
    instance_url: Url,
    access_token: String,
}

#[derive(Deserialize, Clone)]
struct BlueskyConfig {
    host: String,
    identifier: String,
    password: String,
}

#[derive(Deserialize, Clone)]
struct ZsxqConfig {
    cookie: String,
    group_id: String,
}

#[derive(Deserialize, Clone)]
struct WeiboConfig {
    cookie: String,
    xsrf_token: String,
}

#[derive(Deserialize, Clone)]
struct TelegramBotConfig {
    token: String,
    chat_id: i64,
}

#[derive(Deserialize, Debug)]
struct DenylistConfig {
    names: Vec<String>,
    authors: Vec<String>,
    descriptions: Vec<String>,
}

impl DenylistConfig {
    fn contains(&self, repo: &Repo) -> bool {
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

#[derive(Deserialize)]
struct Config {
    interval: IntervalConfig,
    redis: RedisConfig,
    #[serde(default)]
    twitter: Option<TwitterConfig>,
    #[serde(default)]
    mastodon: Option<MastodonConfig>,
    #[serde(default)]
    bluesky: Option<BlueskyConfig>,
    denylist: DenylistConfig,
    zsxq: Option<ZsxqConfig>,
    weibo: Option<WeiboConfig>,
    telegram_bot: Option<TelegramBotConfig>,
}

#[derive(Deserialize, Debug)]
#[cfg_attr(test, derive(Clone, PartialEq, Eq))]
struct Repo {
    author: String,
    description: String,
    name: String,
    stars: usize,
}

impl Repo {
    fn get_url(&self) -> String {
        format!("https://github.com/{}/{}", self.author, self.name)
    }
}

#[inline]
fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn read_config(path: &str) -> Result<Config> {
    let mut file = File::open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(toml::from_str(&content)?)
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

async fn fetch_repos() -> Result<Vec<Repo>> {
    let resp = reqwest::get("https://github.com/trending/rust?since=daily")
        .await?
        .text()
        .await?;
    parse_trending(resp)
}

async fn get_github_og_image(repo: &Repo) -> Result<Bytes> {
    static CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);

    let url = format!(
        "https://opengraph.githubassets.com/{}/{}/{}",
        random_string::generate(64, "0123456789abcdefghijklmnopqrstuvwxyz"),
        repo.author,
        repo.name
    );

    Ok(CLIENT
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?)
}

fn make_repo_title(repo: &Repo) -> String {
    if repo.author != repo.name {
        format!("{} / {}", repo.author, repo.name)
    } else {
        repo.name.clone()
    }
}

fn make_post_prefix(repo: &Repo) -> String {
    format!("{}: ", make_repo_title(repo))
}

fn make_post_stars(repo: &Repo) -> String {
    format!("⭐️{}", repo.stars)
}

fn make_post_url(repo: &Repo) -> String {
    format!(" https://github.com/{}/{}", repo.author, repo.name)
}

fn repo_uri(repo: &Repo) -> String {
    format!("https://github.com/{}/{}", repo.author, repo.name)
}

// 调用 r.jina.ai 接口读取 github repo 地址的内容
async fn fetch_repo_content(url: &str) -> Result<String> {
    let url = format!("https://r.jina.ai/{}", url);
    let client = reqwest::Client::new();
    let resp = client.get(url).send().await?.text().await?;
    Ok(resp)
}

async fn generate_description(content: &str) -> Result<String> {
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
                {"role": "user", "content": format!("假设你是一名资深技术专家，精通开源项目，请对以下开源项目进行详细总结：\n{}", content)}
            ],
        }))
        .send()
        .await?
        .json::<Value>()
        .await?;

    let description = resp["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_string();
    Ok(description)
}


async fn make_post_description(repo: &Repo, length_left: usize) -> Result<String> {
    let url = repo.get_url();
    let repo_content = fetch_repo_content(&url).await.unwrap_or_default();
    let description = generate_description(&repo_content).await.unwrap_or_default();

    // let description = repo.description.replace('@', SMALL_COMMERCIAL_AT);
    if description.graphemes(true).count() < length_left {
        Ok(description)
    } else {
        Ok(format!(
            "{} ...",
            description
                .graphemes(true)
                .take(length_left - 4)
                .collect::<String>()
        ))
    }
}

async fn make_tweet(repo: &Repo) -> Result<String> {
    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = make_post_url(repo);

    let length_left = TWEET_LENGTH - (prefix.len() + stars.len() + url.len());
    let description = make_post_description(repo, length_left).await.unwrap_or_default();
    Ok(format!("{}{}{}{}", prefix, description, stars, url))
}

async fn make_toot(repo: &Repo) -> Result<String> {
    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = make_post_url(repo);

    let length_left = TOOT_LENGTH - (prefix.len() + stars.len() + MASTODON_FIXED_URL_LENGTH);
    let description = make_post_description(repo, length_left).await.unwrap_or_default();
    Ok(format!("{}{}{}{}", prefix, description, stars, url))
}

async fn make_weibo(repo: &Repo) -> Result<String> {
    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = format!("\n\n项目地址：github.com/{}/{}", repo.author, repo.name);
    let length_left = WEIBO_LENGTH - (prefix.len() + stars.len() + url.len());
    let description = make_post_description(repo, length_left).await.unwrap_or_default();
    Ok(format!("{}{}{}\n\n{}", stars, prefix, description, url))
}

async fn make_zsxq(repo: &Repo) -> Result<String> {
    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = make_post_url(repo);
    let length_left = ZSXQ_LENGTH - (prefix.len() + stars.len() + url.len());
    let description = make_post_description(repo, length_left).await.unwrap_or_default();
    info!("{}/{} description: {}", repo.author, repo.name, description);
    Ok(format!("{}{}{}\n\n{}", prefix, description, stars, url))
}

async fn make_telegram_bot(repo: &Repo) -> Result<String> {
    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = make_post_url(repo);
    let length_left = TELEGRAM_BOT_LENGTH - (prefix.len() + stars.len() + url.len());
    let description = make_post_description(repo, length_left).await.unwrap_or_default();
    Ok(format!("{}{}{}{}", prefix, description, stars, url))
}

async fn is_repo_posted(conn: &mut redis::aio::Connection, repo: &Repo) -> Result<bool> {
    Ok(conn
        .exists(format!("{}/{}", repo.author, repo.name))
        .await?)
}

async fn tweet(config: &TwitterConfig, content: String) -> Result<()> {
    let token = Oauth1aToken::new(
        &config.consumer_key,
        &config.consumer_secret,
        &config.token,
        &config.secret,
    );
    TwitterApi::new(token)
        .post_tweet()
        .text(content)
        .send()
        .await?;
    Ok(())
}

#[derive(Serialize, Debug)]
struct PostStatusesBody<'a> {
    status: &'a str,
    visibility: &'a str,
}

async fn toot(config: &MastodonConfig, content: &str) -> Result<()> {
    static CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);
    let url = config.instance_url.join("./api/v1/statuses")?;
    CLIENT
        .post(url)
        .bearer_auth(&config.access_token)
        .form(&PostStatusesBody {
            status: content,
            visibility: "unlisted",
        })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

async fn post_zsxq(config: &ZsxqConfig, content: &str) -> Result<()> {
    let url = format!("https://api.zsxq.com/v2/groups/{}/topics", config.group_id);
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
            HeaderValue::from_str(&config.cookie)?,
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
                Err(anyhow!("post zsxq failed: {}", resp_str))
            }
        }
    }
}

async fn post_weibo(config: &WeiboConfig, content: &str) -> Result<()> {
    let client = reqwest::Client::builder().build()?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("authority", "weibo.com".parse()?);
    headers.insert("accept", "application/json, text/plain, */*".parse()?);
    headers.insert(
        "accept-language",
        "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7".parse()?,
    );
    headers.insert("cache-control", "no-cache".parse()?);
    headers.insert("content-type", "application/x-www-form-urlencoded".parse()?);
    headers.insert("cookie", config.cookie.parse()?);
    headers.insert("origin", "https://weibo.com".parse()?);
    headers.insert("pragma", "no-cache".parse()?);
    headers.insert("referer", "https://weibo.com/".parse()?);
    headers.insert(
        "sec-ch-ua",
        "\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Google Chrome\";v=\"120\"".parse()?,
    );
    headers.insert("sec-ch-ua-mobile", "?0".parse()?);
    headers.insert("sec-ch-ua-platform", "\"macOS\"".parse()?);
    headers.insert("sec-fetch-dest", "empty".parse()?);
    headers.insert("sec-fetch-mode", "cors".parse()?);
    headers.insert("sec-fetch-site", "same-origin".parse()?);
    headers.insert("server-version", "v2024.01.26.2".parse()?);
    headers.insert("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".parse()?);
    headers.insert("x-requested-with", "XMLHttpRequest".parse()?);
    headers.insert("x-xsrf-token", config.xsrf_token.parse()?);

    let mut params = std::collections::HashMap::new();
    params.insert("content", content);
    params.insert("visible", "0");
    params.insert("share_id", "");
    params.insert("media", "");
    params.insert("vote", "");

    let request = client
        .request(
            reqwest::Method::POST,
            "https://weibo.com/ajax/statuses/update",
        )
        .headers(headers)
        .form(&params);

    let response = request.send().await?;
    let body = response.text().await?;

    let resp: Value = serde_json::from_str(body.as_str())?;
    match resp["ok"].as_i64() {
        None => Err(anyhow!("post weibo failed: {}", body)),
        Some(b) => {
            if b == 1 {
                Ok(())
            } else {
                Err(anyhow!("post weibo failed: {}", body))
            }
        }
    }
}

async fn post_telegram_bot(config: &TelegramBotConfig, content: &str) -> Result<()> {
    let bot = Bot::new(&config.token);

    bot.send_message(ChatId(config.chat_id), content)
        .send()
        .await?;
    Ok(())
}

async fn post_bluesky(config: &BlueskyConfig, repo: &Repo) -> Result<()> {
    let thumbnail = get_github_og_image(repo).await?;

    let prefix = make_post_prefix(repo);
    let stars = make_post_stars(repo);
    let url = make_post_url(repo);

    let length_left = BLUESKY_POST_LENGTH - (prefix.len() + stars.len() + url.len());

    let description = make_post_description(repo, length_left).await.unwrap_or_default();
    let text = format!("{}{}{}{}", prefix, description, stars, url);

    let client = AtpServiceClient::new(Arc::new(atrium_xrpc::client::reqwest::ReqwestClient::new(
        config.host.clone(),
    )));

    let session = client
        .com
        .atproto
        .server
        .create_session(atproto::server::create_session::Input {
            identifier: config.identifier.clone(),
            password: config.password.clone(),
        })
        .await?;
    let did = session.did.clone();

    let mut client = atrium_api::agent::AtpAgent::new(
        atrium_xrpc::client::reqwest::ReqwestClient::new(config.host.clone()),
    );
    client.set_session(session);

    let blob = client
        .api
        .com
        .atproto
        .repo
        .upload_blob(thumbnail.to_vec())
        .await?
        .blob;

    client
        .api
        .com
        .atproto
        .repo
        .create_record(atproto::repo::create_record::Input {
            collection: "app.bsky.feed.post".to_string(),
            record: atrium_api::records::Record::AppBskyFeedPost(Box::new(
                bsky::feed::post::Record {
                    created_at: OffsetDateTime::now_utc()
                        .format(&time::format_description::well_known::Rfc3339)?,
                    embed: Some(bsky::feed::post::RecordEmbedEnum::AppBskyEmbedExternalMain(
                        Box::new(bsky::embed::external::Main {
                            external: bsky::embed::external::External {
                                description: repo.description.clone(),
                                thumb: Some(blob),
                                title: format!("{} / {}", repo.author, repo.name),
                                uri: repo_uri(repo),
                            },
                        }),
                    )),
                    entities: None,
                    facets: None,
                    langs: None,
                    reply: None,
                    text,
                },
            )),
            repo: did,
            rkey: None,
            swap_commit: None,
            validate: None,
        })
        .await?;

    Ok(())
}

async fn mark_posted_repo(
    conn: &mut redis::aio::Connection,
    repo: &Repo,
    ttl: usize,
) -> Result<()> {
    conn.set_ex(format!("{}/{}", repo.author, repo.name), now_ts(), ttl)
        .await?;
    Ok(())
}

async fn main_loop(config: &Config, redis_conn: &mut redis::aio::Connection) -> Result<()> {
    let repos = fetch_repos().await.context("While fetching repo")?;

    for repo in repos {
        if config.denylist.contains(&repo)
            || is_repo_posted(redis_conn, &repo)
                .await
                .context("While checking repo posted")?
        {
            continue;
        }

        if let Some(config) = &config.twitter {
            let content = make_tweet(&repo).await.unwrap_or_default();
            if let Err(error) = tweet(config, content).await.context("While tweeting") {
                error!("{:#?}", error);
            }
        }

        if let Some(config) = &config.mastodon {
            let content = make_toot(&repo).await.unwrap_or_default();
            if let Err(error) = toot(config, &content).await.context("While tooting") {
                error!("{:#?}", error);
            }
        }

        if let Some(config) = &config.bluesky {
            if let Err(error) = post_bluesky(config, &repo)
                .await
                .context("While posting to Bluesky")
            {
                error!("{:#?}", error);
            }
        }

        if let Some(config) = &config.zsxq {
            let content = make_zsxq(&repo).await.unwrap_or_default();
            if let Err(error) = post_zsxq(config, &content)
                .await
                .context("While posting to zsxq")
            {
                error!("{:#?}", error)
            }
        }

        if let Some(config) = &config.weibo {
            let content = make_weibo(&repo).await.unwrap_or_default();
            if let Err(error) = post_weibo(config, &content)
                .await
                .context("While posting to weibo")
            {
                error!("{:#?}", error)
            }
        }

        if let Some(config) = &config.telegram_bot {
            let content = make_telegram_bot(&repo).await.unwrap_or_default();
            if let Err(error) = post_telegram_bot(config, &content)
                .await
                .context("While posting to telegram_bot")
            {
                error!("{:#?}", error)
            }
        }

        mark_posted_repo(redis_conn, &repo, config.interval.post_ttl)
            .await
            .context("While marking repo posted")?;

        info!("posted {} - {}", repo.author, repo.name);

        tokio::time::sleep(tokio::time::Duration::from_secs(
            config.interval.post_interval,
        ))
        .await;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::try_init().context("While initializing env_logger")?;

    let mut args = std::env::args();
    args.next();
    let config_file_path = args.next().unwrap_or_else(|| "./config.toml".to_string());
    let config = read_config(&config_file_path).context("While reading config file")?;

    let redis_client =
        redis::Client::open(config.redis.url.as_str()).context("While creating redis client")?;
    let mut redis_conn = redis_client
        .get_async_connection()
        .await
        .context("While connecting redis")?;

    loop {
        let res = main_loop(&config, &mut redis_conn).await;
        if let Err(e) = res {
            error!("{:#}", e);
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(
            config.interval.fetch_interval,
        ))
        .await;
    }
}
