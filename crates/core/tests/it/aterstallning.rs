//! `maya restore` (A5, #123): hela installationen återställs ur en backup, genom `core` mot
//! databasfiler i en tillfällig katalog.

use std::path::PathBuf;

use maya_core::aktor::Aktör;
use maya_core::aterstallning::{self, ÅterställningsFel};
use maya_core::att_gora::{self, Sak};
use maya_core::backup::{self, Backupmål};
use maya_core::behandlingshistorik::{self, Urval};
use maya_core::foretag::FöretagsId;
use maya_core::installation::{self, RelyingParty};
use maya_core::kontrollsumma::Kontrollsummor;
use maya_core::verifikat::{self, Urval as Verifikaturval};
use maya_core::{Databas, Klocka, installationslogg, testhjalp};

const BACKUPTID: &str = "2026-10-18T01:00:00Z";
const BACKUPNAMN: &str = "maya-20261018T010000Z";
const NU: &str = "2026-10-18T12:00:00Z";

fn klocka(tid: &str) -> Klocka {
    Klocka::fast(tid.parse().unwrap())
}

fn rp(värd: &str) -> RelyingParty {
    RelyingParty {
        rp_id: värd.to_string(),
        origin: format!("https://{värd}"),
    }
}

/// En installation på disk med en backup i ett katalogmål: ett företag med ett verifikat
/// bokfört före backupen och ett efter.
struct Installation {
    data: tempfile::TempDir,
    mål_dir: tempfile::TempDir,
    anna: Aktör,
    företag: FöretagsId,
}

impl Installation {
    async fn ny() -> Self {
        let data = tempfile::tempdir().unwrap();
        let mål_dir = tempfile::tempdir().unwrap();
        let db = Databas::öppna(&data.path().join("maya.db")).await.unwrap();
        installation::kontrollera_rp(&db, &rp("bokforing.example.se"))
            .await
            .unwrap();
        let (anna, företag) = testhjalp::företag(&db).await;
        testhjalp::bokför(
            &db,
            &anna,
            företag,
            "2026-03-14",
            &[(1930, 12_500, 0), (3001, 0, 12_500)],
        )
        .await;
        let mål = Backupmål::katalog(mål_dir.path());
        let resultat = backup::ta_backup(
            &db,
            &klocka(BACKUPTID),
            &Aktör::maya(),
            &mål,
            &data.path().join("backup-arbete"),
        )
        .await
        .unwrap();
        assert!(resultat.verifiering.lyckades());
        // Bokfört efter backupen: försvinner vid återställningen.
        testhjalp::bokför(
            &db,
            &anna,
            företag,
            "2026-10-18",
            &[(1930, 5_000, 0), (3001, 0, 5_000)],
        )
        .await;
        db.stäng().await;
        Installation {
            data,
            mål_dir,
            anna,
            företag,
        }
    }

    fn fil(&self) -> PathBuf {
        self.data.path().join("maya.db")
    }

    fn mål(&self) -> Backupmål {
        Backupmål::katalog(self.mål_dir.path())
    }

    async fn återställ(
        &self,
        aktör: &Aktör,
        rp: &RelyingParty,
    ) -> Result<aterstallning::Återställd, ÅterställningsFel> {
        aterstallning::återställ(
            &self.fil(),
            &self.mål(),
            BACKUPNAMN,
            rp,
            &klocka(NU),
            aktör,
            &self.data.path().join("backup-arbete"),
        )
        .await
    }

    async fn öppna(&self) -> Databas {
        Databas::öppna(&self.fil()).await.unwrap()
    }
}

#[tokio::test]
async fn den_senaste_backupen_väljs_med_sin_tidpunkt() {
    let inst = Installation::ny().await;

    let vald = aterstallning::välj(&inst.mål(), None).await.unwrap();

    assert_eq!(vald.backup.namn, BACKUPNAMN);
    assert_eq!(vald.backup.tid, BACKUPTID.parse().unwrap());
    assert_eq!(vald.manifest.kontrollsummor.verifikat[0].antal, 1);
    let okänd = aterstallning::välj(&inst.mål(), Some("maya-20200101T000000Z")).await;
    assert!(
        matches!(
            okänd,
            Err(ÅterställningsFel::Backup(backup::BackupFel::IngenBackup))
        ),
        "{okänd:?}"
    );
}

