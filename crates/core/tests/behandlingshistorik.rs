//! Behandlingshistoriken är append-only också förbi `core` (H3).

use jiff::civil::date;
use maya_core::aktor::{Aktör, Kanal};
use maya_core::atkomst::{self, Inbjudan};
use maya_core::behandlingshistorik::{Aktörsval, Händelse, Urval};
use maya_core::foretag::{self, FöretagsId};
use maya_core::inloggning::{self, NyPasskey};
use maya_core::{
    Databas, Klocka, behandlingshistorik, export, kontoplan, leverantor, saldo, sie4, testhjalp,
    underlag, verifikat,
};
use maya_domain::foretag::{Byte, NyttFöretag, Uppgifter};
use maya_domain::ingaende_balans::{NyIngåendeBalans, NySaldorad};
use maya_domain::kontoplan::{Kontonummer, NyttKonto};
use maya_domain::leverantor::NyLeverantör;
use maya_domain::verifikat::{NyRad, NyttVerifikat};

#[tokio::test]
async fn behandlingshistoriken_går_inte_att_ändra_eller_radera() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("maya.db");
    let db = Databas::öppna(&path).await.unwrap();
    let anna = testhjalp::användare(&db, "anna@example.se").await;
    let id = foretag::lägg_upp(
        &db,
        &Klocka::system(),
        &anna,
        &NyttFöretag {
            orgnr: "556036-0793".into(),
            namn: "Exempel AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "manad".into(),
        },
    )
    .await
    .unwrap();
    db.stäng().await;

    // Förbi `core`, som en angripare eller ett felaktigt verktyg med databasfilen.
    let ändra = kör_sql(&path, "UPDATE behandlingshistorik SET objekt = 'annat'").await;
    let radera = kör_sql(&path, "DELETE FROM behandlingshistorik").await;

    assert!(ändra.unwrap_err().contains("går inte att ändra"));
    assert!(radera.unwrap_err().contains("går inte att radera"));
    let db = Databas::öppna(&path).await.unwrap();
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    assert_eq!(historik.len(), 2);
    assert_eq!(historik[0].objekt, "556036-0793");
}

async fn kör_sql(path: &std::path::Path, sql: &'static str) -> Result<(), String> {
    use sqlx::Connection;
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let resultat = sqlx::query(sql).execute(&mut conn).await;
    conn.close().await.unwrap();
    resultat.map(|_| ()).map_err(|e| e.to_string())
}

fn klocka(tid: &str) -> Klocka {
    Klocka::fast(tid.parse().unwrap())
}

