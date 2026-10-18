//! Binären `maya`: servern och CLI:t. Kommandona är på engelska och utskrifterna på svenska.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use maya_core::config::Config;
use maya_core::installation::{self, RelyingParty};
use maya_core::kontoplan;
use maya_core::{Databas, Klocka};

mod backup;
mod cli;
mod healthcheck;
mod restore;
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
    /// Hanterar installationens domän.
    Domain {
        #[command(subcommand)]
        command: user::DomainCommand,
    },
    /// Hanterar backuperna.
    Backup {
        #[command(subcommand)]
        command: backup::BackupCommand,
    },
    /// Återställer hela installationen ur en backup i backupmålet. Stoppa servern först.
    Restore {
        /// Backupens namn, t.ex. maya-20261018T010000Z. Utan namn används den senaste.
        name: Option<String>,
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
        Command::User { command } => user::run_user(command),
        Command::Domain { command } => user::run_domain(command),
        Command::Backup { command } => backup::run(command),
        Command::Restore { name } => restore::run(name),
    }
}

fn serve() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stdout)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = match cli::config() {
        Ok(config) => config,
        Err(kod) => return kod,
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
    // Domänen prövas före öppningen, som migrerar och loggar programversionen: en start med
    // fel MAYA_PUBLIC_URL ska inte skriva något.
    let rp = RelyingParty::från_publik_url(&config.public_url);
    installation::kontrollera_rp_i_fil(&config.database_path(), &rp).await?;
    let db = Databas::öppna_med_backup(
        &config.database_path(),
        &maya_core::backup::Backupmål::från(&config.backup),
    )
    .await?;
    installation::kontrollera_rp(&db, &rp).await?;
    let tillagda = kontoplan::lägg_till_inbyggd_bas(&db, &Klocka::system()).await?;
    if tillagda > 0 {
        tracing::info!("{tillagda} nya BAS-konton tillagda som inaktiva");
    }
    backup::varna_vid_start(&config);
    let config = Arc::new(config);
    let jobb = {
        let (config, db) = (config.clone(), db.clone());
        tokio::spawn(async move { backup::nattligt_jobb(&config, db).await })
    };
    let larm = {
        let (config, db) = (config.clone(), db.clone());
        tokio::spawn(async move { backup::larmjobb(&config, db).await })
    };
    let app = maya_web::router(config.clone(), db);

    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|e| format!("kan inte lyssna på {}: {e}", config.listen))?;
    // Den verkliga adressen: med port 0 väljer operativsystemet porten.
    let adress = listener.local_addr()?;
    tracing::info!(
        "Maya {} lyssnar på {adress}, publik adress {}",
        env!("CARGO_PKG_VERSION"),
        config.public_url
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    jobb.abort();
    larm.abort();
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
