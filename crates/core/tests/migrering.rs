//! Säkra schemamigreringar (A3, A4): backup och kontrollsummor före och efter, återställning
//! vid avvikelse, loggning i behandlingshistoriken och schemaepoken.

use std::path::{Path, PathBuf};

use maya_core::aktor::Aktör;
use maya_core::backup::{self, Backupmål};
use maya_core::behandlingshistorik::{self, Urval};
use maya_core::foretag::FöretagsId;
use maya_core::kontrollsumma::Kontrollsummor;
use maya_core::{Databas, DatabasFel, Klocka, testhjalp, underlag};
use sqlx::migrate::Migrator;

const VERSION: &str = "0.4.0";

/// En katalog med Mayas egna migreringar och `extra` (filnamn, SQL), som en senare Maya-version
/// skulle ha dem.
struct Migreringar {
    dir: tempfile::TempDir,
}

impl Migreringar {
    fn nya(extra: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let källa = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        for fil in std::fs::read_dir(källa).unwrap() {
            let fil = fil.unwrap();
            std::fs::copy(fil.path(), dir.path().join(fil.file_name())).unwrap();
        }
        for (namn, sql) in extra {
            std::fs::write(dir.path().join(namn), sql).unwrap();
        }
        Migreringar { dir }
    }

    async fn migrator(&self) -> Migrator {
        Migrator::new(self.dir.path()).await.unwrap()
    }
}

/// En installation med ett företag, två bokförda verifikat, ett utkast och underlag.
struct Installation {
    _dir: tempfile::TempDir,
    path: PathBuf,
    backup: PathBuf,
    anna: Aktör,
    företag: FöretagsId,
}

impl Installation {
    async fn med_bokföring() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join("maya.db");
        let backup = dir.path().join("backup");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&backup).unwrap();
        let migrator = Migreringar::nya(&[]).migrator().await;
        let db = Databas::öppna_som(&path, VERSION, &migrator, None)
            .await
            .unwrap();
        let (anna, företag) = testhjalp::företag(&db).await;
        let första = testhjalp::bokför(
            &db,
            &anna,
            företag,
            "2026-03-14",
            &[(1930, 12_500, 0), (3001, 0, 12_500)],
        )
        .await;
        testhjalp::bokför(
            &db,
            &anna,
            företag,
            "2026-04-02",
            &[(5410, 80_000, 0), (2641, 20_000, 0), (1930, 0, 100_000)],
        )
        .await;
        let k = Klocka::system();
        let kvitto = underlag::ladda_upp(&db, &k, &anna, företag, "kvitto.pdf", "", b"%PDF kvitto")
            .await
            .unwrap();
        underlag::komplettera(&db, &k, &anna, företag, första.id, kvitto.id)
            .await
            .unwrap();
        underlag::ladda_upp(&db, &k, &anna, företag, "okopplat.png", "", b"\x89PNG bild")
            .await
            .unwrap();
        db.stäng().await;
        Installation {
            _dir: dir,
            path,
            backup,
            anna,
            företag,
        }
    }

    fn mål(&self) -> Backupmål {
        Backupmål::katalog(&self.backup)
    }

    async fn öppna(
        &self,
        migreringar: &Migreringar,
        version: &str,
    ) -> Result<Databas, DatabasFel> {
        let migrator = migreringar.migrator().await;
        Databas::öppna_som(&self.path, version, &migrator, Some(&self.mål())).await
    }

    async fn kontrollsummor(&self) -> Kontrollsummor {
        let db = self.öppna(&Migreringar::nya(&[]), VERSION).await.unwrap();
        let summor = Kontrollsummor::räkna(&db).await.unwrap();
        db.stäng().await;
        summor
    }

    async fn historik(&self, db: &Databas) -> Vec<behandlingshistorik::Post> {
        behandlingshistorik::historik(db, &self.anna, self.företag, &Urval::default())
            .await
            .unwrap()
            .poster
    }
}

/// En vanlig migrering i en transaktion: ny tabell och ett nytt index.
const NY_TABELL: (&str, &str) = (
    "29990101000000_ny_tabell.sql",
    "CREATE TABLE anteckning (id INTEGER PRIMARY KEY, text TEXT NOT NULL) STRICT;
     CREATE INDEX verifikatrad_debet ON verifikatrad (debet);",
);

