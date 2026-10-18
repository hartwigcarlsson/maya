//! Resultat- och balansräkningen ur bokföringen. Saldona kommer från [`crate::saldo`] (bara
//! bokförda verifikat, aldrig utkast) och ställs upp enligt INK2-kopplingstabellen i
//! `maya_domain::rapport`.

use jiff::civil::Date;
use maya_domain::rapport::{Rapport, Årsdata};

use crate::aktor::{Aktör, Handling};
use crate::atkomst;
use crate::foretag::FöretagsId;
use crate::saldo::{self, SaldoFel, Saldobalans, Status};
use crate::{Databas, kontoplan};

pub use maya_domain::rapport::Urval;

/// Rapporten i urvalet för räkenskapsåret som börjar `år`, med föregående år som jämförelse.
pub async fn rapport(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    år: Date,
    urval: &Urval,
) -> Result<Rapport, SaldoFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let balanser = saldo::saldobalanser(&mut tx, företag).await?;
    let mut rörelser = saldo::månadsrörelser(&mut tx, företag).await?;
    drop(tx);
    let i = balanser
        .iter()
        .position(|sb| sb.år.start() == år)
        .ok_or(SaldoFel::OkäntRäkenskapsår)?;
    let mut årsdata = |sb: &Saldobalans| Årsdata {
        år: sb.år,
        ingående: sb
            .konton
            .iter()
            .filter(|k| k.ingående.öre() != 0)
            .map(|k| (k.konto, k.ingående))
            .collect(),
        preliminär: sb.status == Status::Preliminär,
        rörelser: rörelser.remove(&sb.år.start()).unwrap_or_default(),
    };
    let detta = årsdata(&balanser[i]);
    let föregående = i.checked_sub(1).map(|f| årsdata(&balanser[f]));
    let konton = kontoplan::konton(db, aktör, företag).await?;
    let namn: Vec<_> = konton.into_iter().map(|k| (k.nummer, k.namn)).collect();
    Ok(Rapport::ställ_upp(
        urval,
        &detta,
        föregående.as_ref(),
        &namn,
    ))
}
