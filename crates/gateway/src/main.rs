//! `brier-gateway`: the exchange process (core, journal, REST API, read-model projector).
//!
//! ```text
//! brier-gateway [serve]                 run the exchange
//! brier-gateway promote <user> <reason> give a user the admin role (audit-logged)
//! ```
//! Configuration comes from the environment only: `BRIER_DATABASE_URL` (required; never
//! logged), `BRIER_DATA_DIR` (default `data`), `BRIER_LISTEN` (default `127.0.0.1:8080`).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use brier_core::CoreConfig;
use brier_gateway::{AppState, router, run_projector};
use serde_json::json;
use tokio::net::TcpListener;
use tokio_postgres::{Client, NoTls};

fn info(msg: &str, detail: &serde_json::Value) {
    eprintln!(
        "{}",
        json!({ "level": "info", "msg": msg, "detail": detail })
    );
}

async fn connect(url: &str) -> Result<Client, String> {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .map_err(|e| format!("database: {}", brier_readmodel::describe_db_error(&e)))?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!(
                "{}",
                json!({ "level": "error", "msg": "database connection closed", "detail": e.to_string() })
            );
        }
    });
    Ok(client)
}

async fn shutdown_signal() {
    let interrupt = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        else {
            let _ = interrupt.await;
            return;
        };
        tokio::select! {
            _ = interrupt => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = interrupt.await;
}

async fn serve(url: &str) -> Result<(), String> {
    let data = PathBuf::from(std::env::var("BRIER_DATA_DIR").unwrap_or_else(|_| "data".to_owned()));
    let listen: SocketAddr = std::env::var("BRIER_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
        .parse()
        .map_err(|e| format!("BRIER_LISTEN: {e}"))?;
    let mut db = connect(url).await?;
    brier_readmodel::migrate(&mut db)
        .await
        .map_err(|e| e.to_string())?;
    let projector_db = connect(url).await?;

    let (core, recovery, core_thread) =
        brier_core::start(CoreConfig::new(&data)).map_err(|e| e.to_string())?;
    info(
        "recovered",
        &json!({ "next_seq": recovery.next_seq.get(), "replayed": recovery.replayed, "snapshot": recovery.snapshot.map(brier_types::Seq::get) }),
    );
    let state = Arc::new(AppState::new(core.clone(), db).map_err(|e| format!("{e:?}"))?);
    let projector = tokio::spawn(run_projector(
        projector_db,
        data,
        core.durable(),
        Arc::clone(&state.projected),
    ));
    drop(core);

    let listener = TcpListener::bind(listen)
        .await
        .map_err(|e| format!("bind {listen}: {e}"))?;
    info("listening", &json!({ "address": listen.to_string() }));
    axum::serve(
        listener,
        router(Arc::clone(&state)).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        // WebSocket connections are not HTTP requests the server waits for; close them.
        state.shutdown.send_replace(true);
    })
    .await
    .map_err(|e| e.to_string())?;

    // Every handle is gone with the router, so the core finishes its last batch and stops.
    let core_result = tokio::task::spawn_blocking(move || core_thread.join())
        .await
        .map_err(|e| e.to_string())?;
    core_result
        .map_err(|_| "the core thread panicked".to_owned())?
        .map_err(|e| e.to_string())?;
    match projector.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(format!("projector: {e}")),
        Err(e) => return Err(format!("projector task: {e}")),
    }
    info("stopped", &json!({}));
    Ok(())
}

fn db_err(e: &tokio_postgres::Error) -> String {
    format!("database: {}", brier_readmodel::describe_db_error(e))
}

async fn promote(url: &str, username: &str, reason: &str) -> Result<(), String> {
    if !(3..=500).contains(&reason.trim().chars().count()) {
        return Err("reason must be 3 to 500 characters".to_owned());
    }
    let mut db = connect(url).await?;
    brier_readmodel::migrate(&mut db)
        .await
        .map_err(|e| e.to_string())?;
    let tx = db.transaction().await.map_err(|e| db_err(&e))?;
    let row = tx
        .query_opt(
            "UPDATE users SET role = 'admin' WHERE username = $1 RETURNING id",
            &[&username],
        )
        .await
        .map_err(|e| db_err(&e))?
        .ok_or_else(|| format!("no user named {username}"))?;
    let id: i64 = row.get(0);
    tx.execute(
        "INSERT INTO audit_log (user_id, action, detail, reason) VALUES ($1, 'promote_admin', '{\"via\": \"cli\"}', $2)",
        &[&id, &reason.trim()],
    )
    .await
    .map_err(|e| db_err(&e))?;
    tx.commit().await.map_err(|e| db_err(&e))?;
    info("promoted", &json!({ "user_id": id }));
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Ok(url) = std::env::var("BRIER_DATABASE_URL") else {
        eprintln!("set BRIER_DATABASE_URL (for local development: make db-url)");
        return ExitCode::FAILURE;
    };
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["serve"] => serve(&url).await,
        ["promote", username, reason] => promote(&url, username, reason).await,
        _ => Err(
            "usage: brier-gateway [serve] | brier-gateway promote <username> <reason>".to_owned(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!(
                "{}",
                json!({ "level": "error", "msg": "brier-gateway failed", "detail": e })
            );
            ExitCode::FAILURE
        }
    }
}
