use std::fmt;

use anyhow::Context;
use futures::future;
use futures::stream::{FuturesUnordered, StreamExt};
use redis_async::{
    client::{self, pubsub::PubsubStream},
    resp::RespValue,
    resp_array,
};
use tokio::sync::mpsc;

use crate::secret::{self, Secret};

pub fn subscriber<T>(secret: &Secret, topic: &'static str) -> anyhow::Result<mpsc::Receiver<T>>
where
    T: fmt::Debug + Send + Sync + serde::de::DeserializeOwned + 'static,
{
    async fn read<'t, T>(tx: &mpsc::Sender<T>, pubsub: &mut PubsubStream) -> anyhow::Result<()>
    where
        T: fmt::Debug + Send + Sync + serde::de::DeserializeOwned + 'static,
    {
        let msg = pubsub.next().await.context("Connection broken")??;
        let payload: T = match msg {
            RespValue::SimpleString(bs) => serde_json::from_str(&bs)?,
            _ => anyhow::bail!("Incorrect pubsub data type"),
        };
        tx.send(payload)
            .await
            .context("Failed handling subscribed message")?;
        Ok(())
    }

    async fn main<T>(
        tx: mpsc::Sender<T>,
        secret: secret::Redis,
        topic: &'static str,
    ) -> anyhow::Result<()>
    where
        T: fmt::Debug + Send + Sync + serde::de::DeserializeOwned + 'static,
    {
        let conn = client::pubsub_connect(secret.addr().await?).await?;
        let mut sub = conn.subscribe(topic).await?;

        loop {
            if let Err(err) = read(&tx, &mut sub).await {
                log::error!("Error handling pubsub: {}", err);
            }
        }
    }

    let (tx, rx) = mpsc::channel(16);

    tokio::spawn(main(tx, secret.redis.clone(), topic));

    Ok(rx)
}

/// Schema of the `on_seen` pubsub topic
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct OnSeen {
    pub deletions: Vec<u64>,
    pub dereacts: Vec<u64>,
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

    pub async fn seen_bool_multi(
        &self,
        repo_ids: impl IntoIterator<Item = u64>,
        seen: bool,
    ) -> anyhow::Result<()> {
        let mut futs = repo_ids
            .into_iter()
            .map(|id| self.seen_bool(id, seen))
            .collect::<FuturesUnordered<_>>();
        while let Some(ret) = futs.next().await {
            ret?;
        }
        Ok(())
    }

    pub async fn seen_bool(&self, repo_id: u64, seen: bool) -> anyhow::Result<()> {
        if seen {
            self.seen(repo_id).await
        } else {
            self.mark_unseen(repo_id).await.map(|_| ())
        }
    }

    async fn seen(&self, repo_id: u64) -> anyhow::Result<()> {
        self.mark_seen(repo_id)
            .await
            .context("Error marking seen")?;

        let deletion_strings: Vec<String> = self
            .conn
            .send(resp_array![
                "LRANGE",
                format!("delete-on-seen:{}", repo_id),
                "0",
                "-1"
            ])
            .await
            .context("Failed fetching deletion list")?;
        let dereact_strings: Vec<String> = self
            .conn
            .send(resp_array![
                "LRANGE",
                format!("dereact-on-seen:{}", repo_id),
                "0",
                "-1"
            ])
            .await
            .context("Failed fetching dereact list")?;

        let mut deletions = Vec::with_capacity(deletion_strings.len());
        for deletion in deletion_strings {
            deletions.push(
                deletion
                    .parse::<u64>()
                    .context("Deletion ID is not integer")?,
            );
        }
        let mut dereacts = Vec::with_capacity(dereact_strings.len());
        for dereact in dereact_strings {
            dereacts.push(
                dereact
                    .parse::<u64>()
                    .context("Dereact ID is not integer")?,
            );
        }

        let on_seen = OnSeen {
            deletions,
            dereacts,
        };
        let json = serde_json::to_string(&on_seen)?;
        self.conn
            .send(resp_array!["PUBLISH", "on_seen", json])
            .await
            .context("Failed to publish on_seen")?;

        Ok(())
    }

    async fn mark_seen(&self, repo_id: u64) -> anyhow::Result<bool> {
        let changed: bool = self
            .conn
            .send(resp_array!["SADD", "seen", repo_id.to_string()])
            .await
            .context("Error marking repo as seen")?;
        Ok(changed)
    }

    async fn mark_unseen(&self, repo_id: u64) -> anyhow::Result<bool> {
        let changed: bool = self
            .conn
            .send(resp_array!["SADD", "seen", repo_id.to_string()])
            .await
            .context("Error marking repo as seen")?;
        Ok(changed)
    }

    pub async fn is_seen(&self, repo_id: u64) -> anyhow::Result<bool> {
        let found: Vec<bool> = self
            .conn
            .send(resp_array!["SMISMEMBER", "seen", repo_id.to_string()])
            .await
            .context("Error checking repo seen status")?;
        Ok(*found
            .get(0)
            .expect("SMISMEMBER ret count = param count - 1"))
    }

    pub async fn on_repo_update(&self, repo_id: u64, user: &str, repo: &str) -> anyhow::Result<()> {
        for update in self.repo_updates(repo_id, user, repo).await? {
            let json = serde_json::to_string(&update)?;
            self.conn
                .send(resp_array!["PUBLISH", "updates", json])
                .await?;
        }
        Ok(())
    }

    async fn repo_updates(
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
            let _: String = self
                .conn
                .send(resp_array![
                    "SET",
                    format!("mirror-group-rev:{}", message_id),
                    id
                ])
                .await?;
            Ok(())
        });
        let rev_future = future::try_join_all(rev_futures);
        let _: (String, String, usize, _) =
            future::try_join4(path_future, channel_future, messages_future, rev_future).await?;

        Ok(())
    }

    pub async fn delete_on_seen(
        &self,
        repo_id: u64,
        channel_id: u64,
        message_id: u64,
    ) -> anyhow::Result<()> {
        let _: bool = self
            .conn
            .send(resp_array![
                "RPUSH",
                format!("delete-on-seen:{}", repo_id),
                channel_id.to_string(),
                message_id.to_string(),
            ])
            .await?;
        Ok(())
    }

    pub async fn dereact_on_seen(
        &self,
        repo_id: u64,
        channel_id: u64,
        message_id: u64,
    ) -> anyhow::Result<()> {
        let _: usize = self
            .conn
            .send(resp_array![
                "RPUSH",
                format!("dereact-on-seen:{}", repo_id),
                channel_id.to_string(),
                message_id.to_string(),
            ])
            .await?;
        Ok(())
    }
}
