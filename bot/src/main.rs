use std::cmp;
use std::future::Future;
use std::marker::PhantomData;

use anyhow::Context as _;
use futures::future::FutureExt;
use serenity::model::channel::Message;
use serenity::model::gateway::{Activity, Ready};
use serenity::model::id::{ChannelId, MessageId};
use serenity::prelude::*;

use common::db;
use common::secret::Secret;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let secret = common::secret::load().context("Failed loading secret file")?;

    let conn = db::Conn::new(&secret).await.context("Failed initializing database")?;

    let handler = Handler {
        client_id: secret.discord.client_id,
        prefix1: format!("<@!{}>", secret.discord.client_id),
        prefix2: format!("<@{}>", secret.discord.client_id),
        invite_link: format!(
            "https://discord.com/oauth2/authorize?client_id={}&scope=bot",
            secret.discord.client_id
        ),
    };
    log::info!("Invite link: {}", &handler.invite_link);

    let mut client = Client::builder(&secret.discord.token)
        .type_map_insert::<Data<Secret>>(secret)
        .type_map_insert::<Data<db::Conn>>(conn)
        .event_handler(handler)
        .await?;

    client.start().await?;

    Ok(())
}

struct Data<T>(PhantomData<T>);
impl<T: Send + Sync + 'static> TypeMapKey for Data<T> {
    type Value = T;
}

struct Handler {
    client_id: u64,
    prefix1: String,
    prefix2: String,
    invite_link: String,
}

const MESSAGE_MAX_LENGTH: usize = 2000;

#[async_trait::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _: Ready) {
        async fn do_update(update: db::Update, ctx: Context) -> anyhow::Result<()> {
            let resp = reqwest::get(&update.url)
                .await
                .context("Failed to download file")?;
            let mut text: String = resp.text().await.context("The file is not valid UTF-8")?;

            let max_len = update.message_ids.len() * MESSAGE_MAX_LENGTH;
            if max_len < text.len() {
                let err = format!("\u{2026}\nSee <{}> for more", &update.url);
                text.truncate(max_len - err.len());
                text += &err;
            }

            let channel = ChannelId::from(update.channel_id);
            for (i, message) in update.message_ids.iter().enumerate() {
                let range =
                    (MESSAGE_MAX_LENGTH * i)..cmp::min(MESSAGE_MAX_LENGTH * (i + 1), text.len());
                let slice = text
                    .get(range)
                    .unwrap_or("*(message reserved for expansion)*");
                let message = MessageId::from(*message);
                let mut message = channel.message(&ctx, message).await?;
                message.edit(&ctx, |m| m.content(slice)).await?;
            }

            Ok(())
        }

        ctx.set_activity(Activity::playing("https://github.com/SOF3/blob-mirror"))
            .await;

        let mut conn = {
            let data = ctx.data.read().await;
            let secret = data.get::<Data<Secret>>().expect("Secret uninitialized");
            common::db::subscriber(secret).expect("Failed to initialize database connection")
        };

        tokio::spawn(async move {
            while let Some(update) = conn.recv().await {
                tokio::spawn(do_update(update, ctx.clone()).map(|result| {
                    if let Err(err) = result {
                        log::error!("Error dispatching update: {}", err);
                    }
                }));
            }
        });
    }

    async fn message(&self, ctx: Context, msg: Message) {
        trying(async move {
            let content = if let Some(content) = msg.content.strip_prefix(&self.prefix1) {
                content
            } else if let Some(content) = msg.content.strip_prefix(&self.prefix2) {
                content
            } else {
                return Ok(());
            };

            log::debug!("Received command: {}", content.trim());
            let reaction = msg.react(&ctx, '⏳').await;
            let mut args = content
                .split(char::is_whitespace)
                .filter(|str| !str.is_empty());

            let ret = match args.next() {
                Some("invite") => {
                    match msg
                        .reply(&ctx, format!("Invite link: {}", &self.invite_link))
                        .await
                    {
                        Ok(_) => Ok(()),
                        Err(err) => Err(anyhow::Error::from(err)),
                    }
                }
                Some("mirror") => {
                    let result = mirror_cmd(&ctx, &msg, args).await;
                    if let Err(err) = result {
                        log::warn!("Error handling command: {:?}", err);
                        msg.reply(&ctx, format!("{:?}", err))
                            .await
                            .map(|_| ())
                            .map_err(Into::into)
                    } else {
                        Ok(())
                    }
                }
                _ => Ok(()),
            };

            if let Ok(reaction) = reaction {
                reaction.delete(ctx).await?;
            }

            ret
        })
        .await;
    }
}

