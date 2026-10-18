//! Att registrera och bokföra ett verifikat, genom `core` mot en tillfällig databas.

use jiff::civil::date;
use maya_core::aktor::{Aktör, Kanal};
use maya_core::atkomst::ÅtkomstFel;
use maya_core::foretag::FöretagsId;
use maya_core::verifikat::{self, Urval, VerifikatFel};
use maya_core::{Databas, Klocka, behandlingshistorik, foretag, testhjalp};
use maya_domain::belopp::Belopp;
use maya_domain::foretag::NyttFöretag;
use maya_domain::verifikat::{Fält, NyRad, NyttVerifikat};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

/// Ett företag med kalenderåret 2026 som första räkenskapsår.
async fn företag(db: &Databas) -> (Aktör, FöretagsId) {
    let anna = testhjalp::användare(db, "anna@example.se").await;
    let id = foretag::lägg_upp(
        db,
        &klocka(),
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
    .unwrap();
    (anna, id)
}

fn rad(konto: &str, debet: &str, kredit: &str) -> NyRad {
    NyRad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
        text: String::new(),
    }
}

fn försäljning(datum: &str) -> NyttVerifikat {
    NyttVerifikat {
        datum: datum.into(),
        beskrivning: "Försäljning kontant".into(),
        rader: vec![rad("1930", "1 250", ""), rad("3001", "", "1 250")],
    }
}

#[tokio::test]
async fn ett_bokfört_verifikat_får_v1_och_visas_med_rader_och_vem_som_bokförde_det() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let mut f = försäljning("2026-03-14");
    f.rader[0].text = "Kassan".into();

    let bokfört = verifikat::bokför(&db, &klocka(), &anna, id, &f)
        .await
        .unwrap();

    assert_eq!(bokfört.nummer.to_string(), "V1");
    let v = verifikat::verifikatet(&db, &anna, id, bokfört.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.nummer.map(|n| n.to_string()).as_deref(), Some("V1"));
    assert_eq!(v.datum, Some(date(2026, 3, 14)));
    assert_eq!(v.beskrivning, "Försäljning kontant");
    assert_eq!(v.summa, Belopp::från_öre(125_000));
    let rader: Vec<_> = v
        .rader
        .iter()
        .map(|r| {
            (
                r.konto.unwrap().get(),
                r.debet.öre(),
                r.kredit.öre(),
                r.text.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        rader,
        [(1930, 125_000, 0, Some("Kassan")), (3001, 0, 125_000, None)]
    );
    let år = v.räkenskapsår.unwrap();
    assert_eq!(
        (år.start(), år.slut()),
        (date(2026, 1, 1), date(2026, 12, 31))
    );
    let period = v.period.unwrap();
    assert_eq!(
        (period.start, period.slut),
        (date(2026, 3, 1), date(2026, 3, 31))
    );
    let bokförd = v.bokförd.unwrap();
    assert_eq!(bokförd.tid, NU.parse().unwrap());
    assert_eq!(bokförd.aktör, anna);
    assert_eq!(bokförd.namn, "anna@example.se");
    assert_eq!(v.skapad.aktör.kanal, Kanal::Webb);
    assert_eq!(v.skapad.tid, NU.parse().unwrap());
}

#[tokio::test]
async fn varje_spärr_nekar_bokföringen_med_samma_kod_som_formuläret_visar() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let mut f = försäljning("2025-12-31");
    f.beskrivning.clear();
    f.rader[1].kredit = "1 000".into();
    f.rader.push(rad("1950", "1", ""));
    f.rader.push(rad("9999", "", "1"));

    let fel = verifikat::bokför(&db, &klocka(), &anna, id, &f)
        .await
        .unwrap_err();

    let VerifikatFel::Spärrat(spärrar) = fel else {
        panic!("{fel:?}")
    };
    let koder: Vec<_> = spärrar.iter().map(|s| (s.fält, s.kod)).collect();
    assert_eq!(
        koder,
        [
            (Fält::Datum, "fore_forsta_aret"),
            (Fält::Beskrivning, "beskrivning_saknas"),
            (Fält::Konto(2), "konto_inaktivt"),
            (Fält::Konto(3), "konto_okant"),
            (Fält::Summa, "differens"),
        ]
    );
    // Formuläret får samma spärrar ur samma förutsättningar.
    let förutsättningar = verifikat::förutsättningar(&db, &anna, id).await.unwrap();
    assert_eq!(f.kontrollera(&förutsättningar).unwrap_err(), spärrar);
    assert!(
        verifikat::lista(&db, &anna, id, Urval::Alla)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn bokförda_verifikat_numreras_v1_v2_v3_i_den_ordning_de_bokförs() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    for datum in ["2026-05-01", "2026-02-01", "2026-05-01"] {
        verifikat::bokför(&db, &klocka(), &anna, id, &försäljning(datum))
            .await
            .unwrap();
    }

    let lista = verifikat::lista(&db, &anna, id, Urval::Bokförda)
        .await
        .unwrap();
    let nummer: Vec<_> = lista
        .iter()
        .map(|v| (v.nummer.unwrap().to_string(), v.datum.unwrap()))
        .collect();
    assert_eq!(
        nummer,
        [
            ("V3".to_string(), date(2026, 5, 1)),
            ("V2".to_string(), date(2026, 2, 1)),
            ("V1".to_string(), date(2026, 5, 1)),
        ]
    );
    assert_eq!(lista[0].summa, Belopp::från_öre(125_000));
    assert!(
        verifikat::lista(&db, &anna, id, Urval::Utkast)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn samtidiga_bokföringar_ger_varken_dubbletter_eller_luckor() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    let uppgifter: Vec<_> = (0..24)
        .map(|i| {
            let db: Databas = (*db).clone();
            // Hälften i året efter det senaste, så att också skapandet av året krockar.
            let datum = if i % 2 == 0 {
                "2026-06-01"
            } else {
                "2027-06-01"
            };
            tokio::spawn(async move {
                verifikat::bokför(&db, &klocka(), &anna, id, &försäljning(datum)).await
            })
        })
        .collect();
    for uppgift in uppgifter {
        uppgift.await.unwrap().unwrap();
    }

    let lista = verifikat::lista(&db, &anna, id, Urval::Alla).await.unwrap();
    for år in [2026, 2027] {
        let mut nummer: Vec<u32> = lista
            .iter()
            .filter(|v| v.datum.unwrap().year() == år)
            .map(|v| v.nummer.unwrap().get())
            .collect();
        nummer.sort();
        assert_eq!(nummer, (1..=12).collect::<Vec<_>>(), "{år}");
    }
    let företaget = foretag::företaget(&db, &anna, id).await.unwrap();
    assert_eq!(företaget.räkenskapsår.len(), 2);
}

#[tokio::test]
async fn två_räkenskapsår_har_öppna_serier_samtidigt() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    let mut nummer = Vec::new();
    for datum in ["2026-12-30", "2027-01-02", "2026-12-31", "2027-01-03"] {
        let b = verifikat::bokför(&db, &klocka(), &anna, id, &försäljning(datum))
            .await
            .unwrap();
        nummer.push(b.nummer.to_string());
    }

    assert_eq!(nummer, ["V1", "V1", "V2", "V2"]);
}

#[tokio::test]
async fn ett_verifikat_i_året_efter_det_senaste_skapar_året_och_det_loggas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    verifikat::bokför(&db, &klocka(), &anna, id, &försäljning("2027-03-01"))
        .await
        .unwrap();

    let företaget = foretag::företaget(&db, &anna, id).await.unwrap();
    let år = &företaget.räkenskapsår[1];
    assert_eq!((år.start, år.slut), (date(2027, 1, 1), date(2027, 12, 31)));
    assert_eq!(år.perioder.len(), 12);
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let typer: Vec<_> = historik.iter().map(|p| p.typ.as_str()).collect();
    assert_eq!(
        typer,
        [
            "företag skapat",
            "åtkomst given",
            "räkenskapsår skapat",
            "verifikat bokfört"
        ]
    );
    assert_eq!(historik[2].beskrivning["start"], "2027-01-01");
    assert_eq!(historik[2].beskrivning["slut"], "2027-12-31");
}

