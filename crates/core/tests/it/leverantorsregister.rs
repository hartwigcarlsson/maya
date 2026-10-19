//! Leverantörsregistret genom `core`: skapa, ändra, arkivera och radera, och att varje åtgärd
//! loggas (H2), standardkontot som egen typ.

use maya_core::atkomst::ÅtkomstFel;
use maya_core::foretag::FöretagsId;
use maya_core::leverantor::{self, LeverantörsFel, Status, Urval};
use maya_core::{Databas, Klocka, behandlingshistorik, kontoplan, testhjalp};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::leverantor::{Fält, NyLeverantör, OmvändSkattskyldighet};

const NU: &str = "2026-10-19T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn kontorsvaror() -> NyLeverantör {
    NyLeverantör {
        namn: "Kontorsvaror AB".into(),
        orgnr: "556036-0793".into(),
        gatuadress: "Lagervägen 1".into(),
        postnummer: "411 01".into(),
        postort: "Göteborg".into(),
        land: "SE".into(),
        momsregnr: "SE556036079301".into(),
        bankgiro: "5050-1055".into(),
        standardkonto: "5410".into(),
        ..Default::default()
    }
}

/// Posterna i företagets historik som `(typ, objekt, text)`.
async fn historik(
    db: &Databas,
    aktör: &maya_core::aktor::Aktör,
    id: FöretagsId,
) -> Vec<(String, String, String)> {
    behandlingshistorik::läs(db, aktör, id)
        .await
        .unwrap()
        .into_iter()
        .filter(|p| p.typ.contains("leverantör") || p.typ.contains("standardkonto"))
        .map(|p| (p.typ.clone(), p.objekt.clone(), p.text()))
        .collect()
}

#[tokio::test]
async fn en_leverantör_skapas_och_skapandet_och_standardkontot_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    let l = leverantor::skapa(&db, &klocka(), &anna, id, &kontorsvaror())
        .await
        .unwrap();

    let lev = leverantor::leverantören(&db, &anna, id, l)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lev.status, Status::Aktiv);
    assert_eq!(lev.uppgifter.namn, "Kontorsvaror AB");
    assert_eq!(lev.uppgifter.bankgiro, "5050-1055");
    assert_eq!(lev.uppgifter.standardkonto, Kontonummer::new(5410));
    assert!(lev.kan_raderas);
    assert_eq!(
        historik(&db, &anna, id).await,
        [
            (
                "leverantör skapad".into(),
                "Kontorsvaror AB".into(),
                "”Kontorsvaror AB” upplagd, orgnr 556036-0793".into()
            ),
            (
                "standardkonto ändrat".into(),
                "Kontorsvaror AB".into(),
                "Standardkonto för ”Kontorsvaror AB”: inget → 5410".into()
            ),
        ]
    );
}

#[tokio::test]
async fn en_ändring_loggar_fälten_som_ändrades_och_standardkontot_som_egen_typ() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let l = leverantor::skapa(&db, &klocka(), &anna, id, &kontorsvaror())
        .await
        .unwrap();

    let ändrad = NyLeverantör {
        postort: "Mölndal".into(),
        standardkonto: "5010".into(),
        omvänd_skattskyldighet: "bygg".into(),
        ..kontorsvaror()
    };
    leverantor::ändra(&db, &klocka(), &anna, id, l, &ändrad)
        .await
        .unwrap();
    // Samma uppgifter igen ändrar inget och loggar inget.
    leverantor::ändra(&db, &klocka(), &anna, id, l, &ändrad)
        .await
        .unwrap();

    let lev = leverantor::leverantören(&db, &anna, id, l)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lev.uppgifter.postort, "Mölndal");
    assert_eq!(
        lev.uppgifter.omvänd_skattskyldighet,
        OmvändSkattskyldighet::Bygg
    );
    let poster = historik(&db, &anna, id).await;
    assert_eq!(poster.len(), 4, "{poster:?}");
    assert_eq!(poster[2].0, "leverantör ändrad");
    assert_eq!(
        poster[2].2,
        "omvänd skattskyldighet: ”ingen” → ”bygg”; postort: ”Göteborg” → ”Mölndal”"
    );
    assert_eq!(
        poster[3],
        (
            "standardkonto ändrat".into(),
            "Kontorsvaror AB".into(),
            "Standardkonto för ”Kontorsvaror AB”: 5410 → 5010".into()
        )
    );
}

#[tokio::test]
async fn bara_standardkontot_ändrat_loggar_bara_standardkontot() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let l = leverantor::skapa(&db, &klocka(), &anna, id, &kontorsvaror())
        .await
        .unwrap();

    let utan = NyLeverantör {
        standardkonto: String::new(),
        ..kontorsvaror()
    };
    leverantor::ändra(&db, &klocka(), &anna, id, l, &utan)
        .await
        .unwrap();

    let poster = historik(&db, &anna, id).await;
    assert_eq!(poster.len(), 3, "{poster:?}");
    assert_eq!(
        poster[2].2,
        "Standardkonto för ”Kontorsvaror AB”: 5410 → inget"
    );
}