/// En migrering som bygger om en tabell som andra pekar på, med främmande nycklar avstängda
/// utanför en transaktion (samma mönster som rättelsens migrering).
const OMBYGGNAD: (&str, &str) = (
    "29990102000000_ombyggnad.sql",
    "-- no-transaction
PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;
BEGIN IMMEDIATE;
CREATE TABLE underlag_ny (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    foretag_id INTEGER NOT NULL REFERENCES foretag (id),
    filnamn TEXT NOT NULL,
    mime TEXT NOT NULL,
    storlek INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    innehall BLOB NOT NULL,
    uppladdad TEXT NOT NULL,
    uppladdad_kanal TEXT NOT NULL,
    uppladdad_av INTEGER REFERENCES anvandare (id)
) STRICT;
INSERT INTO underlag_ny SELECT id, foretag_id, filnamn, mime, storlek, sha256, innehall,
    uppladdad, uppladdad_kanal, uppladdad_av FROM underlag;
DROP TABLE underlag;
ALTER TABLE underlag_ny RENAME TO underlag;
COMMIT;
PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;",
);

/// En felaktig migrering som ändrar bokförda rader förbi skyddet.
const ÄNDRAR_BOKFÖRINGEN: (&str, &str) = (
    "29990103000000_andrar_bokforingen.sql",
    "DROP TRIGGER verifikatrad_bokford_andras_inte;
     UPDATE verifikatrad SET debet = debet * 2, kredit = kredit * 2;",
);

#[tokio::test]
async fn migreringarna_körs_mot_en_databas_med_data_och_kontrollsummorna_är_desamma_efter() {
    let inst = Installation::med_bokföring().await;
    let före = inst.kontrollsummor().await;

    let db = inst
        .öppna(&Migreringar::nya(&[NY_TABELL, OMBYGGNAD]), VERSION)
        .await
        .unwrap();

    let efter = Kontrollsummor::räkna(&db).await.unwrap();
    assert_eq!(före.verifikat.len(), 1);
    assert_eq!(före.verifikat[0].antal, 2);
    assert_eq!(före.underlag.len(), 2);
    assert_eq!(före, efter);
    assert!(före.avvikelser(&efter).is_empty());
    db.stäng().await;
}

#[tokio::test]
async fn före_migreringen_tas_en_backup_med_kontrollsummorna_i_backupmålet() {
    let inst = Installation::med_bokföring().await;
    let före = inst.kontrollsummor().await;
    assert!(backup::lista(&inst.mål()).await.unwrap().is_empty());

    let db = inst
        .öppna(&Migreringar::nya(&[NY_TABELL]), VERSION)
        .await
        .unwrap();
    db.stäng().await;

    let backuper = backup::lista(&inst.mål()).await.unwrap();
    assert_eq!(backuper.len(), 1);
    let manifest = backup::manifest(&inst.mål(), &backuper[0].namn)
        .await
        .unwrap();
    assert_eq!(manifest.kontrollsummor, före);
    // Backupen är den gamla databasen: utan den nya tabellen.
    let dir = tempfile::tempdir().unwrap();
    let fil = backup::hämta(&inst.mål(), &backuper[0].namn, dir.path())
        .await
        .unwrap();
    let finns = fråga(
        &fil,
        "SELECT count(*) FROM sqlite_master WHERE name = 'anteckning'",
    )
    .await;
    assert_eq!(finns, 0);
}

/// Ett tal ur databasfilen, förbi `core`.
async fn fråga(path: &Path, sql: &'static str) -> i64 {
    use sqlx::Connection;
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}?mode=ro", path.display()))
        .await
        .unwrap();
    let tal = sqlx::query_scalar(sql).fetch_one(&mut conn).await.unwrap();
    conn.close().await.unwrap();
    tal
}

#[tokio::test]
async fn ingen_backup_tas_när_inget_finns_att_migrera() {
    let inst = Installation::med_bokföring().await;

    inst.öppna(&Migreringar::nya(&[]), VERSION)
        .await
        .unwrap()
        .stäng()
        .await;

    assert!(backup::lista(&inst.mål()).await.unwrap().is_empty());
}