fn nr(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

fn byte(gäller_från: &str, val: &str) -> Byte {
    Byte {
        gäller_från: gäller_från.into(),
        val: val.into(),
    }
}

/// Kör varje användningsfall i etapp 1 som loggar, en gång var, på tiden `tid`.
async fn alla_ändringar(db: &Databas, anna: &Aktör, id: FöretagsId, tid: &str) {
    let k = klocka(tid);
    foretag::ändra_uppgifter(
        db,
        &k,
        anna,
        id,
        &Uppgifter {
            namn: "Exempel Handel AB".into(),
            ..Uppgifter::default()
        },
    )
    .await
    .unwrap();
    foretag::ändra_första_årets_slut(db, &k, anna, id, "2027-06-30")
        .await
        .unwrap();
    foretag::byt_bokföringsmetod(db, &k, anna, id, &byte("2027-07-01", "kontant"))
        .await
        .unwrap();
    foretag::byt_momsperiod(db, &k, anna, id, &byte("2026-04-01", "manad"))
        .await
        .unwrap();
    saldo::ändra_ingående_balans(
        db,
        &k,
        anna,
        id,
        date(2026, 1, 1),
        &NyIngåendeBalans {
            rader: vec![
                NySaldorad {
                    konto: "1930".into(),
                    debet: "1 000".into(),
                    kredit: String::new(),
                },
                NySaldorad {
                    konto: "2081".into(),
                    debet: String::new(),
                    kredit: "1 000".into(),
                },
            ],
        },
    )
    .await
    .unwrap();
    let eget = kontoplan::skapa_eget(
        db,
        &k,
        anna,
        id,
        &NyttKonto {
            nummer: "1939".into(),
            namn: "Sparkonto".into(),
        },
    )
    .await
    .unwrap();
    kontoplan::byt_namn(db, &k, anna, id, eget, "Sparkonto Swedbank")
        .await
        .unwrap();
    kontoplan::inaktivera(db, &k, anna, id, eget).await.unwrap();
    kontoplan::aktivera(db, &k, anna, id, eget).await.unwrap();
    kontoplan::bas_tillägg(db, &k, "BAS 2027", &[(nr(6542), "Molntjänster")])
        .await
        .unwrap();
    let rader = |belopp: &str| {
        vec![
            NyRad {
                konto: "1930".into(),
                debet: belopp.into(),
                kredit: String::new(),
                text: String::new(),
            },
            NyRad {
                konto: "3001".into(),
                debet: String::new(),
                kredit: belopp.into(),
                text: String::new(),
            },
        ]
    };
    let mut bokförda = Vec::new();
    for (datum, belopp) in [("2026-03-14", "125"), ("2027-08-01", "1 234,50")] {
        // Det andra verifikatet är det första i nästa år och skapar året.
        let bokfört = verifikat::bokför(
            db,
            &k,
            anna,
            id,
            &NyttVerifikat {
                datum: datum.into(),
                beskrivning: format!("Kontantförsäljning {datum}"),
                rader: rader(belopp),
            },
        )
        .await
        .unwrap();
        bokförda.push(bokfört);
    }
    verifikat::rätta(
        db,
        &k,
        anna,
        id,
        bokförda[0].id,
        "2026-03-20",
        "fel belopp",
        verifikat::Rättelseval::BaraRättelse,
    )
    .await
    .unwrap();
    let kvitto = underlag::ladda_upp(db, &k, anna, id, "kvitto.pdf", "", b"%PDF-1.7 kvitto")
        .await
        .unwrap();
    underlag::komplettera(db, &k, anna, id, bokförda[0].id, kvitto.id)
        .await
        .unwrap();
    let fel = underlag::ladda_upp(db, &k, anna, id, "fel.pdf", "", b"%PDF-1.7 fel")
        .await
        .unwrap();
    underlag::ta_bort(db, &k, anna, id, fel.id).await.unwrap();
    let utkast = verifikat::spara_utkast(
        db,
        &k,
        anna,
        id,
        None,
        &NyttVerifikat {
            datum: "2026-05-02".into(),
            beskrivning: "Halvfärdigt".into(),
            rader: Vec::new(),
        },
    )
    .await
    .unwrap();
    verifikat::radera_utkast(db, &k, anna, id, utkast)
        .await
        .unwrap();
    // Bo bjuds in, registrerar sig och förlorar åtkomsten igen.
    let inloggad = testhjalp::inloggad(db, &k, anna).await;
    let Inbjudan::Länk(länk) = atkomst::bjud_in(db, &k, &inloggad, id, "bo@example.se")
        .await
        .unwrap()
    else {
        panic!("väntade en länk");
    };
    let session = inloggning::slutför_registrering(
        db,
        &k,
        länk.token(),
        NyPasskey {
            credential_id: b"bo".to_vec(),
            namn: "Telefon".into(),
            passkey: "{}".into(),
        },
        None,
    )
    .await
    .unwrap();
    let bo = inloggning::session(db, &k, session.token())
        .await
        .unwrap()
        .unwrap();
    inloggning::avaktivera_användare(db, &k, &Aktör::cli(), "bo@example.se")
        .await
        .unwrap();
    atkomst::ta_åtkomst(db, &k, &inloggad, id, bo.aktör.användare.unwrap())
        .await
        .unwrap();
    sie4::exportera(db, &k, anna, id, date(2026, 1, 1))
        .await
        .unwrap();
    // Driften återställer installationen ur nattens backup.
    testhjalp::logga_återställning(db, &k, "2026-03-01T02:00:00Z".parse().unwrap()).await;
    // En ny Maya-version försöker en migrering som misslyckas, och sedan en som går igenom.
    testhjalp::misslyckad_migrering(
        db,
        &k,
        "0.5.0",
        &[(20270101000000, "exempel")],
        "no such table: finns_inte",
    )
    .await;
    testhjalp::starta_om(
        db,
        &k,
        "0.4.0",
        "0.5.0",
        &[(20270101000000, "exempel")],
        Some("maya-20260301T090000Z"),
    )
    .await;
}

/// Kör varje användningsfall i etapp 2 som loggar, en gång var, på tiden `tid`. Bara testet
/// som kräver en post per typ använder den; CSV-exportens golden-fil följer `alla_ändringar`.
/// Lägg till anrop sist, under `// #<ticket>`.
async fn fakturaändringar(db: &Databas, anna: &Aktör, id: FöretagsId, tid: &str) {
    let k = klocka(tid);
    // #144 Leverantörsregister
    let leverantör = NyLeverantör {
        namn: "Kontorsvaror AB".into(),
        standardkonto: "5410".into(),
        ..NyLeverantör::default()
    };
    let l = leverantor::skapa(db, &k, anna, id, &leverantör)
        .await
        .unwrap();
    leverantor::ändra(
        db,
        &k,
        anna,
        id,
        l,
        &NyLeverantör {
            postort: "Göteborg".into(),
            ..leverantör
        },
    )
    .await
    .unwrap();
    leverantor::arkivera(db, &k, anna, id, l).await.unwrap();
    leverantor::radera(db, &k, anna, id, l).await.unwrap();
}

#[tokio::test]
async fn varje_typ_som_har_en_kodväg_ger_en_post() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;

    alla_ändringar(&db, &anna, id, "2026-03-01T09:00:00Z").await;
    fakturaändringar(&db, &anna, id, "2026-03-01T09:00:00Z").await;

    let historik = behandlingshistorik::historik(&db, &anna, id, &Urval::default())
        .await
        .unwrap();
    let mut typer: Vec<&str> = historik.poster.iter().map(|p| p.typ.as_str()).collect();
    typer.sort();
    typer.dedup();
    let mut väntade: Vec<&str> = Händelse::ALLA.iter().map(|h| h.namn()).collect();
    väntade.sort();
    assert_eq!(typer, väntade);
    // BAS-tillägget, migreringen och versionsbytet gör Maya självt; resten gör Anna i webben.
    for post in &historik.poster {
        let maya = matches!(
            post.typ.as_str(),
            "BAS-tillägg" | "migrering körd" | "migrering misslyckad" | "programversion bytt"
        );
        assert_eq!(post.aktör.kanal == Kanal::Maya, maya, "{post:?}");
    }
}

