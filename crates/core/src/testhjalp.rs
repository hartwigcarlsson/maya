//! Hjälpmedel för tester i andra crates (feature `test-support`).

use maya_domain::foretag::NyttFöretag;
use maya_domain::verifikat::{NyRad, NyttVerifikat};

use crate::aktor::{Aktör, AnvändarId};
use crate::foretag::{self, FöretagsId};
use crate::inloggning::{self, Inloggad, Token, hash};
use crate::{Databas, Klocka, sie4, verifikat};

/// SIE4-exporten med en skrivare som `förvanskar` filen innan den skrivs, för att visa att
/// återläsningen stoppar den.
pub async fn exportera_sie4_förvanskad(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    år: jiff::civil::Date,
    förvanska: impl FnOnce(&mut maya_sie::Sie4),
) -> Result<sie4::Sie4Export, sie4::Sie4Fel> {
    sie4::exportera_med(db, klocka, aktör, företag, år, förvanska).await
}

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

/// `aktör` inloggad i en ny session där passkeyn användes vid `klocka`s tid, som om hen just
/// hade loggat in.
pub async fn inloggad(db: &Databas, klocka: &Klocka, aktör: &Aktör) -> Inloggad {
    let token = session(db, klocka, aktör).await;
    inloggning::session(db, klocka, &token)
        .await
        .expect("sessionen läses")
        .expect("sessionen gäller")
}

/// Som [`inloggad`], men ger sessionstoken till cookien.
pub async fn session(db: &Databas, klocka: &Klocka, aktör: &Aktör) -> String {
    let token = Token::ny();
    sqlx::query(
        "INSERT INTO session (hash, anvandare_id, skapad, senast_aktiv, passkey_anvand)
         VALUES (?1, ?2, ?3, ?3, ?3)",
    )
    .bind(hash(token.hemlighet()))
    .bind(aktör.användare.expect("en användare").0)
    .bind(klocka.nu().to_string())
    .execute(db.pool())
    .await
    .expect("testsession");
    token.hemlighet().to_string()
}

/// Kör en SQL-sats direkt mot databasen, förbi `core`, för att visa att schemat avvisar den.
pub async fn kör_sql(db: &Databas, sql: &'static str) -> Result<(), sqlx::Error> {
    sqlx::query(sql).execute(db.pool()).await.map(|_| ())
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

/// Loggar en start som `Databas::öppna` gör den när `till` startar efter `från` och har kört
/// `migreringar` (version, namn) med backupen `backup` först, vid `klocka`s tid.
pub async fn starta_om(
    db: &Databas,
    klocka: &Klocka,
    från: &str,
    till: &str,
    migreringar: &[(i64, &str)],
    backup: Option<&str>,
) {
    let mut tx = db.pool().begin().await.expect("transaktion");
    sqlx::query("UPDATE programversion SET version = ?")
        .bind(från)
        .execute(&mut *tx)
        .await
        .expect("programversionen");
    let körda = crate::databas::Migreringsresultat {
        migreringar: migreringar
            .iter()
            .map(|(v, namn)| (*v, namn.to_string()))
            .collect(),
        backup: backup.map(str::to_string),
    };
    crate::databas::logga_start(&mut tx, klocka.nu(), till, Some(&körda))
        .await
        .expect("starten loggas");
    tx.commit().await.expect("commit");
}
