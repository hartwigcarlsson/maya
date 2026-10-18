//! Binären `maya`: servern och CLI:t. Kommandona är på engelska och utskrifterna på svenska.

use std::net::{Ipv4Addr, SocketAddr};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use maya_core::Databas;
use maya_core::config::Config;
use maya_core::installation::{self, RelyingParty};

mod healthcheck;
mod user;

#[derive(Parser)]
#[command(name = "maya", version, about = "Maya: bokföring för små aktiebolag")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Startar servern.
    Serve,
    /// Frågar en körande server på /healthz, för Dockers hälsokontroll.
    Healthcheck,
    /// Hanterar användare.
    User {
        #[command(subcommand)]
        command: user::UserCommand,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Err(e) = dotenvy::dotenv()
        && !e.not_found()
    {
        eprintln!("Maya kan inte läsa .env: {e}");
        return ExitCode::FAILURE;
    }
    match cli.command {
        Command::Serve => serve(),
        Command::Healthcheck => healthcheck::run(),
        Command::User { command } => user::run(command),
    }
}

fn serve() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stdout)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = match Config::from_env(std::env::vars()) {
        Ok(config) => config,
        Err(e) => {
            eprint!("{e}");
            return ExitCode::FAILURE;
        }
    };
    println!("{config}");

    let runtime = tokio::runtime::Runtime::new().expect("tokio-runtime");
    match runtime.block_on(run(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Maya kan inte starta: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&config.data_dir)
        .map_err(|e| format!("kan inte skapa {}: {e}", config.data_dir.display()))?;
    let db = Databas::öppna(&config.database_path()).await?;
    installation::kontrollera_rp(&db, &RelyingParty::från_publik_url(&config.public_url)).await?;
    let config = Arc::new(config);
    let app = maya_web::router(config.clone(), db);

    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|e| format!("kan inte lyssna på {}: {e}", config.listen))?;
    tracing::info!(
        "Maya {} lyssnar på {}, publik adress {}",
        env!("CARGO_PKG_VERSION"),
        config.listen,
        config.public_url
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    tracing::info!("Maya har stannat");
    Ok(())
}

/// Väntar på SIGTERM (från `docker stop`) eller Ctrl-C.
async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signalhanterare");
        tokio::select! {
            _ = ctrl_c => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}

/// Adressen som `MAYA_LISTEN` anger, för kommandon som inte behöver hela konfigurationen.
fn listen_addr() -> SocketAddr {
    std::env::var("MAYA_LISTEN")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8080)))
}
