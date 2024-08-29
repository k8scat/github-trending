use anyhow::{Context, Result};
use log::{error, info};
use platform::types::Platform;

mod config;
mod platform;
mod repo;
mod openai;

async fn main_loop(config: &config::Config, redis_conn: &mut redis::aio::Connection) -> Result<()> {
    let repos = repo::fetch_repos().await.context("While fetching repo")?;
    info!("fetched {} repos", repos.len());
    
    for repo in repos {
        if config.denylist.contains(&repo)
            || repo::is_repo_posted(redis_conn, &repo)
            .await
            .context("While checking repo posted")?
        {
            continue;
        }

        if let Some(zsxq) = &config.zsxq {
            let result = zsxq.content_by_repo(&repo).await.context("While getting zsxq content");
            match result {
                Ok(content) => {
                    zsxq.post(&content).await.context("While posting to zsxq")?;
                }
                Err(e) => {
                    error!("{:#}", e);
                }
            }
            // zsxq.post(&content).await.context("While posting to zsxq")?;
        }

        repo::mark_posted_repo(redis_conn, &repo, config.interval.post_ttl)
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
    let config = config::read_file(&config_file_path).context("While reading config file")?;

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
