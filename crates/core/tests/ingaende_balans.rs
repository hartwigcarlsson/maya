//! Ingående balans och preliminär ingående balans, genom `core` mot en tillfällig databas.

use jiff::civil::{Date, date};
use maya_core::aktor::Aktör;
use maya_core::atkomst::ÅtkomstFel;
use maya_core::foretag::FöretagsId;
use maya_core::kontoplan::{self, KontoFel};
use maya_core::saldo::{self, IngåendeBalansFel, SaldoFel, Status};
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_domain::belopp::Saldo;
use maya_domain::ingaende_balans::{Fält, NyIngåendeBalans, NySaldorad};
use maya_domain::kontoplan::Kontonummer;

const NU: &str = "2026-10-18T08:00:00Z";
const ÅR_2026: Date = date(2026, 1, 1);

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn rad(konto: &str, debet: &str, kredit: &str) -> NySaldorad {
    NySaldorad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
    }
}

/// Bank 1 000 kr och aktiekapital 1 000 kr.
fn startkapital() -> NyIngåendeBalans {
    NyIngåendeBalans {
        rader: vec![rad("1930", "1 000", ""), rad("2081", "", "1 000")],
    }
}

fn saldon(saldon: &[(Kontonummer, Saldo)]) -> Vec<(u16, i64)> {
    saldon.iter().map(|(k, s)| (k.get(), s.öre())).collect()
}

async fn ingående(
    db: &Databas,
    aktör: &Aktör,
    id: FöretagsId,
    år: Date,
) -> (Status, Vec<(u16, i64)>) {
    let ib = saldo::ingående_balans(db, aktör, id, år).await.unwrap();
    let rader = ib.saldon.iter().map(|(k, s)| (k.get(), s.öre())).collect();
    (ib.status, rader)
}

#[tokio::test]
async fn första_årets_ingående_balans_skrivs_in_och_läses_tillbaka() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    saldo::ändra_ingående_balans(&db, &klocka(), &anna, id, ÅR_2026, &startkapital())
        .await
        .unwrap();

    let ib = saldo::ingående_balans(&db, &anna, id, ÅR_2026)
        .await
        .unwrap();
    assert_eq!(ib.status, Status::Inskriven);
    assert_eq!((ib.år.start(), ib.år.slut()), (ÅR_2026, date(2026, 12, 31)));
    assert_eq!(saldon(&ib.saldon), [(1930, 100_000), (2081, -100_000)]);
}

#[tokio::test]
async fn första_året_utan_inskriven_ingående_balans_börjar_på_noll() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    assert_eq!(
        ingående(&db, &anna, id, ÅR_2026).await,
        (Status::Inskriven, vec![])
    );
}

#[tokio::test]
async fn en_ingående_balans_vars_summa_inte_är_noll_nekas_och_sparas_inte() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let mut ib = startkapital();
    ib.rader[1].kredit = "900".into();

    let fel = saldo::ändra_ingående_balans(&db, &klocka(), &anna, id, ÅR_2026, &ib)
        .await
        .unwrap_err();

    let IngåendeBalansFel::Spärrat(spärrar) = fel else {
        panic!("{fel:?}")
    };
    let koder: Vec<_> = spärrar.iter().map(|s| (s.fält, s.kod)).collect();
    assert_eq!(koder, [(Fält::Summa, "differens")]);
    // Formuläret får samma spärrar ur samma kontoplan.
    let konton = saldo::balanskonton(&db, &anna, id).await.unwrap();
    assert_eq!(ib.kontrollera(&konton).unwrap_err(), spärrar);
    assert_eq!(ingående(&db, &anna, id, ÅR_2026).await.1, vec![]);
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    assert!(historik.iter().all(|p| p.typ != "ingående balans ändrad"));
}

