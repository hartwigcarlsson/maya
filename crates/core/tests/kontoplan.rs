//! Kontoplanens regler, genom `core` mot en tillfällig databas.

use maya_core::aktor::{Aktör, Kanal};
use maya_core::atkomst::ÅtkomstFel;
use maya_core::foretag::{self, FöretagsId};
use maya_core::kontoplan::{self, Konto, KontoFel, Ursprung};
use maya_core::{Databas, Klocka, testhjalp};
use maya_domain::foretag::NyttFöretag;
use maya_domain::kontoplan::{Inaktiveringsspärr, Kontonummer, NyttKonto};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn nr(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

async fn företag(db: &Databas) -> (Aktör, FöretagsId) {
    let anna = testhjalp::användare(db, "anna@example.se").await;
    let formulär = NyttFöretag {
        orgnr: "556036-0793".into(),
        namn: "Exempel AB".into(),
        första_år_start: "2026-01-01".into(),
        första_år_slut: "2026-12-31".into(),
        bokföringsmetod: "faktura".into(),
        momsperiod: "kvartal".into(),
    };
    let id = foretag::lägg_upp(db, &klocka(), &anna, &formulär)
        .await
        .unwrap();
    (anna, id)
}

async fn konto(db: &Databas, aktör: &Aktör, id: FöretagsId, n: u16) -> Option<Konto> {
    kontoplan::konton(db, aktör, id)
        .await
        .unwrap()
        .into_iter()
        .find(|k| k.nummer == nr(n))
}

/// Kontots poster i behandlingshistoriken som (typ, beskrivning).
async fn historik(
    db: &Databas,
    aktör: &Aktör,
    id: FöretagsId,
    n: u16,
) -> Vec<(String, serde_json::Value)> {
    kontoplan::historik(db, aktör, id, nr(n))
        .await
        .unwrap()
        .into_iter()
        .map(|p| (p.typ, p.beskrivning))
        .collect()
}

#[tokio::test]
async fn ett_namnbyte_sparas_och_loggas_med_det_gamla_och_nya_namnet() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    kontoplan::byt_namn(&db, &klocka(), &anna, id, nr(1930), " Företagskonto SEB ")
        .await
        .unwrap();

    let k = konto(&db, &anna, id, 1930).await.unwrap();
    assert_eq!(k.namn, "Företagskonto SEB");
    assert_eq!(k.bas_namn.as_deref(), Some("Företagskonto"));
    let poster = kontoplan::historik(&db, &anna, id, nr(1930)).await.unwrap();
    let senaste = poster.last().unwrap();
    assert_eq!(senaste.typ, "konto namnbytt");
    assert_eq!(senaste.objekt, "1930");
    assert_eq!(senaste.aktör, anna);
    assert_eq!(senaste.användarnamn.as_deref(), Some("anna@example.se"));
    assert_eq!(senaste.tid, NU.parse().unwrap());
    assert_eq!(
        senaste.beskrivning,
        serde_json::json!({
            "från": "Företagskonto",
            "till": "Företagskonto SEB",
        })
    );
}

#[tokio::test]
async fn ett_tomt_namn_nekas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    let fel = kontoplan::byt_namn(&db, &klocka(), &anna, id, nr(1930), "  ")
        .await
        .unwrap_err();

    assert!(matches!(fel, KontoFel::NamnTomt), "{fel:?}");
}

#[tokio::test]
async fn aktivering_och_inaktivering_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    assert!(!konto(&db, &anna, id, 6212).await.unwrap().aktiv);

    kontoplan::aktivera(&db, &klocka(), &anna, id, nr(6212))
        .await
        .unwrap();
    assert!(konto(&db, &anna, id, 6212).await.unwrap().aktiv);
    kontoplan::inaktivera(&db, &klocka(), &anna, id, nr(6212))
        .await
        .unwrap();
    assert!(!konto(&db, &anna, id, 6212).await.unwrap().aktiv);

    let typer: Vec<_> = historik(&db, &anna, id, 6212)
        .await
        .into_iter()
        .map(|(typ, _)| typ)
        .collect();
    assert_eq!(
        typer,
        ["företag skapat", "konto aktiverat", "konto inaktiverat"]
    );
}

