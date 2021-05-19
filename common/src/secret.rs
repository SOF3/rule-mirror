use std::net::SocketAddr;

use anyhow::Context;

#[derive(serde::Deserialize)]
pub struct Secret {
    pub discord: Discord,
    pub github: Github,
    pub web: Web,
    pub redis: Redis,
}

#[derive(serde::Deserialize)]
pub struct Discord {
    pub client_id: u64,
    pub client_secret: String,
    pub token: String,
}

#[derive(serde::Deserialize)]
pub struct Github {
    pub client_id: String,
    pub client_secret: String,
    pub slug: String,
    pub app_id: u64,
    pub webhook_secret: String,
}

#[derive(serde::Deserialize)]
pub struct Web {
    pub port: u16,
}

#[derive(Clone, serde::Deserialize)]
pub struct Redis {
    addr: String,
}

impl Redis {
    pub async fn addr(&self) -> anyhow::Result<SocketAddr> {
        let mut addrs = tokio::net::lookup_host(&self.addr).await?;
        addrs.next().context("Failed to lookup redis address")
    }
}

pub fn load() -> anyhow::Result<Secret> {
    let mut config = config::Config::new();
    config.merge(config::Environment::with_prefix("BLOB_MIRROR"))?;
    config.merge(config::File::with_name("secret"))?;
    Ok(config.try_into()?)
}