#[tokio::test]
async fn varje_ändring_av_den_ingående_balansen_loggas_med_aktör_tid_och_vad_som_ändrades() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    saldo::ändra_ingående_balans(&db, &klocka(), &anna, id, ÅR_2026, &startkapital())
        .await
        .unwrap();
    let senare = Klocka::fast("2026-10-19T09:30:00Z".parse().unwrap());
    let rättad = NyIngåendeBalans {
        rader: vec![
            rad("1930", "800", ""),
            rad("1510", "200", ""),
            rad("2081", "", "1 000"),
        ],
    };

    saldo::ändra_ingående_balans(&db, &senare, &anna, id, ÅR_2026, &rättad)
        .await
        .unwrap();
    // Samma saldon igen är ingen ändring.
    saldo::ändra_ingående_balans(&db, &senare, &anna, id, ÅR_2026, &rättad)
        .await
        .unwrap();

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let ändringar: Vec<_> = historik
        .iter()
        .filter(|p| p.typ == "ingående balans ändrad")
        .collect();
    assert_eq!(ändringar.len(), 2);
    assert_eq!(ändringar[0].tid, NU.parse().unwrap());
    assert_eq!(ändringar[1].tid, "2026-10-19T09:30:00Z".parse().unwrap());
    assert!(ändringar.iter().all(|p| p.aktör == anna));
    assert_eq!(ändringar[1].objekt, "2026-01-01 – 2026-12-31");
    assert_eq!(
        ändringar[1].beskrivning["ändringar"],
        serde_json::json!([
            { "konto": 1510, "före_öre": 0, "efter_öre": 20_000 },
            { "konto": 1930, "före_öre": 100_000, "efter_öre": 80_000 },
        ])
    );
    assert_eq!(
        ingående(&db, &anna, id, ÅR_2026).await.1,
        [(1510, 20_000), (1930, 80_000), (2081, -100_000)]
    );
}

#[tokio::test]
async fn den_preliminära_ingående_balansen_är_förra_årets_utgående_och_resultatet_på_2099() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    saldo::ändra_ingående_balans(&db, &klocka(), &anna, id, ÅR_2026, &startkapital())
        .await
        .unwrap();
    // 2026: försäljning 500 kr, en kostnad 100 kr och 20 kr på 2099 direkt.
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(1930, 50_000, 0), (3001, 0, 50_000)],
    )
    .await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-04-01",
        &[(6071, 10_000, 0), (1930, 0, 10_000)],
    )
    .await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-05-01",
        &[(1930, 2_000, 0), (2099, 0, 2_000)],
    )
    .await;
    // 2027 skapas av sitt första verifikat: försäljning 70 kr.
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2027-02-01",
        &[(1930, 7_000, 0), (3001, 0, 7_000)],
    )
    .await;
    // Ett utkast eller verifikat i 2028 behövs för att året ska finnas.
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2028-01-10",
        &[(1930, 100, 0), (3001, 0, 100)],
    )
    .await;

    // IB(1930, 2027) = 1 000 + 500 − 100 + 20; IB(2099, 2027) = −20 + (−500 + 100).
    assert_eq!(
        ingående(&db, &anna, id, date(2027, 1, 1)).await,
        (
            Status::Preliminär,
            vec![(1930, 142_000), (2081, -100_000), (2099, -42_000)]
        )
    );
    // Året efter bygger på den preliminära: 2027:s resultat läggs till på 2099.
    assert_eq!(
        ingående(&db, &anna, id, date(2028, 1, 1)).await,
        (
            Status::Preliminär,
            vec![(1930, 149_000), (2081, -100_000), (2099, -49_000)]
        )
    );
}