#[tokio::test]
async fn csv_exporten_är_utf8_med_bom_semikolon_och_decimalkomma() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag_upplagt(&db, "2026-01-10T08:00:00Z").await;
    alla_ändringar(&db, &anna, id, "2026-03-02T09:15:00Z").await;

    let csv = export::behandlingshistorik(
        &db,
        &klocka("2026-03-02T09:15:00Z"),
        &anna,
        id,
        &Urval::default(),
    )
    .await
    .unwrap();

    assert_eq!(csv.filnamn, "behandlingshistorik_5560360793_20260302.csv");
    let väntad = include_bytes!("golden/behandlingshistorik.csv");
    assert_eq!(
        String::from_utf8_lossy(&csv.innehåll),
        String::from_utf8_lossy(väntad)
    );
    assert_eq!(csv.innehåll, väntad);
}

#[tokio::test]
async fn csv_exporten_tar_bara_med_urvalet_och_citerar_fält_med_semikolon() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag_upplagt(&db, "2026-01-10T08:00:00Z").await;
    kontoplan::skapa_eget(
        &db,
        &klocka("2026-02-01T08:00:00Z"),
        &anna,
        id,
        &NyttKonto {
            nummer: "1939".into(),
            namn: "Spar; \"buffert\"".into(),
        },
    )
    .await
    .unwrap();

    let urval = Urval {
        typ: Some("konto skapat".into()),
        ..Urval::default()
    };
    let csv = export::behandlingshistorik(&db, &klocka("2026-02-01T08:00:00Z"), &anna, id, &urval)
        .await
        .unwrap();

    let text = String::from_utf8(csv.innehåll).unwrap();
    let rader: Vec<&str> = text.trim_start_matches('\u{feff}').split("\r\n").collect();
    assert_eq!(
        rader,
        [
            "Tidpunkt;Aktör;Kanal;Typ;Objekt;Belopp;Beskrivning",
            "2026-02-01 09:00:00;anna@example.se;webb;konto skapat;1939;;\"Eget konto ”Spar; \"\"buffert\"\"”\"",
            "",
        ]
    );
}

#[tokio::test]
async fn en_typ_utan_egen_text_visas_med_sina_fält() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag_upplagt(&db, "2026-01-10T08:00:00Z").await;
    // En typ som ännu saknar kodväg, som en senare version skriver den.
    testhjalp::kör_sql(
        &db,
        r#"INSERT INTO behandlingshistorik (foretag_id, tid, kanal, typ, objekt, beskrivning)
           SELECT id, '2026-02-01T08:00:00Z', 'maya', 'årsarkiv skapat', '2025-01-01 – 2025-12-31',
                  '{"från":"0.1.0","till":"0.2.0","migreringar":{"antal":2}}'
           FROM foretag"#,
    )
    .await
    .unwrap();

    let historik = behandlingshistorik::historik(&db, &anna, id, &Urval::default())
        .await
        .unwrap();

    assert!(historik.typer.contains(&"årsarkiv skapat".to_string()));
    let post = historik.poster.last().unwrap();
    assert_eq!(post.aktörsnamn(), "Maya");
    assert_eq!(
        post.text(),
        "från: 0.1.0; migreringar.antal: 2; till: 0.2.0"
    );
}