#[tokio::test]
async fn ett_konto_som_flödena_bokför_på_kan_inte_inaktiveras() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    let fel = kontoplan::inaktivera(&db, &klocka(), &anna, id, nr(2099))
        .await
        .unwrap_err();

    let KontoFel::KanInteInaktiveras(spärr) = fel else {
        panic!("{fel:?}");
    };
    assert_eq!(
        spärr,
        Inaktiveringsspärr::Flöde("Preliminär ingående balans")
    );
    assert_eq!(
        spärr.to_string(),
        "Används av flödet Preliminär ingående balans"
    );
    assert!(konto(&db, &anna, id, 2099).await.unwrap().aktiv);
    assert_eq!(historik(&db, &anna, id, 2099).await.len(), 1);
}

#[tokio::test]
async fn inaktivering_av_ett_konto_med_saldo_i_ett_öppet_år_nekas_med_skäl_och_saldo() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-01",
        &[(1930, 123_450, 0), (2081, 0, 123_450)],
    )
    .await;

    let fel = kontoplan::inaktivera(&db, &klocka(), &anna, id, nr(1930))
        .await
        .unwrap_err();

    let KontoFel::KanInteInaktiveras(spärr) = fel else {
        panic!("{fel:?}");
    };
    assert_eq!(
        spärr.to_string(),
        "Saldo 1 234,50 kr i räkenskapsåret 2026-01-01–2026-12-31, som inte är stängt. \
         Bara konton med nollsaldo kan inaktiveras."
    );
    assert!(konto(&db, &anna, id, 1930).await.unwrap().aktiv);
    let saldon = kontoplan::saldon(&db, &anna, id, nr(2081)).await.unwrap();
    assert_eq!(saldon[0].saldo.öre(), -123_450);
}

#[tokio::test]
async fn ett_eget_konto_skapas_aktivt_och_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let nytt = NyttKonto {
        nummer: "6542".into(),
        namn: "Molntjänster".into(),
    };

    let nummer = kontoplan::skapa_eget(&db, &klocka(), &anna, id, &nytt)
        .await
        .unwrap();

    assert_eq!(nummer, nr(6542));
    let k = konto(&db, &anna, id, 6542).await.unwrap();
    assert_eq!(
        (
            k.namn.as_str(),
            k.ursprung,
            k.aktiv,
            k.bas_namn,
            k.bas_version
        ),
        ("Molntjänster", Ursprung::Eget, true, None, None)
    );
    assert_eq!(
        historik(&db, &anna, id, 6542).await,
        [(
            "konto skapat".to_string(),
            serde_json::json!({ "namn": "Molntjänster" })
        )]
    );
}

#[tokio::test]
async fn ett_befintligt_nummer_nekas_och_säger_om_kontot_är_inaktivt() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let nytt = |nummer: &str| NyttKonto {
        nummer: nummer.into(),
        namn: "Dubblett".into(),
    };

    let aktivt = kontoplan::skapa_eget(&db, &klocka(), &anna, id, &nytt("1930"))
        .await
        .unwrap_err();
    let inaktivt = kontoplan::skapa_eget(&db, &klocka(), &anna, id, &nytt("6212"))
        .await
        .unwrap_err();

    assert!(
        matches!(aktivt, KontoFel::Finns { aktiv: true, .. }),
        "{aktivt:?}"
    );
    let KontoFel::Finns { namn, aktiv, .. } = inaktivt else {
        panic!("{inaktivt:?}");
    };
    assert_eq!((namn.as_str(), aktiv), ("Mobiltelefon", false));
}

#[tokio::test]
async fn klass_9_nekas_med_samma_kod_som_formuläret() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let nytt = NyttKonto {
        nummer: "9100".into(),
        namn: "Internt".into(),
    };

    let fel = kontoplan::skapa_eget(&db, &klocka(), &anna, id, &nytt)
        .await
        .unwrap_err();

    let KontoFel::Ogiltigt(fel) = fel else {
        panic!("{fel:?}");
    };
    assert_eq!(fel[0].kod, "nummer_klass_9");
}

#[tokio::test]
async fn en_annan_användare_kan_inte_ändra_kontoplanen() {
    let db = Databas::tillfällig().await;
    let (_, id) = företag(&db).await;
    let bertil = testhjalp::användare(&db, "bertil@example.se").await;

    let fel = kontoplan::aktivera(&db, &klocka(), &bertil, id, nr(6212))
        .await
        .unwrap_err();

    assert!(
        matches!(fel, KontoFel::Åtkomst(ÅtkomstFel::IngenÅtkomst)),
        "{fel:?}"
    );
}

