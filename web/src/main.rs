use std::net;
use std::sync::Arc;

use anyhow::Context;
use warp::Filter;
use warp_github_webhook::{webhook, Kind as EventType};

use common::db;

mod schema;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use warp::Filter;

    pretty_env_logger::init();

    let secret = common::secret::load().context("Failed loading secret file")?;

    let conn = db::Conn::new(&secret)
        .await
        .context("Failed initializing database")?;
    let conn = Arc::new(conn);

    let routes = warp::post().and(warp::path("webhook")).and(
        ping_event(secret.github.webhook_secret.clone())
            .or(installation_event(
                secret.github.webhook_secret.clone(),
                Arc::clone(&conn),
            ))
            .or(installation_repositories_event(
                secret.github.webhook_secret.clone(),
                Arc::clone(&conn),
            ))
            .or(push_event(
                secret.github.webhook_secret.clone(),
                Arc::clone(&conn),
            ))
            .or(repository_event(
                secret.github.webhook_secret.clone(),
                Arc::clone(&conn),
            )),
    );

    let addr = net::SocketAddr::from(([0, 0, 0, 0], 8000));
    warp::serve(routes).run(addr).await;

    Ok(())
}

fn ping_event(
    webhook_secret: String,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    webhook(EventType::PING, webhook_secret).map(|_: schema::PingEvent| "OK")
}

fn installation_event(
    webhook_secret: String,
    conn: Arc<db::Conn>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    webhook(EventType::INSTALLATION, webhook_secret).and_then({
        move |event: schema::InstallationEvent| {
            let conn = Arc::clone(&conn);
            async move {
                let seen = event.action.seen();

                let ids: Vec<u64> = event.repositories.iter().map(|repo| repo.id).collect();
                let output = match conn.seen_bool_multi(ids, seen).await {
                    Ok(_) => "OK",
                    Err(err) => {
                        log::error!("Error: {:?}", err);
                        "ERROR"
                    }
                };
                Ok::<&'static str, warp::Rejection>(output)
            }
        }
    })
}

fn installation_repositories_event(
    webhook_secret: String,
    conn: Arc<db::Conn>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    webhook(EventType::INSTALLATION_REPOSITORIES, webhook_secret).and_then({
        move |event: schema::InstallationRepositoriesEvent| {
            let conn = Arc::clone(&conn);
            async move {
                let seen = event.action.seen();

                let add_ids: Vec<u64> = event
                    .repositories_added
                    .iter()
                    .map(|repo| repo.id)
                    .collect();
                if let Err(err) = conn.seen_bool_multi(add_ids, seen).await {
                    log::error!("Error: {:?}", err);
                    return Ok("ERROR");
                };

                let remove_ids: Vec<u64> = event
                    .repositories_removed
                    .iter()
                    .map(|repo| repo.id)
                    .collect();
                if let Err(err) = conn.seen_bool_multi(remove_ids, seen).await {
                    log::error!("Error: {:?}", err);
                    return Ok("ERROR");
                };
                Ok::<&'static str, warp::Rejection>("OK")
            }
        }
    })
}

fn repository_event(
    webhook_secret: String,
    conn: Arc<db::Conn>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    webhook(EventType::REPOSITORY, webhook_secret).and_then({
        move |event: schema::RepoEvent| {
            let conn = Arc::clone(&conn);
            async move {
                let seen = match event.action.seen() {
                    Some(seen) => seen,
                    None => return Ok("OK"),
                };

                let output = match conn.seen_bool(event.repository.id, seen).await {
                    Ok(()) => "OK",
                    Err(err) => {
                        log::error!("Error: {:?}", err);
                        "ERROR"
                    }
                };
                Ok::<&'static str, warp::Rejection>(output)
            }
        }
    })
}

fn push_event(
    webhook_secret: String,
    conn: Arc<db::Conn>,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    webhook(EventType::PUSH, webhook_secret).and_then(move |event: schema::PushEvent| {
        let conn = Arc::clone(&conn);
        async move {
            let mut split = event.repository.full_name.split('/');
            let (user, repo) = match (split.next(), split.next()) {
                (Some(user), Some(repo)) => (user, repo),
                _ => return Ok("Deserialization error"),
            };
            let output = match conn.on_repo_update(event.repository.id, user, repo).await {
                Ok(()) => "OK",
                Err(err) => {
                    log::error!("Error: {:?}", err);
                    "ERROR"
                }
            };
            Ok::<&'static str, warp::Rejection>(output)
        }
    })
}
