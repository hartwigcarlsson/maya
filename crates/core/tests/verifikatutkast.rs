//! Att spara ett verifikat som utkast, öppna det igen, bokföra det och radera det, genom `core`
//! mot en tillfällig databas.

use jiff::civil::date;
use maya_core::aktor::Aktör;
use maya_core::verifikat::{self, Urval, VerifikatFel};
use maya_core::{Databas, Klocka, behandlingshistorik, foretag, testhjalp};
use maya_domain::belopp::Belopp;
use maya_domain::verifikat::{Fält, NyRad, NyttVerifikat};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn rad(konto: &str, debet: &str, kredit: &str) -> NyRad {
    NyRad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
        text: String::new(),
    }
}

#[tokio::test]
async fn ett_obalanserat_och_ofullständigt_utkast_sparas_och_öppnas_igen() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let mut halvfärdigt = NyttVerifikat {
        datum: "2026-03-14".into(),
        beskrivning: String::new(),
        rader: vec![
            rad("6540", "599", ""),
            rad("", "", "748,75"),
            rad("", "", ""),
        ],
    };
    halvfärdigt.rader[1].text = "Kortet".into();

    let utkast = verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &halvfärdigt)
        .await
        .unwrap();

    let v = verifikat::verifikatet(&db, &anna, id, utkast)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.nummer, None);
    assert_eq!(v.bokförd, None);
    assert_eq!(v.datum, Some(date(2026, 3, 14)));
    assert_eq!(v.beskrivning, "");
    let rader: Vec<_> = v
        .rader
        .iter()
        .map(|r| {
            (
                r.konto.map(|k| k.get()),
                r.debet.öre(),
                r.kredit.öre(),
                r.text.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        rader,
        [
            (Some(6540), 59_900, 0, None),
            (None, 0, 74_875, Some("Kortet"))
        ]
    );
    assert_eq!(v.skapad.aktör, anna);
    let lista = verifikat::lista(&db, &anna, id, Urval::Utkast)
        .await
        .unwrap();
    assert_eq!(lista.len(), 1);
    assert_eq!(lista[0].id, utkast);
    assert_eq!(lista[0].nummer, None);
    assert_eq!(lista[0].summa, Belopp::från_öre(59_900));
}

fn försäljning(datum: &str) -> NyttVerifikat {
    NyttVerifikat {
        datum: datum.into(),
        beskrivning: "Försäljning kontant".into(),
        rader: vec![rad("1930", "1 250", ""), rad("3001", "", "1 250")],
    }
}

#[tokio::test]
async fn ett_utkast_ändras_och_sparas_igen_med_nytt_innehåll() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let mut f = försäljning("2026-03-14");
    f.rader[1].kredit = "1 000".into();
    let utkast = verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &f)
        .await
        .unwrap();

    f.rader[1].kredit = "1 250".into();
    f.rader.push(rad("", "", ""));
    f.beskrivning = "Försäljning, rättad".into();
    let igen = verifikat::spara_utkast(&db, &klocka(), &anna, id, Some(utkast), &f)
        .await
        .unwrap();

    assert_eq!(igen, utkast);
    let v = verifikat::verifikatet(&db, &anna, id, utkast)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.beskrivning, "Försäljning, rättad");
    assert_eq!(v.rader.len(), 2);
    assert_eq!(v.rader[1].kredit, Belopp::från_öre(125_000));
    assert_eq!(
        verifikat::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn ett_utkast_får_sitt_nummer_först_när_det_bokförs_och_bokföringen_följer_spärrarna() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let mut f = försäljning("2026-03-14");
    f.rader[1].kredit = "1 000".into();
    f.rader.push(rad("1950", "", ""));
    let utkast = verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &f)
        .await
        .unwrap();
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-04-01",
        &[(1930, 100, 0), (3001, 0, 100)],
    )
    .await;

    let fel = verifikat::bokför_utkast(&db, &klocka(), &anna, id, utkast, &f)
        .await
        .unwrap_err();
    let VerifikatFel::Spärrat(spärrar) = fel else {
        panic!("{fel:?}")
    };
    let koder: Vec<_> = spärrar.iter().map(|s| (s.fält, s.kod)).collect();
    assert_eq!(
        koder,
        [
            (Fält::Konto(2), "konto_inaktivt"),
            (Fält::Belopp(2), "belopp_saknas"),
            (Fält::Summa, "differens"),
        ]
    );
    let v = verifikat::verifikatet(&db, &anna, id, utkast)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.nummer, None);
    assert_eq!(v.rader.len(), 3, "utkastet är orört");

    let rätt = försäljning("2026-03-14");
    let b = verifikat::bokför_utkast(&db, &klocka(), &anna, id, utkast, &rätt)
        .await
        .unwrap();

    assert_eq!(b.id, utkast);
    assert_eq!(b.nummer.to_string(), "V2");
    let v = verifikat::verifikatet(&db, &anna, id, utkast)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.rader.len(), 2);
    assert_eq!(v.bokförd.unwrap().aktör, anna);
    let typer: Vec<_> = v.historik.iter().map(|h| h.post.typ.as_str()).collect();
    assert_eq!(typer, ["verifikat bokfört"]);
    assert!(
        verifikat::lista(&db, &anna, id, Urval::Utkast)
            .await
            .unwrap()
            .is_empty()
    );
    // Ett bokfört verifikat är inget utkast längre.
    let igen = verifikat::bokför_utkast(&db, &klocka(), &anna, id, utkast, &rätt).await;
    assert!(matches!(igen, Err(VerifikatFel::IngetUtkast)), "{igen:?}");
    let sparat = verifikat::spara_utkast(&db, &klocka(), &anna, id, Some(utkast), &f).await;
    assert!(
        matches!(sparat, Err(VerifikatFel::IngetUtkast)),
        "{sparat:?}"
    );
}

