//! Hjälpmedel för tester i andra crates (feature `test-support`).

use maya_domain::foretag::NyttFöretag;
use maya_domain::verifikat::{NyRad, NyttVerifikat};

use crate::aktor::{Aktör, AnvändarId};
use crate::foretag::{self, FöretagsId};
use crate::{Databas, Klocka, verifikat};

/// En ny användare i webben, utan passkey och utan åtkomst till något företag. Inbjudningar
/// kommer i ett senare ticket; tills dess är det här vägen till en andra användare i tester.
pub async fn användare(db: &Databas, epost: &str) -> Aktör {
    let mut handle = [0u8; 16];
    getrandom::fill(&mut handle).expect("slumptal från operativsystemet");
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO anvandare (handle, epost, visningsnamn, skapad)
         VALUES (?, ?, ?, '2026-01-01T00:00:00Z') RETURNING id",
    )
    .bind(&handle[..])
    .bind(epost)
    .bind(epost)
    .fetch_one(db.pool())
    .await
    .expect("testanvändare");
    Aktör::webb(AnvändarId(id))
}

/// Ett företag med kalenderåret 2026 som första räkenskapsår, upplagt av en ny användare
/// `anna@example.se`, som returneras.
pub async fn företag(db: &Databas) -> (Aktör, FöretagsId) {
    let anna = användare(db, "anna@example.se").await;
    let id = foretag::lägg_upp(
        db,
        &Klocka::system(),
        &anna,
        &NyttFöretag {
            orgnr: "556036-0793".into(),
            namn: "Exempel AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "kvartal".into(),
        },
    )
    .await
    .expect("testföretag");
    (anna, id)
}

/// Bokför ett verifikat daterat `datum` med raderna `(konto, debet i öre, kredit i öre)`
/// genom [`verifikat::bokför`]. Panikar om en spärr slår till.
pub async fn bokför(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    datum: &str,
    rader: &[(u16, i64, i64)],
) -> verifikat::Bokfört {
    let belopp = |öre: i64| {
        if öre == 0 {
            String::new()
        } else {
            format!("{},{:02}", öre / 100, öre % 100)
        }
    };
    let formulär = NyttVerifikat {
        datum: datum.into(),
        beskrivning: format!("Testverifikat {datum}"),
        rader: rader
            .iter()
            .map(|&(konto, debet, kredit)| NyRad {
                konto: konto.to_string(),
                debet: belopp(debet),
                kredit: belopp(kredit),
                text: String::new(),
            })
            .collect(),
    };
    verifikat::bokför(db, &Klocka::system(), aktör, företag, &formulär)
        .await
        .expect("testverifikatet bokförs")
}