#[tokio::test]
async fn återställningen_ger_backupens_bokföring_och_kontrollsummor() {
    let inst = Installation::ny().await;
    let manifest = backup::manifest(&inst.mål(), BACKUPNAMN).await.unwrap();

    let återställd = inst
        .återställ(&Aktör::cli(), &rp("bokforing.example.se"))
        .await
        .unwrap();

    assert_eq!(återställd.backup.namn, BACKUPNAMN);
    assert_eq!(återställd.företag, 1);
    let db = inst.öppna().await;
    assert_eq!(
        Kontrollsummor::räkna(&db).await.unwrap(),
        manifest.kontrollsummor
    );
    let kvar = verifikat::lista(&db, &inst.anna, inst.företag, Verifikaturval::Bokförda)
        .await
        .unwrap();
    assert_eq!(kvar.len(), 1, "verifikatet efter backupen är borta");
    // Den gamla databasen ligger kvar bredvid, med det som bokfördes efter backupen.
    assert!(återställd.undanflyttad.is_file());
    assert_eq!(återställd.undanflyttad.parent(), Some(inst.data.path()));
}

#[tokio::test]
async fn återställningen_loggas_i_varje_företag_och_i_installationsloggen() {
    let inst = Installation::ny().await;
    inst.återställ(&Aktör::cli(), &rp("bokforing.example.se"))
        .await
        .unwrap();

    let db = inst.öppna().await;
    let historik = behandlingshistorik::historik(&db, &inst.anna, inst.företag, &Urval::default())
        .await
        .unwrap();
    let post = historik
        .poster
        .iter()
        .find(|p| p.typ == "återställning gjord")
        .expect("en post om återställningen");
    assert_eq!(post.tid, NU.parse().unwrap());
    assert_eq!(post.aktör, Aktör::cli());
    assert_eq!(post.objekt, BACKUPNAMN);
    assert_eq!(
        post.text(),
        "Hela installationen återställd från backupen 2026-10-18 03:00. Det som registrerades \
         efter den måste registreras om"
    );

    let logg = installationslogg::läs(&db).await.unwrap();
    let post = logg.last().unwrap();
    assert_eq!(post.typ, "återställning gjord");
    assert_eq!(post.aktör, Aktör::cli());
    assert_eq!(post.objekt, BACKUPNAMN);
    assert_eq!(post.beskrivning["backup_tid"], BACKUPTID);
    assert_eq!(post.beskrivning["företag"], 1);

    // Och Anna ser varningen överst under Att göra.
    let a = att_gora::att_göra(
        &db,
        &klocka(NU),
        &testhjalp::config(),
        &inst.anna,
        inst.företag,
    )
    .await
    .unwrap();
    assert!(
        matches!(&a.saker[0], Sak::Återställning(v) if v.backup_tid == BACKUPTID.parse().unwrap()),
        "{:?}",
        a.saker
    );
}

#[tokio::test]
async fn en_backup_med_annat_rp_id_vägras_och_databasen_lämnas_orörd() {
    let inst = Installation::ny().await;
    let före = std::fs::read(inst.fil()).unwrap();

    let fel = inst
        .återställ(&Aktör::cli(), &rp("annan.example.se"))
        .await
        .unwrap_err();

    match &fel {
        ÅterställningsFel::AnnatRpId {
            backupens,
            konfigurationens,
        } => {
            assert_eq!(backupens, "bokforing.example.se");
            assert_eq!(konfigurationens, "annan.example.se");
        }
        fel => panic!("{fel:?}"),
    }
    assert_eq!(std::fs::read(inst.fil()).unwrap(), före);
    let db = inst.öppna().await;
    let bokförda = verifikat::lista(&db, &inst.anna, inst.företag, Verifikaturval::Bokförda)
        .await
        .unwrap();
    assert_eq!(bokförda.len(), 2);
}

