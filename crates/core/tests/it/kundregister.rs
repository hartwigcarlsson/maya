//! Kundregistret: skapa, ändra, arkivera och radera en kund genom `core` mot en tillfällig
//! databas, med behandlingshistoriken för varje åtgärd (H2).

use maya_core::atkomst::ÅtkomstFel;
use maya_core::kund::{self, KundFel, Kundstatus};
use maya_core::{Databas, Klocka, behandlingshistorik, foretag, testhjalp};
use maya_domain::foretag::NyttFöretag;
use maya_domain::kund::{Fält, NyKund, OmvändSkattskyldighet};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn kund_ab() -> NyKund {
    NyKund {
        namn: "Kund AB".into(),
        identitet: "5560360793".into(),
        adress: "Storgatan 1\n111 22 Stockholm".into(),
        epost: "Faktura@Kund.se".into(),
        momsregnr: "SE556036079301".into(),
        betalningsvillkor: "30".into(),
        omvänd_skattskyldighet: "bygg".into(),
    }
}

#[tokio::test]
async fn en_kund_skapas_i_registret_och_loggas_med_aktör_och_tid() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    let k = kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();

    let kunden = kund::kunden(&db, &anna, id, k).await.unwrap();
    assert_eq!(kunden.id, k);
    assert_eq!(kunden.namn, "Kund AB");
    assert_eq!(kunden.identitet, "556036-0793");
    assert_eq!(kunden.adress, "Storgatan 1\n111 22 Stockholm");
    assert_eq!(kunden.epost, "faktura@kund.se");
    assert_eq!(kunden.momsregnr, "SE556036079301");
    assert_eq!(kunden.betalningsvillkor, 30);
    assert_eq!(kunden.omvänd_skattskyldighet, OmvändSkattskyldighet::Bygg);
    assert_eq!(kunden.status, Kundstatus::Aktiv);

    let post = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(post.typ, "kund skapad");
    assert_eq!(post.objekt, "Kund AB");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.tid.to_string(), NU);
    assert_eq!(post.beskrivning["id"].to_string(), k.to_string());
    assert_eq!(post.beskrivning["identitet"], "556036-0793");
    assert_eq!(post.beskrivning["betalningsvillkor"], 30);
    assert_eq!(post.beskrivning["omvänd_skattskyldighet"], "bygg");
    assert_eq!(
        post.text(),
        "Kund AB, 556036-0793, faktura@kund.se, 30 dagar, omvänd skattskyldighet: Bygg inom Sverige"
    );
}

#[tokio::test]
async fn en_felaktig_kund_nekas_med_samma_koder_som_formuläret() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let före = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .len();

    let fel = kund::skapa(
        &db,
        &klocka(),
        &anna,
        id,
        &NyKund {
            identitet: "556036-0794".into(),
            epost: "faktura@".into(),
            ..kund_ab()
        },
    )
    .await
    .unwrap_err();

    let KundFel::Ogiltigt(fel) = fel else {
        panic!("{fel:?}")
    };
    assert_eq!(
        fel.iter().map(|f| (f.fält, f.kod)).collect::<Vec<_>>(),
        [
            (Fält::Identitet, "identitet_kontrollsiffra"),
            (Fält::Epost, "epost_ogiltig")
        ]
    );
    assert!(kund::kunder(&db, &anna, id).await.unwrap().is_empty());
    assert_eq!(
        behandlingshistorik::läs(&db, &anna, id)
            .await
            .unwrap()
            .len(),
        före
    );
}

#[tokio::test]
async fn en_ändrad_kund_loggas_med_det_som_ändrades() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let k = kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();

    kund::ändra(
        &db,
        &klocka(),
        &anna,
        id,
        k,
        &NyKund {
            namn: "Kund Bygg AB".into(),
            betalningsvillkor: "10".into(),
            ..kund_ab()
        },
    )
    .await
    .unwrap();

    let kunden = kund::kunden(&db, &anna, id, k).await.unwrap();
    assert_eq!(kunden.namn, "Kund Bygg AB");
    assert_eq!(kunden.betalningsvillkor, 10);
    let post = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(post.typ, "kund ändrad");
    assert_eq!(post.objekt, "Kund Bygg AB");
    assert_eq!(post.aktör, anna);
    assert_eq!(
        post.beskrivning["ändringar"],
        serde_json::json!({
            "namn": { "från": "Kund AB", "till": "Kund Bygg AB" },
            "betalningsvillkor": { "från": 30, "till": 10 },
        })
    );
    assert_eq!(
        post.text(),
        "betalningsvillkor: ”30” → ”10”; namn: ”Kund AB” → ”Kund Bygg AB”"
    );
}

#[tokio::test]
async fn en_oförändrad_kund_loggas_inte() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let k = kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();
    let före = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .len();

    kund::ändra(&db, &klocka(), &anna, id, k, &kund_ab())
        .await
        .unwrap();

    assert_eq!(
        behandlingshistorik::läs(&db, &anna, id)
            .await
            .unwrap()
            .len(),
        före
    );
}

