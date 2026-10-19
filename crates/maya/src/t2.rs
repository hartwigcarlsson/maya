//! T2-jobbet i `maya serve` (#150): en tunn adapter runt `maya_core::t2::kör`. Jobbet körs vid
//! start och sedan strax efter midnatt i Stockholm varje natt, så att T2-utkastet skapas dagen
//! efter räkenskapsårets slut, också när servern var nere den natten.

use std::time::Duration;

use jiff::Timestamp;
use jiff::civil::Time;
use maya_core::{Databas, Klocka};

/// Klockslaget i Stockholm då jobbet körs.
const KLOCKSLAG: Time = Time::constant(0, 5, 0, 0);

/// Skapar T2-utkasten som Maya självt, vid start och varje natt, tills servern stannar.
pub async fn nattligt_jobb(db: Databas) {
    loop {
        kör(&db).await;
        let nästa = maya_domain::backup::nästa_körning(Timestamp::now(), KLOCKSLAG);
        vänta_till(nästa).await;
    }
}

async fn kör(db: &Databas) {
    match maya_core::t2::kör(db, &Klocka::system()).await {
        Ok(skapade) => {
            for (företag, verifikat) in skapade {
                tracing::info!("T2-utkast skapat i företag {företag}: verifikat {verifikat}");
            }
        }
        Err(e) => tracing::error!("T2-jobbet misslyckades: {e}"),
    }
}

/// Sover till `tid` i korta steg, så att en ändrad systemklocka inte flyttar körningen.
async fn vänta_till(tid: Timestamp) {
    loop {
        let kvar = tid.duration_since(Timestamp::now());
        if kvar.is_negative() || kvar.is_zero() {
            return;
        }
        let steg = Duration::try_from(kvar)
            .unwrap_or(Duration::ZERO)
            .min(Duration::from_secs(60));
        tokio::time::sleep(steg).await;
    }
}