#[tokio::test]
async fn ett_formulär_med_fel_sparas_inte_och_ger_formulärets_koder() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let inaktivt = kontoplan::konton(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .find(|k| !k.aktiv && k.nummer.klass() == 5)
        .unwrap()
        .nummer;

    let fel = leverantor::skapa(
        &db,
        &klocka(),
        &anna,
        id,
        &NyLeverantör {
            bankgiro: "5050-1056".into(),
            standardkonto: inaktivt.get().to_string(),
            ..kontorsvaror()
        },
    )
    .await
    .unwrap_err();

    let LeverantörsFel::Ogiltigt(fel) = fel else {
        panic!("{fel:?}")
    };
    let koder: Vec<(&str, &str)> = fel.iter().map(|f| (f.fält.namn(), f.kod)).collect();
    assert_eq!(
        koder,
        [
            ("bankgiro", "bankgiro_kontrollsiffra"),
            ("standardkonto", "konto_inaktivt")
        ]
    );
    assert_eq!(fel[0].fält, Fält::Bankgiro);
    assert!(
        leverantor::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(historik(&db, &anna, id).await.is_empty());
}

#[tokio::test]
async fn en_arkiverad_leverantör_finns_inte_bland_de_aktiva_och_arkiveringen_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let k = leverantor::skapa(&db, &klocka(), &anna, id, &kontorsvaror())
        .await
        .unwrap();
    leverantor::skapa(
        &db,
        &klocka(),
        &anna,
        id,
        &NyLeverantör {
            namn: "Fikabröd".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    leverantor::arkivera(&db, &klocka(), &anna, id, k)
        .await
        .unwrap();

    let namn = |urval| {
        let db = db.clone();
        async move {
            leverantor::lista(&db, &anna, id, urval)
                .await
                .unwrap()
                .into_iter()
                .map(|l| l.uppgifter.namn)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(namn(Urval::Aktiva).await, ["Fikabröd"]);
    assert_eq!(namn(Urval::Arkiverade).await, ["Kontorsvaror AB"]);
    assert_eq!(namn(Urval::Alla).await, ["Fikabröd", "Kontorsvaror AB"]);
    let lev = leverantor::leverantören(&db, &anna, id, k)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lev.status, Status::Arkiverad);
    let poster = historik(&db, &anna, id).await;
    assert_eq!(
        poster.last().unwrap(),
        &(
            "leverantör arkiverad".into(),
            "Kontorsvaror AB".into(),
            "”Kontorsvaror AB” arkiverad".into()
        )
    );
    // En leverantör som redan är arkiverad arkiveras inte igen.
    leverantor::arkivera(&db, &klocka(), &anna, id, k)
        .await
        .unwrap();
    assert_eq!(historik(&db, &anna, id).await.len(), poster.len());
}

#[tokio::test]
async fn en_leverantör_utan_fakturor_raderas_och_raderingen_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let l = leverantor::skapa(&db, &klocka(), &anna, id, &kontorsvaror())
        .await
        .unwrap();

    leverantor::radera(&db, &klocka(), &anna, id, l)
        .await
        .unwrap();

    assert!(
        leverantor::leverantören(&db, &anna, id, l)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        historik(&db, &anna, id).await.last().unwrap(),
        &(
            "leverantör raderad".into(),
            "Kontorsvaror AB".into(),
            "”Kontorsvaror AB” raderad".into()
        )
    );
    assert!(matches!(
        leverantor::radera(&db, &klocka(), &anna, id, l).await,
        Err(LeverantörsFel::FinnsInte)
    ));
}

#[tokio::test]
async fn leverantörens_historik_visar_bara_dess_egna_poster() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let k = leverantor::skapa(&db, &klocka(), &anna, id, &kontorsvaror())
        .await
        .unwrap();
    leverantor::skapa(
        &db,
        &klocka(),
        &anna,
        id,
        &NyLeverantör {
            namn: "Fikabröd".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    leverantor::arkivera(&db, &klocka(), &anna, id, k)
        .await
        .unwrap();

    let typer: Vec<String> = leverantor::historik(&db, &anna, id, k)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.typ)
        .collect();
    assert_eq!(
        typer,
        [
            "leverantör skapad",
            "standardkonto ändrat",
            "leverantör arkiverad"
        ]
    );
}

#[tokio::test]
async fn en_leverantör_hör_till_sitt_företag() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let l = leverantor::skapa(&db, &klocka(), &anna, id, &kontorsvaror())
        .await
        .unwrap();
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let annat = maya_core::foretag::lägg_upp(
        &db,
        &klocka(),
        &bo,
        &maya_domain::foretag::NyttFöretag {
            orgnr: "212000-0142".into(),
            namn: "Annat AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "kvartal".into(),
        },
    )
    .await
    .unwrap();

    assert!(
        leverantor::leverantören(&db, &bo, annat, l)
            .await
            .unwrap()
            .is_none()
    );
    for fel in [
        leverantor::arkivera(&db, &klocka(), &bo, annat, l).await,
        leverantor::radera(&db, &klocka(), &bo, annat, l).await,
        leverantor::ändra(&db, &klocka(), &bo, annat, l, &kontorsvaror()).await,
    ] {
        assert!(matches!(fel, Err(LeverantörsFel::FinnsInte)), "{fel:?}");
    }
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_se_eller_ändra_leverantörer() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let l = leverantor::skapa(&db, &klocka(), &anna, id, &kontorsvaror())
        .await
        .unwrap();
    let bo = testhjalp::användare(&db, "bo@example.se").await;

    assert!(matches!(
        leverantor::lista(&db, &bo, id, Urval::Alla).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
    assert!(matches!(
        leverantor::skapa(&db, &klocka(), &bo, id, &kontorsvaror()).await,
        Err(LeverantörsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        leverantor::radera(&db, &klocka(), &bo, id, l).await,
        Err(LeverantörsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
}