#[tokio::test]
async fn den_preliminära_ingående_balansen_sparas_inte_utan_följer_förra_året() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2027-02-01",
        &[(1930, 7_000, 0), (3001, 0, 7_000)],
    )
    .await;
    assert_eq!(
        ingående(&db, &anna, id, date(2027, 1, 1)).await,
        (Status::Preliminär, vec![])
    );

    saldo::ändra_ingående_balans(&db, &klocka(), &anna, id, ÅR_2026, &startkapital())
        .await
        .unwrap();

    assert_eq!(
        ingående(&db, &anna, id, date(2027, 1, 1)).await,
        (Status::Preliminär, vec![(1930, 100_000), (2081, -100_000)])
    );
    let fel = saldo::ändra_ingående_balans(
        &db,
        &klocka(),
        &anna,
        id,
        date(2027, 1, 1),
        &startkapital(),
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, IngåendeBalansFel::InteFörstaÅret), "{fel:?}");
}

#[tokio::test]
async fn saldobalansen_har_ingående_balans_förändring_och_utgående_balans_per_konto() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    saldo::ändra_ingående_balans(&db, &klocka(), &anna, id, ÅR_2026, &startkapital())
        .await
        .unwrap();
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(1930, 50_000, 0), (3001, 0, 50_000)],
    )
    .await;

    let sb = saldo::saldobalans(&db, &anna, id, ÅR_2026).await.unwrap();

    assert_eq!(sb.status, Status::Inskriven);
    let konton: Vec<_> = sb
        .konton
        .iter()
        .map(|k| {
            (
                k.konto.get(),
                k.ingående.öre(),
                k.förändring.öre(),
                k.utgående.öre(),
            )
        })
        .collect();
    assert_eq!(
        konton,
        [
            (1930, 100_000, 50_000, 150_000),
            (2081, -100_000, 0, -100_000),
            (3001, 0, -50_000, -50_000),
        ]
    );
}

#[tokio::test]
async fn ett_okänt_räkenskapsår_eller_ett_annat_företag_ger_ett_fel() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;

    let okänt = saldo::ingående_balans(&db, &anna, id, date(2025, 1, 1)).await;
    let främmande = saldo::ingående_balans(&db, &bertil, id, ÅR_2026).await;
    let ändra =
        saldo::ändra_ingående_balans(&db, &klocka(), &bertil, id, ÅR_2026, &startkapital()).await;

    assert!(
        matches!(okänt, Err(SaldoFel::OkäntRäkenskapsår)),
        "{okänt:?}"
    );
    assert!(matches!(
        främmande,
        Err(SaldoFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        ändra,
        Err(IngåendeBalansFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
}

#[tokio::test]
async fn ett_konto_med_ingående_balans_kan_inte_inaktiveras() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    saldo::ändra_ingående_balans(&db, &klocka(), &anna, id, ÅR_2026, &startkapital())
        .await
        .unwrap();
    // 2027 finns, och där har 1930 bara sin (preliminära) ingående balans.
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2027-02-01",
        &[(1510, 7_000, 0), (3001, 0, 7_000)],
    )
    .await;
    let bank = Kontonummer::new(1930).unwrap();

    let fel = kontoplan::inaktivera(&db, &klocka(), &anna, id, bank)
        .await
        .unwrap_err();

    let KontoFel::KanInteInaktiveras(spärr) = fel else {
        panic!("{fel:?}")
    };
    assert!(spärr.to_string().contains("1\u{a0}000,00 kr"), "{spärr}");
    let saldon = kontoplan::saldon(&db, &anna, id, bank).await.unwrap();
    let år: Vec<_> = saldon.iter().map(|s| (s.start, s.öre)).collect();
    assert_eq!(år, [(ÅR_2026, 100_000), (date(2027, 1, 1), 100_000)]);
}

#[tokio::test]
async fn ett_företag_med_ingående_balans_men_utan_bokförda_verifikat_kan_raderas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    saldo::ändra_ingående_balans(&db, &klocka(), &anna, id, ÅR_2026, &startkapital())
        .await
        .unwrap();

    maya_core::foretag::radera(&db, &klocka(), &anna, id)
        .await
        .unwrap();

    assert!(
        maya_core::foretag::mina_företag(&db, &anna)
            .await
            .unwrap()
            .is_empty()
    );
}