#[tokio::test]
async fn ett_datum_längre_fram_än_året_efter_det_senaste_ger_saknat_räkenskapsår() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    let fel = verifikat::bokför(&db, &klocka(), &anna, id, &försäljning("2028-01-01"))
        .await
        .unwrap_err();

    let VerifikatFel::Spärrat(spärrar) = fel else {
        panic!("{fel:?}")
    };
    assert_eq!(spärrar[0].kod, "saknat_rakenskapsar");
    let företaget = foretag::företaget(&db, &anna, id).await.unwrap();
    assert_eq!(företaget.räkenskapsår.len(), 1);
}

#[tokio::test]
async fn bokföringen_loggas_med_registreringstidpunkt_användare_och_kanal() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;

    let b = verifikat::bokför(&db, &klocka(), &anna, id, &försäljning("2026-03-14"))
        .await
        .unwrap();

    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = historik.last().unwrap();
    assert_eq!(post.typ, "verifikat bokfört");
    assert_eq!(post.objekt, "V1");
    assert_eq!(post.tid, NU.parse().unwrap());
    assert_eq!(post.aktör, anna);
    assert_eq!(post.aktör.kanal, Kanal::Webb);
    assert_eq!(post.beskrivning["datum"], "2026-03-14");
    assert_eq!(post.beskrivning["summa_öre"], 125_000);
    assert_eq!(post.beskrivning["rader"][1]["konto"], 3001);
    // Dokumentets historik visar samma post.
    let v = verifikat::verifikatet(&db, &anna, id, b.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.historik.len(), 1);
    assert_eq!(&v.historik[0].post, post);
    assert_eq!(v.historik[0].namn, "anna@example.se");
}

#[tokio::test]
async fn bara_en_användare_med_åtkomst_kan_bokföra_och_läsa_verifikaten() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let b = verifikat::bokför(&db, &klocka(), &anna, id, &försäljning("2026-03-14"))
        .await
        .unwrap();

    let fel = verifikat::bokför(&db, &klocka(), &bo, id, &försäljning("2026-03-14")).await;
    assert!(matches!(
        fel,
        Err(VerifikatFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    let cli = Aktör::cli();
    let fel = verifikat::bokför(&db, &klocka(), &cli, id, &försäljning("2026-03-14")).await;
    assert!(matches!(
        fel,
        Err(VerifikatFel::Åtkomst(ÅtkomstFel::KanalenFårInte))
    ));
    assert!(matches!(
        verifikat::verifikatet(&db, &bo, id, b.id).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
    assert!(matches!(
        verifikat::lista(&db, &bo, id, Urval::Alla).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
}