#[tokio::test]
async fn ett_utkast_sparas_inte_med_det_som_inte_går_att_lagra() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let f = NyttVerifikat {
        datum: "2026-13-01".into(),
        beskrivning: String::new(),
        rader: vec![rad("19300", "", ""), rad("1930", "12,345", "")],
    };

    let fel = verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &f)
        .await
        .unwrap_err();

    let VerifikatFel::Spärrat(spärrar) = fel else {
        panic!("{fel:?}")
    };
    let koder: Vec<_> = spärrar.iter().map(|s| (s.fält, s.kod)).collect();
    assert_eq!(
        koder,
        [
            (Fält::Datum, "datum_ogiltigt"),
            (Fält::Konto(0), "konto_okant"),
            (Fält::Belopp(1), "belopp_ogiltigt"),
        ]
    );
    assert!(
        verifikat::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn ett_utkast_utan_datum_och_rader_sparas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let f = NyttVerifikat {
        beskrivning: "Kvitto från järnhandeln".into(),
        ..Default::default()
    };

    let utkast = verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &f)
        .await
        .unwrap();

    let v = verifikat::verifikatet(&db, &anna, id, utkast)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((v.datum, v.räkenskapsår, v.period), (None, None, None));
    assert!(v.rader.is_empty());
    assert_eq!(v.beskrivning, "Kvitto från järnhandeln");
}

#[tokio::test]
async fn ett_utkast_i_nästa_räkenskapsår_skapar_året_i_samma_transaktion() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let mut f = försäljning("2027-02-01");
    f.rader.truncate(1);

    let utkast = verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &f)
        .await
        .unwrap();

    let företaget = foretag::företaget(&db, &anna, id).await.unwrap();
    assert_eq!(företaget.räkenskapsår.len(), 2);
    let år = &företaget.räkenskapsår[1];
    assert_eq!((år.start, år.slut), (date(2027, 1, 1), date(2027, 12, 31)));
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let typer: Vec<_> = historik.iter().map(|p| p.typ.as_str()).collect();
    assert_eq!(typer, ["företag skapat", "räkenskapsår skapat"]);
    let v = verifikat::verifikatet(&db, &anna, id, utkast)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.räkenskapsår.unwrap().start(), date(2027, 1, 1));
    assert_eq!(v.period.unwrap().start, date(2027, 2, 1));

    // Ett datum längre fram ger spärren, och inget år skapas.
    let längre_fram = försäljning("2029-01-01");
    let fel = verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &längre_fram)
        .await
        .unwrap_err();
    let VerifikatFel::Spärrat(spärrar) = fel else {
        panic!("{fel:?}")
    };
    assert_eq!(spärrar[0].kod, "saknat_rakenskapsar");
    let företaget = foretag::företaget(&db, &anna, id).await.unwrap();
    assert_eq!(företaget.räkenskapsår.len(), 2);
}