#[tokio::test]
async fn en_arkiverad_kund_finns_kvar_i_registret_men_inte_i_väljaren() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let k = kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();
    let annan = kund::skapa(
        &db,
        &klocka(),
        &anna,
        id,
        &NyKund {
            namn: "Annan AB".into(),
            ..NyKund::default()
        },
    )
    .await
    .unwrap();

    kund::arkivera(&db, &klocka(), &anna, id, k).await.unwrap();

    let registret: Vec<_> = kund::kunder(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .map(|k| (k.namn, k.status))
        .collect();
    assert_eq!(
        registret,
        [
            ("Annan AB".to_string(), Kundstatus::Aktiv),
            ("Kund AB".to_string(), Kundstatus::Arkiverad)
        ]
    );
    let väljbara: Vec<_> = kund::väljbara(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .map(|k| k.id)
        .collect();
    assert_eq!(väljbara, [annan]);
    let post = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(post.typ, "kund arkiverad");
    assert_eq!(post.objekt, "Kund AB");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.tid.to_string(), NU);
    assert_eq!(post.text(), "Arkiverad; visas inte längre i väljarna");
}

#[tokio::test]
async fn en_arkiverad_kund_kan_ändras_och_arkiveras_inte_två_gånger() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let k = kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();
    kund::arkivera(&db, &klocka(), &anna, id, k).await.unwrap();
    let före = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .len();

    kund::arkivera(&db, &klocka(), &anna, id, k).await.unwrap();
    assert_eq!(
        behandlingshistorik::läs(&db, &anna, id)
            .await
            .unwrap()
            .len(),
        före
    );

    kund::ändra(
        &db,
        &klocka(),
        &anna,
        id,
        k,
        &NyKund {
            epost: "ny@kund.se".into(),
            ..kund_ab()
        },
    )
    .await
    .unwrap();
    let kunden = kund::kunden(&db, &anna, id, k).await.unwrap();
    assert_eq!(kunden.epost, "ny@kund.se");
    assert_eq!(kunden.status, Kundstatus::Arkiverad);
}

#[tokio::test]
async fn en_kund_utan_fakturor_raderas_och_raderingen_loggas_med_uppgifterna() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let k = kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();

    kund::radera(&db, &klocka(), &anna, id, k).await.unwrap();

    assert!(kund::kunder(&db, &anna, id).await.unwrap().is_empty());
    assert!(matches!(
        kund::kunden(&db, &anna, id, k).await,
        Err(KundFel::FinnsInte)
    ));
    let post = behandlingshistorik::läs(&db, &anna, id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(post.typ, "kund raderad");
    assert_eq!(post.objekt, "Kund AB");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.tid.to_string(), NU);
    assert_eq!(post.beskrivning["identitet"], "556036-0793");
    assert_eq!(post.text(), "Kund AB, 556036-0793 raderad ur kundregistret");
}

#[tokio::test]
async fn en_raderad_kunds_id_används_aldrig_igen() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let första = kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();
    kund::radera(&db, &klocka(), &anna, id, första)
        .await
        .unwrap();

    let andra = kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();

    assert_ne!(första, andra);
}

#[tokio::test]
async fn ett_annat_företags_kund_finns_inte() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let k = kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();
    let annat = foretag::lägg_upp(
        &db,
        &klocka(),
        &anna,
        &NyttFöretag {
            orgnr: "202100-6255".into(),
            namn: "Annat AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "kvartal".into(),
        },
    )
    .await
    .unwrap();

    assert!(matches!(
        kund::kunden(&db, &anna, annat, k).await,
        Err(KundFel::FinnsInte)
    ));
    assert!(matches!(
        kund::ändra(&db, &klocka(), &anna, annat, k, &kund_ab()).await,
        Err(KundFel::FinnsInte)
    ));
    assert!(matches!(
        kund::arkivera(&db, &klocka(), &anna, annat, k).await,
        Err(KundFel::FinnsInte)
    ));
    assert!(matches!(
        kund::radera(&db, &klocka(), &anna, annat, k).await,
        Err(KundFel::FinnsInte)
    ));
    assert_eq!(kund::kunder(&db, &anna, id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_läsa_eller_ändra_registret() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let k = kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();
    let bo = testhjalp::användare(&db, "bo@example.se").await;

    assert!(matches!(
        kund::skapa(&db, &klocka(), &bo, id, &kund_ab()).await,
        Err(KundFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        kund::ändra(&db, &klocka(), &bo, id, k, &kund_ab()).await,
        Err(KundFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        kund::arkivera(&db, &klocka(), &bo, id, k).await,
        Err(KundFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        kund::radera(&db, &klocka(), &bo, id, k).await,
        Err(KundFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        kund::kunder(&db, &bo, id).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
    assert!(matches!(
        kund::kunden(&db, &bo, id, k).await,
        Err(KundFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
}

#[tokio::test]
async fn ett_företag_med_kunder_men_utan_räkenskapsinformation_kan_raderas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    kund::skapa(&db, &klocka(), &anna, id, &kund_ab())
        .await
        .unwrap();

    foretag::radera(&db, &klocka(), &anna, id).await.unwrap();

    assert!(foretag::mina_företag(&db, &anna).await.unwrap().is_empty());
}