#[tokio::test]
async fn en_backup_som_inte_stämmer_med_manifestet_vägras() {
    let inst = Installation::ny().await;
    // Någon har ändrat databasfilen i målet efter backupen.
    let fil = inst.mål_dir.path().join(format!("{BACKUPNAMN}.db"));
    let mut innehåll = std::fs::read(&fil).unwrap();
    let sista = innehåll.len() - 1;
    innehåll[sista] ^= 0xff;
    std::fs::write(&fil, innehåll).unwrap();

    let fel = inst
        .återställ(&Aktör::cli(), &rp("bokforing.example.se"))
        .await
        .unwrap_err();

    assert!(matches!(fel, ÅterställningsFel::Backup(_)), "{fel:?}");
    let db = inst.öppna().await;
    let bokförda = verifikat::lista(&db, &inst.anna, inst.företag, Verifikaturval::Bokförda)
        .await
        .unwrap();
    assert_eq!(bokförda.len(), 2);
}

/// Ändrar backupens databasfil i målet med `sql` och skriver om manifestets storlek och
/// SHA-256, så att backupen ser hel ut för granskningen före bytet.
async fn ändra_backupen(inst: &Installation, sql: &'static str) {
    use sha2::Digest;
    use sqlx::Connection;
    let fil = inst.mål_dir.path().join(format!("{BACKUPNAMN}.db"));
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", fil.display()))
        .await
        .unwrap();
    sqlx::raw_sql(sql).execute(&mut conn).await.unwrap();
    conn.close().await.unwrap();
    let innehåll = std::fs::read(&fil).unwrap();
    let manifestfil = inst.mål_dir.path().join(format!("{BACKUPNAMN}.json"));
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifestfil).unwrap()).unwrap();
    manifest["databas"]["byte"] = innehåll.len().into();
    manifest["databas"]["sha256"] = hex::encode(sha2::Sha256::digest(&innehåll)).into();
    std::fs::write(&manifestfil, serde_json::to_vec(&manifest).unwrap()).unwrap();
}

#[tokio::test]
async fn när_den_återställda_databasen_inte_kan_öppnas_läggs_den_gamla_tillbaka() {
    let inst = Installation::ny().await;
    // Backupen säger sig ha en migrering som den här Maya-versionen inte känner till, så den
    // vägras först när den ligger på plats och öppnas.
    ändra_backupen(
        &inst,
        "INSERT INTO _sqlx_migrations
             (version, description, installed_on, success, checksum, execution_time)
         VALUES (29990101000000, 'framtida', '2999-01-01', 1, x'00', 0)",
    )
    .await;
    let före = std::fs::read(inst.fil()).unwrap();

    let fel = inst
        .återställ(&Aktör::cli(), &rp("bokforing.example.se"))
        .await
        .unwrap_err();

    assert!(matches!(fel, ÅterställningsFel::Databas(_)), "{fel:?}");
    assert_eq!(std::fs::read(inst.fil()).unwrap(), före);
    // Ingen undanflyttad fil ligger kvar bredvid databasen.
    let filer: Vec<String> = std::fs::read_dir(inst.data.path())
        .unwrap()
        .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|f| f.starts_with("maya.db"))
        .collect();
    assert_eq!(filer, ["maya.db"]);
    let db = inst.öppna().await;
    let bokförda = verifikat::lista(&db, &inst.anna, inst.företag, Verifikaturval::Bokförda)
        .await
        .unwrap();
    assert_eq!(bokförda.len(), 2);
    let historik = behandlingshistorik::historik(&db, &inst.anna, inst.företag, &Urval::default())
        .await
        .unwrap();
    assert!(
        !historik
            .poster
            .iter()
            .any(|p| p.typ == "återställning gjord")
    );
}

#[tokio::test]
async fn bara_cli_återställer() {
    let inst = Installation::ny().await;
    for aktör in [inst.anna, Aktör::maya()] {
        let fel = inst
            .återställ(&aktör, &rp("bokforing.example.se"))
            .await
            .unwrap_err();
        assert!(matches!(fel, ÅterställningsFel::KanalenFårInte), "{fel:?}");
    }
}