#[tokio::test]
async fn att_radera_ett_utkast_loggas_med_vem_när_och_en_sammanfattning() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let mut f = försäljning("2026-03-14");
    f.rader.push(rad("", "", "1"));
    let utkast = verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &f)
        .await
        .unwrap();

    verifikat::radera_utkast(&db, &klocka(), &anna, id, utkast)
        .await
        .unwrap();

    assert_eq!(
        verifikat::verifikatet(&db, &anna, id, utkast)
            .await
            .unwrap(),
        None
    );
    assert!(
        verifikat::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .is_empty()
    );
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik.last().unwrap();
    assert_eq!(post.typ, "utkast raderat");
    assert_eq!(post.objekt, "Försäljning kontant");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.tid, NU.parse().unwrap());
    assert_eq!(post.beskrivning["datum"], "2026-03-14");
    assert_eq!(post.beskrivning["beskrivning"], "Försäljning kontant");
    assert_eq!(post.beskrivning["summa_öre"], 125_000);
    assert_eq!(post.beskrivning["antal_rader"], 3);

    // Ett nytt verifikat som får samma id visar inte raderingen i sin historik.
    let b = testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-04-01",
        &[(1930, 100, 0), (3001, 0, 100)],
    )
    .await;
    let v = verifikat::verifikatet(&db, &anna, id, b.id)
        .await
        .unwrap()
        .unwrap();
    let typer: Vec<_> = v.historik.iter().map(|h| h.post.typ.as_str()).collect();
    assert_eq!(typer, ["verifikat bokfört"]);
}

#[tokio::test]
async fn ett_bokfört_verifikat_raderas_aldrig_som_utkast() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let b = testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-04-01",
        &[(1930, 100, 0), (3001, 0, 100)],
    )
    .await;

    let fel = verifikat::radera_utkast(&db, &klocka(), &anna, id, b.id).await;

    assert!(matches!(fel, Err(VerifikatFel::IngetUtkast)), "{fel:?}");
    assert!(
        verifikat::verifikatet(&db, &anna, id, b.id)
            .await
            .unwrap()
            .is_some()
    );
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    assert_eq!(historik.last().unwrap().typ, "verifikat bokfört");
}

#[tokio::test]
async fn bara_en_användare_med_åtkomst_i_webben_sparar_bokför_och_raderar_utkast() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let f = försäljning("2026-03-14");
    let utkast = verifikat::spara_utkast(&db, &klocka(), &anna, id, None, &f)
        .await
        .unwrap();

    for aktör in [bo, Aktör::cli()] {
        let k = klocka();
        let fel = [
            verifikat::spara_utkast(&db, &k, &aktör, id, None, &f)
                .await
                .map(|_| ()),
            verifikat::spara_utkast(&db, &k, &aktör, id, Some(utkast), &f)
                .await
                .map(|_| ()),
            verifikat::bokför_utkast(&db, &k, &aktör, id, utkast, &f)
                .await
                .map(|_| ()),
            verifikat::radera_utkast(&db, &k, &aktör, id, utkast).await,
        ];
        for fel in fel {
            assert!(matches!(fel, Err(VerifikatFel::Åtkomst(_))), "{fel:?}");
        }
    }
    let v = verifikat::verifikatet(&db, &anna, id, utkast)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.nummer, None);
}