/// Exempel AB, upplagt av Anna på tiden `tid`.
async fn företag_upplagt(db: &Databas, tid: &str) -> (Aktör, FöretagsId) {
    let anna = testhjalp::användare(db, "anna@example.se").await;
    let id = foretag::lägg_upp(
        db,
        &klocka(tid),
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

async fn typer(db: &Databas, anna: &Aktör, id: FöretagsId, urval: Urval) -> Vec<String> {
    behandlingshistorik::historik(db, anna, id, &urval)
        .await
        .unwrap()
        .poster
        .into_iter()
        .map(|p| p.typ)
        .collect()
}

#[tokio::test]
async fn filter_på_typ_aktör_och_tid_ger_rätt_urval() {
    let db = Databas::tillfällig().await;
    let (anna, id) = företag_upplagt(&db, "2026-01-10T08:00:00Z").await;
    // 00:30 den 2 mars i Stockholm.
    alla_ändringar(&db, &anna, id, "2026-03-01T23:30:00Z").await;
    kontoplan::byt_namn(
        &db,
        &klocka("2026-04-01T08:00:00Z"),
        &anna,
        id,
        nr(1939),
        "Sparkonto Nordea",
    )
    .await
    .unwrap();

    let historik = behandlingshistorik::historik(&db, &anna, id, &Urval::default())
        .await
        .unwrap();
    assert_eq!(historik.poster.len(), 33);
    let namn: Vec<&str> = historik.aktörer.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(namn, ["anna@example.se", "bo@example.se", "cli", "Maya"]);
    assert_eq!(
        historik.aktörer[0].0,
        Aktörsval::Användare(anna.användare.unwrap())
    );
    assert_eq!(historik.aktörer[2].0, Aktörsval::Kanal(Kanal::Cli));
    assert_eq!(historik.aktörer[3].0, Aktörsval::Kanal(Kanal::Maya));
    assert!(historik.typer.contains(&"konto namnbytt".to_string()));

    let typ = |t: &str| Urval {
        typ: Some(t.into()),
        ..Urval::default()
    };
    assert_eq!(
        typer(&db, &anna, id, typ("konto namnbytt")).await,
        ["konto namnbytt", "konto namnbytt"]
    );
    let maya = Urval {
        aktör: Some(Aktörsval::Kanal(Kanal::Maya)),
        ..Urval::default()
    };
    assert_eq!(
        typer(&db, &anna, id, maya).await,
        [
            "BAS-tillägg",
            "migrering misslyckad",
            "migrering körd",
            "programversion bytt"
        ]
    );
    let anna_urval = Urval {
        aktör: Some(Aktörsval::Användare(anna.användare.unwrap())),
        ..Urval::default()
    };
    assert_eq!(typer(&db, &anna, id, anna_urval).await.len(), 25);
    let bo = Urval {
        aktör: Some(historik.aktörer[1].0),
        ..Urval::default()
    };
    assert_eq!(
        typer(&db, &anna, id, bo).await,
        ["inbjudan använd", "åtkomst given"]
    );

    // Dagarna räknas i Stockholm och tar med båda ändarna.
    let tid = |från: Option<_>, till: Option<_>| Urval {
        från,
        till,
        ..Urval::default()
    };
    assert_eq!(
        typer(&db, &anna, id, tid(None, Some(date(2026, 3, 1)))).await,
        ["företag skapat", "åtkomst given"]
    );
    assert_eq!(
        typer(
            &db,
            &anna,
            id,
            tid(Some(date(2026, 3, 2)), Some(date(2026, 3, 2)))
        )
        .await
        .len(),
        30
    );
    assert_eq!(
        typer(&db, &anna, id, tid(Some(date(2026, 4, 1)), None)).await,
        ["konto namnbytt"]
    );
    let allt = Urval {
        typ: Some("konto namnbytt".into()),
        aktör: Some(Aktörsval::Användare(anna.användare.unwrap())),
        från: Some(date(2026, 3, 2)),
        till: Some(date(2026, 3, 31)),
    };
    assert_eq!(typer(&db, &anna, id, allt).await, ["konto namnbytt"]);
}