#[tokio::test]
async fn en_migrering_som_ändrar_bokförda_data_rullas_tillbaka_och_maya_startar_inte() {
    let inst = Installation::med_bokföring().await;
    let före = inst.kontrollsummor().await;

    let fel = inst
        .öppna(&Migreringar::nya(&[NY_TABELL, ÄNDRAR_BOKFÖRINGEN]), VERSION)
        .await
        .unwrap_err();

    assert!(
        matches!(&fel, DatabasFel::MigreringÄndradeBokföringen { avvikelser, .. } if !avvikelser.is_empty()),
        "{fel:?}"
    );
    assert!(fel.to_string().contains("konto 1930"), "{fel}");
    // Allt är som före: summorna, och ingen av migreringarna är körd (också den oskyldiga).
    assert_eq!(inst.kontrollsummor().await, före);
    let db = inst.öppna(&Migreringar::nya(&[]), VERSION).await.unwrap();
    let antal = fråga(
        &inst.path,
        "SELECT count(*) FROM sqlite_master WHERE name IN ('anteckning', 'verifikatrad_debet')",
    )
    .await;
    assert_eq!(antal, 0);
    assert!(
        !inst
            .historik(&db)
            .await
            .iter()
            .any(|p| p.typ == "migrering körd")
    );
    db.stäng().await;
    // Nästa start försöker igen och vägrar igen.
    assert!(
        inst.öppna(&Migreringar::nya(&[ÄNDRAR_BOKFÖRINGEN]), VERSION)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn en_migrering_som_misslyckas_halvvägs_rullas_tillbaka_helt() {
    let inst = Installation::med_bokföring().await;
    let före = inst.kontrollsummor().await;
    let trasig = (
        "29990104000000_trasig.sql",
        "-- no-transaction
         DROP TRIGGER verifikatrad_bokford_raderas_inte;
         DELETE FROM verifikatrad;
         SELECT * FROM finns_inte;",
    );

    let fel = inst
        .öppna(&Migreringar::nya(&[NY_TABELL, trasig]), VERSION)
        .await
        .unwrap_err();

    assert!(matches!(fel, DatabasFel::Migrering(_)), "{fel:?}");
    assert_eq!(inst.kontrollsummor().await, före);
}

#[tokio::test]
async fn migrering_körd_loggas_i_varje_företags_behandlingshistorik() {
    let inst = Installation::med_bokföring().await;

    let db = inst
        .öppna(&Migreringar::nya(&[NY_TABELL, OMBYGGNAD]), VERSION)
        .await
        .unwrap();

    let historik = inst.historik(&db).await;
    let poster: Vec<_> = historik
        .iter()
        .filter(|p| p.typ == "migrering körd")
        .collect();
    assert_eq!(poster.len(), 1);
    let post = poster[0];
    assert_eq!(post.aktörsnamn(), "Maya");
    assert_eq!(post.beskrivning["programversion"], VERSION);
    assert_eq!(
        post.beskrivning["migreringar"],
        serde_json::json!([
            {"version": 29990101000000_i64, "namn": "ny tabell"},
            {"version": 29990102000000_i64, "namn": "ombyggnad"},
        ])
    );
    assert_eq!(post.beskrivning["kontrollsummor"], "oförändrade");
    let backuper = backup::lista(&inst.mål()).await.unwrap();
    assert_eq!(post.beskrivning["backup"], backuper[0].namn.as_str());
    assert!(post.text().contains("2 migreringar"), "{}", post.text());
    db.stäng().await;
}

#[tokio::test]
async fn programversion_bytt_loggas_när_en_annan_version_startar() {
    let inst = Installation::med_bokföring().await;
    let migreringar = Migreringar::nya(&[]);

    inst.öppna(&migreringar, VERSION)
        .await
        .unwrap()
        .stäng()
        .await;
    inst.öppna(&migreringar, "0.5.0")
        .await
        .unwrap()
        .stäng()
        .await;
    let db = inst.öppna(&migreringar, "0.5.0").await.unwrap();

    let byten: Vec<_> = inst
        .historik(&db)
        .await
        .into_iter()
        .filter(|p| p.typ == "programversion bytt")
        .collect();
    assert_eq!(byten.len(), 1, "{byten:?}");
    assert_eq!(
        byten[0].beskrivning,
        serde_json::json!({"från": VERSION, "till": "0.5.0"})
    );
    assert_eq!(byten[0].aktörsnamn(), "Maya");
    assert_eq!(byten[0].text(), "Från 0.4.0 till 0.5.0");
    db.stäng().await;
}

#[tokio::test]
async fn schemaepoken_är_huvudversionen() {
    assert_eq!(maya_core::SCHEMAEPOK, "0");
    assert_eq!(maya_core::schemaepok_för("0.4.0"), "0");
    assert_eq!(maya_core::schemaepok_för("1.0.0"), "1");
    assert_eq!(maya_core::schemaepok_för("1.2.3-rc.1"), "1");
}

#[tokio::test]
async fn en_binär_för_1_x_vägrar_en_databas_från_epok_0_och_rör_den_inte() {
    let inst = Installation::med_bokföring().await;
    let före = inst.kontrollsummor().await;

    let fel = inst
        .öppna(&Migreringar::nya(&[NY_TABELL]), "1.0.0")
        .await
        .unwrap_err();

    assert!(
        matches!(&fel, DatabasFel::AnnanEpok { funnen, .. } if funnen == "0"),
        "{fel:?}"
    );
    assert!(fel.to_string().contains("schemaepok 1"), "{fel}");
    assert!(backup::lista(&inst.mål()).await.unwrap().is_empty());
    let db = inst.öppna(&Migreringar::nya(&[]), VERSION).await.unwrap();
    assert!(
        !inst
            .historik(&db)
            .await
            .iter()
            .any(|p| p.typ == "programversion bytt")
    );
    db.stäng().await;
    assert_eq!(inst.kontrollsummor().await, före);
}