#[tokio::test]
async fn ett_konto_som_inte_finns_kan_inte_ändras() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    let fel = kontoplan::aktivera(&db, &klocka(), &anna, id, nr(3150))
        .await
        .unwrap_err();

    assert!(matches!(fel, KontoFel::FinnsInte), "{fel:?}");
}

#[tokio::test]
async fn kontonummer_ändras_aldrig_och_konton_raderas_aldrig() {
    let db = Databas::tillfällig().await;
    let (_, _) = företag(&db).await;

    let ändra = testhjalp::kör_sql(&db, "UPDATE konto SET nummer = 1931 WHERE nummer = 1930").await;
    let radera = testhjalp::kör_sql(&db, "DELETE FROM konto WHERE nummer = 1930").await;
    let byt_ursprung = testhjalp::kör_sql(
        &db,
        "UPDATE konto SET ursprung = 'eget' WHERE nummer = 1930",
    )
    .await;

    assert!(
        ändra
            .unwrap_err()
            .to_string()
            .contains("kontonumret går inte att ändra")
    );
    assert!(
        radera
            .unwrap_err()
            .to_string()
            .contains("ett konto raderas aldrig")
    );
    assert!(byt_ursprung.is_err());
}

#[tokio::test]
async fn ett_bas_tillägg_lägger_bara_till_nya_konton_som_inaktiva_och_behåller_egna() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let eget = NyttKonto {
        nummer: "6542".into(),
        namn: "Molntjänster".into(),
    };
    kontoplan::skapa_eget(&db, &klocka(), &anna, id, &eget)
        .await
        .unwrap();
    kontoplan::byt_namn(&db, &klocka(), &anna, id, nr(1930), "Företagskonto SEB")
        .await
        .unwrap();
    let före = kontoplan::konton(&db, &anna, id).await.unwrap();

    let tillagda = kontoplan::bas_tillägg(
        &db,
        &klocka(),
        "BAS 2027",
        &[
            (nr(1930), "Nytt BAS-namn på företagskontot"),
            (nr(6542), "Molntjänster i BAS"),
            (nr(5425), "Molntjänster och licenser"),
        ],
    )
    .await
    .unwrap();

    assert_eq!(tillagda, 1);
    let efter = kontoplan::konton(&db, &anna, id).await.unwrap();
    assert_eq!(efter.len(), före.len() + 1);
    let ny = konto(&db, &anna, id, 5425).await.unwrap();
    assert_eq!(
        (
            ny.namn.as_str(),
            ny.ursprung,
            ny.aktiv,
            ny.bas_version.as_deref(),
            ny.ny_i_bas
        ),
        (
            "Molntjänster och licenser",
            Ursprung::Bas,
            false,
            Some("BAS 2027"),
            true
        )
    );
    let eget = konto(&db, &anna, id, 6542).await.unwrap();
    assert_eq!(
        (eget.namn.as_str(), eget.ursprung),
        ("Molntjänster", Ursprung::Eget)
    );
    assert_eq!(
        konto(&db, &anna, id, 1930).await.unwrap().namn,
        "Företagskonto SEB"
    );
    assert!(!konto(&db, &anna, id, 1930).await.unwrap().ny_i_bas);

    let poster = kontoplan::historik(&db, &anna, id, nr(5425)).await.unwrap();
    assert_eq!(poster.len(), 1);
    assert_eq!(poster[0].typ, "BAS-tillägg");
    assert_eq!(poster[0].aktör.kanal, Kanal::Maya);
    assert_eq!(poster[0].aktör.användare, None);
    assert_eq!(
        poster[0].beskrivning,
        serde_json::json!({ "bas_version": "BAS 2027", "konton": [5425] })
    );
}

#[tokio::test]
async fn den_inbyggda_bas_versionen_lägger_inte_till_något_i_ett_nytt_företag() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    let tillagda = kontoplan::lägg_till_inbyggd_bas(&db, &klocka())
        .await
        .unwrap();

    assert_eq!(tillagda, 0);
    assert_eq!(historik(&db, &anna, id, 1930).await.len(), 1);
}