async fn mirror_cmd(
    ctx: &Context,
    msg: &Message,
    mut args: impl Iterator<Item = &str>,
) -> anyhow::Result<()> {
    let url = args
        .next()
        .context("Usage: `mirror <url> [message splits]`")?;
    let mut pages = match args.next() {
        Some(pages) => pages
            .parse::<usize>()
            .context("Usage: `mirror <url> [message splits]`")?,
        None => 1,
    };

    struct UrlInfo<T: AsRef<str>> {
        user: T,
        repo: T,
        path: T,
    }

    fn parse_url(url: &str) -> Option<UrlInfo<&str>> {
        if let Some(url) = url.strip_prefix("https://github.com/") {
            let mut split = url.splitn(4, '/');
            let user = split.next()?;
            let repo = split.next()?;
            let _ = split.next()?;
            let path = split.next()?;

            Some(UrlInfo { user, repo, path })
        } else if let Some(url) = url.strip_prefix("https://raw.githubusercontent.com/") {
            let mut split = url.splitn(3, '/');
            let user = split.next()?;
            let repo = split.next()?;
            let path = split.next()?;

            Some(UrlInfo { user, repo, path })
        } else {
            None
        }
    }

    let info = parse_url(url).context("The URL must be a file on GitHub repo.")?;

    let channel = msg
        .channel_id
        .to_channel(ctx)
        .await
        .context("blob-mirror is only usable in guild channels")?
        .guild()
        .context("blob-mirror is only usable in guild channels")?;

    let real_url = format!(
        "https://raw.githubusercontent.com/{}/{}/{}",
        info.user, info.repo, info.path
    );
    let text = reqwest::get(&real_url)
        .await
        .context("Failed to fetch file")?
        .text()
        .await
        .context("The file is not valid UTF-8")?;

    #[derive(serde::Deserialize)]
    struct GhRepo {
        id: u64,
    }

    let client = reqwest::Client::new();
    let gh_repo = client.get(&format!("https://api.github.com/repos/{}/{}", info.user, info.repo))
        .header("User-Agent", "blob-mirror/v0.1").send()
        .await
        .context("Failed to lookup repo")?
        .json::<GhRepo>()
        .await
        .context("GitHub API is not working correctly")?;
    let repo_id = gh_repo.id;

    pages = cmp::max(text.len() / MESSAGE_MAX_LENGTH + 1, pages);

    let mut message_ids = Vec::with_capacity(pages);
    for i in 0..pages {
        let range = (MESSAGE_MAX_LENGTH * i)..cmp::min(MESSAGE_MAX_LENGTH * (i + 1), text.len());
        let slice = text
            .get(range)
            .unwrap_or("*(message reserved for expansion)*");
        let message = channel.send_message(ctx, |m| m.content(slice)).await?;
        message_ids.push(*message.id.as_u64());
    }

    let channel_id = *msg.channel_id.as_u64();

    {
        let tymap = ctx.data.read().await;
        let conn = tymap.get::<Data<db::Conn>>().expect("Conn uninitialized");
        if let Err(err) = conn.add_update(repo_id, info.path, channel_id, &message_ids).await {
            log::error!("Error storing message group: {}", err);
            anyhow::bail!("Error storing message group");
        }

        match conn.is_seen(repo_id).await {
            Ok(true) => (),
            Ok(false) => {
                msg.reply(ctx, "⚠️  I have never heard from this repo. \
                    Please contact the author to install the blob-mirror GitHub App \
                    at https://github.com/apps/blob-mirror for this repo.")
                    .await?;
            },
            Err(err) => {
                log::error!("Error checking seen status: {:?}", err);
            }
        }
    }

    Ok(())
}

async fn trying(f: impl Future<Output = anyhow::Result<()>>) {
    if let Err(err) = f.await {
        log::error!("Error handling message: {}", err);
    }
}
