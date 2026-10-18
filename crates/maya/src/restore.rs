//! `maya restore`: återställer hela installationen ur en backup i backupmålet. En tunn adapter
//! runt `maya_core::aterstallning`. Den nuvarande databasen öppnas aldrig, så återställningen
//! fungerar också när den är trasig eller har ett nyare schema än binären.

use std::io::BufRead;
use std::process::ExitCode;

use maya_core::Klocka;
use maya_core::aktor::Aktör;
use maya_core::aterstallning;
use maya_core::backup::Backupmål;
use maya_core::config::Config;
use maya_core::installation::RelyingParty;

pub fn run(namn: Option<String>) -> ExitCode {
    let config = match crate::cli::config() {
        Ok(config) => config,
        Err(kod) => return kod,
    };
    let runtime = tokio::runtime::Runtime::new().expect("tokio-runtime");
    match runtime.block_on(restore(&config, namn.as_deref())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

use maya_domain::tid::minut as stockholm;

async fn restore(config: &Config, namn: Option<&str>) -> crate::cli::Resultat {
    let mål = Backupmål::från(&config.backup);
    let vald = aterstallning::välj(&mål, namn)
        .await
        .map_err(|e| format!("Maya kan inte återställa från {}: {e}", mål.beskrivning()))?;
    let tid = stockholm(vald.backup.tid);
    let databasfil = config.database_path();
    println!(
        "Backupen {} från {tid} i {} (Maya {}).",
        vald.backup.namn,
        mål.beskrivning(),
        vald.manifest.maya
    );
    println!(
        "Hela installationen i {} ersätts med backupen. Det som har registrerats efter {tid} \
         finns sedan inte i Maya och måste registreras om; den nuvarande databasen sparas \
         bredvid. Stoppa servern innan du fortsätter.",
        databasfil.display()
    );
    println!("Skriv ja för att återställa:");
    let mut svar = String::new();
    std::io::stdin().lock().read_line(&mut svar)?;
    if svar.trim() != "ja" {
        return Err("Återställningen är avbruten; inget har ändrats.".into());
    }

    std::fs::create_dir_all(&config.data_dir)
        .map_err(|e| format!("kan inte skapa {}: {e}", config.data_dir.display()))?;
    let återställd = aterstallning::återställ(
        &databasfil,
        &mål,
        &vald.backup.namn,
        &RelyingParty::från_publik_url(&config.public_url),
        &Klocka::system(),
        &Aktör::cli(),
        &config.arbetskatalog(),
    )
    .await
    .map_err(|e| format!("Maya kunde inte återställa installationen: {e}"))?;
    println!(
        "Installationen är återställd från backupen {} ({tid}). Kontrollsummorna stämmer med \
         backupen, och återställningen är loggad i behandlingshistoriken för {} företag. \
         Användarna ser en varning under Att göra tills de kvitterar den.",
        återställd.backup.namn, återställd.företag
    );
    println!(
        "Den tidigare databasen ligger kvar i {}. Starta servern igen.",
        återställd.undanflyttad.display()
    );
    Ok(())
}
