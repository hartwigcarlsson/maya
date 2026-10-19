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
    sie4::exportera_med(db, klocka, aktör, företag, år, |_| {}, förvanska).await
}

/// SIE4-exporten med en läsning som tappar årets sista verifikat, som en bugg i frågan som
/// filen byggs av, för att visa att avstämningen läser databasen för sig.
pub async fn exportera_sie4_med_felaktig_läsning(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    år: jiff::civil::Date,
) -> Result<sie4::Sie4Export, sie4::Sie4Fel> {
    sie4::exportera_med(
        db,
        klocka,
        aktör,
        företag,
        år,
        |verifikat| {
            verifikat.pop();
        },
        |_| {},
    )
    .await
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
    företag_med_metod(db, "faktura").await
}

/// Ett företag som [`företag`] med bokföringsmetoden `metod` (`faktura` eller `kontant`, #149).
pub async fn företag_med_metod(db: &Databas, metod: &str) -> (Aktör, FöretagsId) {
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
            bokföringsmetod: metod.into(),
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

/// Loggar en återställning från backupen som togs vid `backup_tid`, som `maya restore` gör
/// efter att databasen har bytts, utan att byta databasen.
pub async fn logga_återställning(db: &Databas, klocka: &Klocka, backup_tid: jiff::Timestamp) {
    let backup = crate::backup::Backup {
        namn: format!("maya-{}", backup_tid.strftime("%Y%m%dT%H%M%SZ")),
        tid: backup_tid,
    };
    crate::aterstallning::logga(
        db,
        &backup,
        "0.1.0",
        "katalogen /backup",
        klocka.nu(),
        &Aktör::cli(),
    )
    .await
    .expect("återställningen loggas");
}

/// En konfiguration utan driftvarningar: S3-mål. Inget SMTP.
pub fn config() -> crate::config::Config {
    crate::config::Config::from_env(
        [
            ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
            ("MAYA_BACKUP_S3_ENDPOINT", "https://s3.example.se"),
            ("MAYA_BACKUP_S3_BUCKET", "maya"),
            ("MAYA_BACKUP_S3_ACCESS_KEY_ID", "maya-backup"),
            ("MAYA_BACKUP_S3_SECRET_ACCESS_KEY", "hemlig"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .expect("testkonfigurationen")
}

/// Tar en lyckad backup vid `klocka`s tid till en katalog som tas bort efteråt, så att
/// backupens körning finns och ingen driftvarning om backupen visas.
pub async fn backup(db: &Databas, klocka: &Klocka) {
    let (mål, arbete) = (
        tempfile::tempdir().expect("tempdir"),
        tempfile::tempdir().expect("tempdir"),
    );
    let resultat = crate::backup::ta_backup(
        db,
        klocka,
        &Aktör::maya(),
        &crate::backup::Backupmål::katalog(mål.path()),
        arbete.path(),
    )
    .await
    .expect("testbackupen");
    assert!(
        resultat.verifiering.lyckades(),
        "testbackupen verifierades inte"
    );
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

/// Loggar en migrering som misslyckades och rullades tillbaka, utan att köra någon.
pub async fn misslyckad_migrering(
    db: &Databas,
    klocka: &Klocka,
    version: &str,
    migreringar: &[(i64, &str)],
    fel: &str,
) {
    let mut tx = db.pool().begin().await.expect("transaktion");
    let försök = crate::databas::Migreringsresultat {
        migreringar: migreringar
            .iter()
            .map(|(v, namn)| (*v, namn.to_string()))
            .collect(),
        backup: None,
    };
    crate::databas::logga_misslyckad_i(&mut tx, klocka.nu(), version, &försök, fel)
        .await
        .expect("misslyckandet loggas");
    tx.commit().await.expect("commit");
}

// #144 leverantörsregistret
/// En aktiv leverantör `namn` med standardkontot 5410 och bankgiro 5050-1055.
pub async fn leverantör(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    namn: &str,
) -> crate::leverantor::LeverantörsId {
    crate::leverantor::skapa(
        db,
        &Klocka::system(),
        aktör,
        företag,
        &maya_domain::leverantor::NyLeverantör {
            namn: namn.into(),
            bankgiro: "5050-1055".into(),
            standardkonto: "5410".into(),
            ..Default::default()
        },
    )
    .await
    .expect("testleverantör")
}

// #135 Kundfakturor
/// Ett företag som [`företag`], med de uppgifter som en kundfaktura kräver: adress, säte,
/// momsregistreringsnummer, F-skatt och bankgiro 5050-1055.
pub async fn fakturaföretag(db: &Databas) -> (Aktör, FöretagsId) {
    fakturaföretag_med_metod(db, "faktura").await
}

/// Ett företag som [`fakturaföretag`] med bokföringsmetoden `metod` (#149).
pub async fn fakturaföretag_med_metod(db: &Databas, metod: &str) -> (Aktör, FöretagsId) {
    let (anna, id) = företag_med_metod(db, metod).await;
    let klocka = Klocka::system();
    foretag::ändra_uppgifter(
        db,
        &klocka,
        &anna,
        id,
        &maya_domain::foretag::Uppgifter {
            namn: "Exempel AB".into(),
            gatuadress: "Exempelgatan 1".into(),
            postnummer: "12345".into(),
            postort: "Exempelstad".into(),
            säte: "Stockholm".into(),
            momsregnr: "SE556036079301".into(),
            f_skatt: true,
            ..Default::default()
        },
    )
    .await
    .expect("företagets uppgifter");
    crate::fakturauppgifter::ändra(
        db,
        &klocka,
        &anna,
        id,
        &maya_domain::fakturauppgifter::Fakturauppgifter {
            bankgiro: "5050-1055".into(),
            ..Default::default()
        },
    )
    .await
    .expect("fakturauppgifterna");
    (anna, id)
}

/// En aktiv kund `namn` med adress och 30 dagars betalningsvillkor.
pub async fn kund(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    namn: &str,
) -> crate::kund::KundId {
    crate::kund::skapa(
        db,
        &Klocka::system(),
        aktör,
        företag,
        &maya_domain::kund::NyKund {
            namn: namn.into(),
            adress: "Storgatan 1\n111 22 Stockholm".into(),
            epost: "ekonomi@kund.se".into(),
            ..Default::default()
        },
    )
    .await
    .expect("testkund")
}

/// Ett utkast till kundfaktura till `kund` daterat `fakturadatum`, med en rad: 10 timmar à
/// 950 kr med 25 % moms på 3001 (totalt 11 875 kr).
pub async fn kundfakturautkast(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    kund: crate::kund::KundId,
    fakturadatum: &str,
) -> crate::kundfaktura::KundfakturaId {
    crate::kundfaktura::spara_utkast(
        db,
        &Klocka::system(),
        aktör,
        företag,
        None,
        &maya_domain::kundfaktura::NyKundfaktura {
            kund: kund.to_string(),
            fakturadatum: fakturadatum.into(),
            rader: vec![maya_domain::kundfaktura::NyFakturarad {
                beskrivning: "Konsulttimmar".into(),
                antal: "10".into(),
                enhet: "tim".into(),
                à_pris: "950".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .await
    .expect("testutkastet")
}

/// En utfärdad kundfaktura som [`kundfakturautkast`] till en ny kund `namn`.
pub async fn utfärdad_kundfaktura(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    namn: &str,
    fakturadatum: &str,
) -> crate::kundfaktura::Utfärdad {
    let kund = kund(db, aktör, företag, namn).await;
    let utkast = kundfakturautkast(db, aktör, företag, kund, fakturadatum).await;
    crate::kundfaktura::utfärda(db, &Klocka::system(), aktör, företag, utkast)
        .await
        .expect("fakturan utfärdas")
}

// #138 Utskick
/// En brevlåda i stället för SMTP: sparar fakturamejlen, eller avvisar varje mejl med ett fel.
#[derive(Debug, Clone, Default)]
pub struct Brevlåda {
    mejl: std::sync::Arc<std::sync::Mutex<Vec<crate::utskick::Fakturamejl>>>,
    avvisar: Option<String>,
}

impl Brevlåda {
    pub fn ny() -> Self {
        Self::default()
    }

    /// En brevlåda som avvisar varje mejl med `fel`, som en SMTP-server som svarar 554.
    pub fn som_avvisar(fel: &str) -> Self {
        Brevlåda {
            avvisar: Some(fel.into()),
            ..Self::default()
        }
    }

    /// Mejlen som kom fram.
    pub fn mejl(&self) -> Vec<crate::utskick::Fakturamejl> {
        self.mejl.lock().expect("brevlådan").clone()
    }
}

impl crate::utskick::Epost for Brevlåda {
    async fn skicka(
        &self,
        mejl: &crate::utskick::Fakturamejl,
        _nu: jiff::Timestamp,
    ) -> Result<(), String> {
        match &self.avvisar {
            Some(fel) => Err(fel.clone()),
            None => {
                self.mejl.lock().expect("brevlådan").push(mejl.clone());
                Ok(())
            }
        }
    }
}

// #147 Betalning av leverantörsfaktura
/// En registrerad leverantörsfaktura `nummer` från `leverantör` på 1 250,00 kr (5410 netto
/// 1 000 kr och 25 % moms), fakturerad 2026-03-14 och förfallen 2026-04-13. `originalvaluta` är
/// fritext, t.ex. "EUR 109,50", eller tom.
pub async fn registrerad_leverantörsfaktura(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    leverantör: crate::leverantor::LeverantörsId,
    nummer: &str,
    originalvaluta: &str,
) -> crate::leverantorsfaktura::LeverantörsfakturaId {
    use crate::leverantorsfaktura;
    let klocka = Klocka::system();
    let innehåll = format!("%PDF-1.4 leverantörsfaktura {nummer}");
    let (id, _) = leverantorsfaktura::ladda_upp(
        db,
        &klocka,
        aktör,
        företag,
        (
            &format!("faktura-{nummer}.pdf"),
            "application/pdf",
            innehåll.as_bytes(),
        ),
    )
    .await
    .expect("originalet laddas upp");
    leverantorsfaktura::registrera(
        db,
        &klocka,
        aktör,
        företag,
        id,
        &maya_domain::leverantorsfaktura::NyLeverantörsfaktura {
            leverantör: leverantör.to_string(),
            fakturanummer: nummer.into(),
            fakturadatum: "2026-03-14".into(),
            förfallodag: "2026-04-13".into(),
            referens: "4455667788".into(),
            totalbelopp: "1 250,00".into(),
            originalvaluta: originalvaluta.into(),
            rader: vec![maya_domain::leverantorsfaktura::NyRad {
                konto: "5410".into(),
                netto: "1 000".into(),
                momssats: "25".into(),
                moms: String::new(),
            }],
            ..Default::default()
        },
    )
    .await
    .expect("fakturan registreras");
    id
}

// #140 Kreditfakturor
/// En utfärdad kreditfaktura som krediterar hela den utfärdade fakturan `original`.
pub async fn utfärdad_kreditfaktura(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    original: crate::kundfaktura::KundfakturaId,
) -> crate::kundfaktura::Utfärdad {
    let klocka = Klocka::system();
    let utkast = crate::kundfaktura::kreditera(
        db,
        &klocka,
        aktör,
        företag,
        original,
        &maya_domain::kreditfaktura::NyKreditering {
            hela: true,
            antal: Vec::new(),
        },
    )
    .await
    .expect("kreditutkastet");
    crate::kundfaktura::utfärda(db, &klocka, aktör, företag, utkast)
        .await
        .expect("kreditfakturan utfärdas")
}

// #143 Externa kundfakturor
/// En registrerad extern kundfaktura `nummer` till kunden `kund`: 10 000 kr på 3001 med 25 %
/// moms, 12 500,00 kr, fakturerad 2026-03-10 och förfallen 2026-04-09.
pub async fn registrerad_extern_kundfaktura(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    kund: crate::kund::KundId,
    nummer: &str,
) -> crate::kundfaktura::KundfakturaId {
    use crate::extern_kundfaktura;
    let klocka = Klocka::system();
    let innehåll = format!("%PDF-1.4 extern kundfaktura {nummer}");
    let (id, _) = extern_kundfaktura::ladda_upp(
        db,
        &klocka,
        aktör,
        företag,
        (
            &format!("{nummer}.pdf"),
            "application/pdf",
            innehåll.as_bytes(),
        ),
    )
    .await
    .expect("originalet laddas upp");
    extern_kundfaktura::registrera(
        db,
        &klocka,
        aktör,
        företag,
        id,
        &maya_domain::extern_kundfaktura::NyExternFaktura {
            kund: kund.to_string(),
            fakturanummer: nummer.into(),
            fakturadatum: "2026-03-10".into(),
            förfallodag: "2026-04-09".into(),
            rader: vec![maya_domain::extern_kundfaktura::NyExternRad {
                momssats: "25".into(),
                konto: "3001".into(),
                belopp: "10 000".into(),
            }],
            ..Default::default()
        },
    )
    .await
    .expect("den externa fakturan registreras");
    id
}
