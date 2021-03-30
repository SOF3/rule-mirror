use anyhow::Context;
use futures::future;
use futures::stream::StreamExt;
use redis_async::{
    client::{self, pubsub::PubsubStream},
    resp::RespValue,
    resp_array,
};
use tokio::sync::mpsc;

use crate::secret::{self, Secret};

pub fn subscriber(secret: &Secret) -> anyhow::Result<mpsc::Receiver<Update>> {
    async fn read<'t>(tx: &mpsc::Sender<Update>, pubsub: &mut PubsubStream) -> anyhow::Result<()> {
        let msg = pubsub.next().await.context("Connection broken")??;
        let payload: Update = match msg {
            RespValue::BulkString(bs) => serde_json::from_slice(&bs)?,
            _ => anyhow::bail!("Incorrect channel data"),
        };
        tx.send(payload)
            .await
            .context("Failed handling subscribed message")?;
        Ok(())
    }

    async fn main(tx: mpsc::Sender<Update>, secret: secret::Redis) -> anyhow::Result<()> {
        let conn = client::pubsub_connect(secret.addr().await?).await?;
        let mut sub = conn.subscribe("updates").await?;

        loop {
            if let Err(err) = read(&tx, &mut sub).await {
                log::error!("Error handling pubsub: {}", err);
            }
        }
    }

    let (tx, rx) = mpsc::channel(16);

    tokio::spawn(main(tx, secret.redis.clone()));

    Ok(rx)
}

/// Schema of the `updates` pubsub topic
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Update {
    pub channel_id: u64,
    pub message_ids: Vec<u64>,
    pub url: String,
}

pub struct Conn {
    conn: client::PairedConnection,
}

impl Conn {
    pub async fn new(secret: &Secret) -> anyhow::Result<Self> {
        Ok(Self {
            conn: client::paired_connect(secret.redis.addr().await?)
                .await
                .context("Failed to connect to redis")?,
        })
    }

    pub async fn mark_seen(&self, repo_id: u64) -> anyhow::Result<bool> {
        let changed: bool = self
            .conn
            .send(resp_array!["SADD", "seen", repo_id.to_string()])
            .await
            .context("Error marking repo as seen")?;
        Ok(changed)
    }

    pub async fn mark_unseen(&self, repo_id: u64) -> anyhow::Result<bool> {
        let changed: bool = self
            .conn
            .send(resp_array!["SADD", "seen", repo_id.to_string()])
            .await
            .context("Error marking repo as seen")?;
        Ok(changed)
    }

    pub async fn is_seen(&self, repo_id: u64) -> anyhow::Result<bool> {
        let found: Vec<bool> = self.conn.send(resp_array!["SMISMEMBER", "seen", repo_id.to_string()]).await
            .context("Error checking repo seen status")?;
        Ok(*found.get(0).expect("SMISMEMBER ret count = param count - 1"))
    }

    pub async fn repo_updates(
        &self,
        repo_id: u64,
        user: &str,
        repo: &str,
    ) -> anyhow::Result<Vec<Update>> {
        #[allow(clippy::unnecessary_wraps)]
        fn ok<T>(t: T) -> anyhow::Result<T> {
            Ok(t)
        }

        let groups: Vec<String> = self
            .conn
            .send(resp_array!["SMEMBERS", format!("repo:{}", repo_id)])
            .await
            .context("Could not fetch repo mirror groups")?;

        let updates = groups.iter().map(|id| async move {
            let path = async move {
                let path_key = format!("mirror-group:{}:path", id);
                let value: String = self
                    .conn
                    .send(resp_array!["GET", path_key])
                    .await
                    .context("Could not fetch mirrored file path")?;
                ok(value)
            };
            let channel_id = async move {
                let channel_key = format!("mirror-group:{}:channel", id);
                let value: String = self
                    .conn
                    .send(resp_array!["GET", channel_key])
                    .await
                    .context("Could not fetch mirror channel ID")?;
                let value = value
                    .parse::<u64>()
                    .context("Channel ID is not an integer")?;
                ok(value)
            };
            let message_ids = async move {
                let messages_key = format!("mirror-group:{}:messages", id);
                let value: Vec<String> = self
                    .conn
                    .send(resp_array!["LANGE", messages_key])
                    .await
                    .context("Could not fetch mirror message list")?;
                let value: Vec<u64> = value
                    .into_iter()
                    .map(|s| s.parse().context("Message ID has incorrect format"))
                    .collect::<anyhow::Result<_>>()?;
                ok(value)
            };

            let (path, channel_id, message_ids) =
                future::try_join3(path, channel_id, message_ids).await?;

            ok(Update {
                channel_id,
                message_ids,
                url: format!(
                    "https://raw.githubusercontent.com/{}/{}/{}",
                    user, repo, path
                ),
            })
        });
        let updates = future::try_join_all(updates).await?;

        Ok(updates)
    }

    pub async fn add_update(
        &self,
        repo_id: u64,
        path: &str,
        channel: u64,
        message_ids: &[u64],
    ) -> anyhow::Result<()> {
        use rand::Rng;

        let id: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
        let id = id.as_str();
        let changed: bool = self
            .conn
            .send(resp_array!["SADD", format!("repo:{}", repo_id), id])
            .await
            .context("Could not add mirror group to repo")?;
        assert!(changed, "Duplicate ID???");

        let path_future = self.conn.send(resp_array![
            "SET",
            format!("mirror-group:{}:path", id),
            path
        ]);
        let channel_future = self.conn.send(resp_array![
            "SET",
            format!("mirror-group:{}:channel", id),
            channel.to_string()
        ]);
        let messages_future = self.conn.send(
            resp_array!["RPUSH", format!("mirror-group:{}:messages", id)]
                .append(message_ids.iter().map(|id| id.to_string())),
        );
        let rev_futures = message_ids.iter().map(|&message_id| async move {
            let _: String = self.conn.send(resp_array!["SET", format!("mirror-group-rev:{}", message_id), id]).await?;
            Ok(())
        });
        let rev_future = future::try_join_all(rev_futures);
        let _: (String, String, usize, _) =
            future::try_join4(path_future, channel_future, messages_future, rev_future).await?;

        Ok(())
    }
}
