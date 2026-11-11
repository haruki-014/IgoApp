use std::process::ExitCode;

use server::config::Config;
use tracing_subscriber::EnvFilter;

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

async fn run() -> Result<(), String> {
    let config = Config::from_env()?;
    let pool = server::connect_db(&config.database_url)
        .await
        .map_err(|e| format!("DB に接続できません: {e}"))?;
    server::migrate(&pool)
        .await
        .map_err(|e| format!("マイグレーションに失敗しました: {e}"))?;

    let bind_addr = config.bind_addr;
    let engines: Vec<String> = config
        .engines
        .iter()
        .map(|e| format!("{}@{}:{}", e.name, e.host, e.port))
        .collect();
    tracing::info!(?engines, ai_max_concurrent = config.ai_max_concurrent, "設定を読み込みました");

    let manager = server::build(config, pool).await;
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("{bind_addr} で待ち受けできません: {e}"))?;
    tracing::info!(%bind_addr, "サーバーを起動しました");
    axum::serve(listener, server::router(manager))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("サーバーが異常終了しました: {e}"))
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            tracing::error!("{message}");
            ExitCode::FAILURE
        }
    }
}
