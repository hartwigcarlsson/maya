//! Resultat- och balansräkningen genom `core`: siffrorna ur bokförda verifikat (aldrig utkast),
//! urvalet (period, jämförelse, nivå), panelen med verifikaten och exporten.

use jiff::civil::{Date, date};
use maya_core::aktor::Aktör;
use maya_core::foretag::FöretagsId;
use maya_core::rapport::{self, Urval};
use maya_core::{Databas, Klocka, TillfälligDatabas, saldo, testhjalp, utskrift, verifikat};
use maya_domain::belopp::Saldo;
use maya_domain::ingaende_balans::{NyIngåendeBalans, NySaldorad};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rapport::{
    Nivå, Rad, Radslag, Rapport, Rapporttyp, Visning, ÅRETS_RESULTAT_EJ_BOKFÖRT,
};
use maya_domain::verifikat::{NyRad, NyttVerifikat};
use maya_pdf::testhjalp::{innehåller, sidor, är_pdfa_2b};

fn urval(typ: Rapporttyp, till_och_med: Date) -> Urval {
    Urval {
        typ,
        till_och_med,
        visning: Visning::Båda,
        jämförelse: false,
        nivå: Nivå::Konton,
    }
}

fn rad<'a>(r: &'a Rapport, text: &str) -> &'a Rad {
    r.rader
        .iter()
        .find(|rad| rad.text == text)
        .unwrap_or_else(|| panic!("raden {text:?} saknas i {:#?}", r.rader))
}

fn konto(r: &Rapport, nr: u16) -> &Rad {
    r.rader
        .iter()
        .find(|rad| rad.konto.map(|k| k.get()) == Some(nr))
        .unwrap_or_else(|| panic!("kontot {nr} saknas i {:#?}", r.rader))
}

fn kr(kronor: i64) -> Option<i64> {
    Some(kronor * 100)
}

/// Ett utkast med rader som skulle synas om utkast räknades.
async fn utkast(db: &Databas, aktör: &Aktör, företag: FöretagsId, datum: &str) {
    let rad = |konto: &str, debet: &str, kredit: &str| NyRad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
        text: String::new(),
    };
    verifikat::spara_utkast(
        db,
        &Klocka::system(),
        aktör,
        företag,
        None,
        &NyttVerifikat {
            datum: datum.into(),
            beskrivning: "Utkast som inte räknas".into(),
            rader: vec![rad("1930", "99 999", ""), rad("3001", "", "99 999")],
        },
    )
    .await
    .expect("utkastet sparas");
}

/// Försäljning i januari och mars, hyra i mars och ett utkast i mars.
async fn exempel() -> (TillfälligDatabas, Aktör, FöretagsId) {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-01-10",
        &[(1930, 1_000_000, 0), (3001, 0, 1_000_000)],
    )
    .await;
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-05",
        &[(1510, 500_000, 0), (3001, 0, 500_000)],
    )
    .await;
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-20",
        &[(5010, 300_000, 0), (1930, 0, 300_000)],
    )
    .await;
    utkast(&db, &anna, företag, "2026-03-25").await;
    (db, anna, företag)
}

#[tokio::test]
async fn resultaträkningen_räknar_bokfört_per_månad_och_ackumulerat_men_aldrig_utkast() {
    let (db, anna, företag) = exempel().await;

    let r = rapport::rapport(
        &db,
        &anna,
        företag,
        date(2026, 1, 1),
        &urval(Rapporttyp::Resultaträkning, date(2026, 3, 31)),
    )
    .await
    .unwrap();

    // Kolumnerna: mars 2026 och januari–mars 2026. Intäkter med plus, kostnader med minus.
    assert_eq!(r.kolumner.len(), 2);
    assert_eq!(konto(&r, 3001).värden, vec![kr(5_000), kr(15_000)]);
    assert_eq!(konto(&r, 5010).värden, vec![kr(-3_000), kr(-3_000)]);
    assert_eq!(rad(&r, "Nettoomsättning").ink2.as_deref(), Some("3.1"));
    assert_eq!(
        rad(&r, "Rörelseresultat").värden,
        vec![kr(2_000), kr(12_000)]
    );
    assert_eq!(
        rad(&r, "Årets resultat").värden,
        vec![kr(2_000), kr(12_000)]
    );
    assert_eq!(rad(&r, "Årets resultat").slag, Radslag::Slutsumma);
}

/// Exemplet med en inskriven ingående balans för 2026 och en försäljning i februari 2027, som
/// lägger upp 2027 med en preliminär ingående balans.
async fn två_år() -> (TillfälligDatabas, Aktör, FöretagsId) {
    let (db, anna, företag) = exempel().await;
    let rad = |konto: &str, debet: &str, kredit: &str| NySaldorad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
    };
    saldo::ändra_ingående_balans(
        &db,
        &Klocka::system(),
        &anna,
        företag,
        date(2026, 1, 1),
        &NyIngåendeBalans {
            rader: vec![rad("1930", "50 000", ""), rad("2081", "", "50 000")],
        },
    )
    .await
    .unwrap();
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2027-02-01",
        &[(1930, 200_000, 0), (3001, 0, 200_000)],
    )
    .await;
    (db, anna, företag)
}

#[tokio::test]
async fn balansräkningen_går_jämnt_ut_med_årets_resultat_som_egen_rad() {
    let (db, anna, företag) = två_år().await;

    let r = rapport::rapport(
        &db,
        &anna,
        företag,
        date(2026, 1, 1),
        &urval(Rapporttyp::Balansräkning, date(2026, 12, 1)),
    )
    .await
    .unwrap();

    // Ingående balans, förändring och utgående balans.
    assert_eq!(r.kolumner.len(), 3);
    assert!(!r.preliminär);
    assert_eq!(
        konto(&r, 1930).värden,
        vec![kr(50_000), kr(7_000), kr(57_000)]
    );
    assert_eq!(konto(&r, 2081).värden, vec![kr(50_000), kr(0), kr(50_000)]);
    assert_eq!(
        rad(&r, "Kassa, bank och redovisningsmedel").ink2.as_deref(),
        Some("2.26")
    );
    let årets = rad(&r, ÅRETS_RESULTAT_EJ_BOKFÖRT);
    assert_eq!(årets.slag, Radslag::ÅretsResultat);
    assert_eq!(årets.värden, vec![kr(0), kr(12_000), kr(12_000)]);
    assert_eq!(
        rad(&r, "Summa tillgångar").värden,
        vec![kr(50_000), kr(12_000), kr(62_000)]
    );
    assert_eq!(
        rad(&r, "Summa eget kapital och skulder").värden,
        vec![kr(50_000), kr(12_000), kr(62_000)]
    );
    let kontroll = r.rader.last().unwrap();
    assert_eq!(kontroll.slag, Radslag::Kontroll);
    assert_eq!(kontroll.värden, vec![kr(0), kr(0), kr(0)]);
}

#[tokio::test]
async fn kontrollraden_är_noll_också_med_preliminär_ingående_balans() {
    let (db, anna, företag) = två_år().await;

    let r = rapport::rapport(
        &db,
        &anna,
        företag,
        date(2027, 1, 1),
        &Urval {
            jämförelse: true,
            ..urval(Rapporttyp::Balansräkning, date(2027, 2, 1))
        },
    )
    .await
    .unwrap();

    assert!(r.preliminär);
    assert_eq!(r.kolumner[0].underrubrik, "2027-01-01 · preliminär");
    // Fjolårets resultat står på 2099 i den preliminära ingående balansen.
    assert_eq!(konto(&r, 2099).värden[0], kr(12_000));
    assert_eq!(
        rad(&r, ÅRETS_RESULTAT_EJ_BOKFÖRT).värden,
        vec![kr(0), kr(2_000), kr(2_000), kr(10_000)]
    );
    // Jämförelsen är utgående balans vid samma tid föregående år: 2026-02-28.
    assert_eq!(r.kolumner[3].underrubrik, "2026-02-28");
    assert_eq!(
        konto(&r, 1930).värden,
        vec![kr(57_000), kr(2_000), kr(59_000), kr(60_000)]
    );
    let kontroll = r.rader.last().unwrap();
    assert_eq!(kontroll.slag, Radslag::Kontroll);
    assert_eq!(kontroll.värden, vec![kr(0); 4]);
}

#[tokio::test]
async fn visning_nivå_och_jämförelse_ändrar_urvalet() {
    let (db, anna, företag) = två_år().await;
    let hämta = |år: Date, urval: Urval| {
        let (db, anna) = (&db, &anna);
        async move {
            rapport::rapport(db, anna, företag, år, &urval)
                .await
                .unwrap()
        }
    };
    let rr = urval(Rapporttyp::Resultaträkning, date(2026, 3, 15));

    let månad = hämta(
        date(2026, 1, 1),
        Urval {
            visning: Visning::Månad,
            ..rr
        },
    )
    .await;
    assert_eq!(månad.kolumner.len(), 1);
    assert_eq!(månad.kolumner[0].rubrik, "Mars 2026");
    assert_eq!(konto(&månad, 3001).värden, vec![kr(5_000)]);

    let ack = hämta(
        date(2026, 1, 1),
        Urval {
            visning: Visning::Ackumulerat,
            ..rr
        },
    )
    .await;
    assert_eq!(ack.kolumner.len(), 1);
    assert_eq!(ack.kolumner[0].rubrik, "januari–mars 2026");
    assert_eq!(konto(&ack, 3001).värden, vec![kr(15_000)]);

    let rubriker = hämta(
        date(2026, 1, 1),
        Urval {
            nivå: Nivå::Rubriker,
            ..rr
        },
    )
    .await;
    assert!(
        rubriker
            .rader
            .iter()
            .all(|r| !matches!(r.slag, Radslag::Ink2Rad | Radslag::Konto))
    );
    assert_eq!(
        rad(&rubriker, "Rörelsens intäkter").värden,
        vec![kr(5_000), kr(15_000)]
    );
    // Exporten har alltid en rad per konto.
    assert_eq!(rubriker.konton.len(), 2);

    let ink2 = hämta(
        date(2026, 1, 1),
        Urval {
            nivå: Nivå::Ink2Rader,
            ..rr
        },
    )
    .await;
    assert!(ink2.rader.iter().any(|r| r.slag == Radslag::Ink2Rad));
    assert!(ink2.rader.iter().all(|r| r.slag != Radslag::Konto));
    assert_eq!(
        rad(&ink2, "Summa rörelsens intäkter").värden,
        vec![kr(5_000), kr(15_000)]
    );

    // Första året: jämförelsen finns inte.
    let första = hämta(
        date(2026, 1, 1),
        Urval {
            jämförelse: true,
            ..rr
        },
    )
    .await;
    assert_eq!(första.föregående, None);
    assert_eq!(första.kolumner.len(), 4);
    assert_eq!(första.kolumner[2].underrubrik, "finns inte i Maya");
    assert_eq!(konto(&första, 3001).värden[2..], [None, None]);

    // Andra året jämförs med samma period föregående år.
    let andra = hämta(
        date(2027, 1, 1),
        Urval {
            jämförelse: true,
            till_och_med: date(2027, 2, 1),
            ..rr
        },
    )
    .await;
    assert_eq!(
        andra
            .kolumner
            .iter()
            .map(|k| k.rubrik.as_str())
            .collect::<Vec<_>>(),
        [
            "Februari 2027",
            "januari–februari 2027",
            "Februari 2026",
            "januari–februari 2026"
        ]
    );
    assert_eq!(
        konto(&andra, 3001).värden,
        vec![kr(2_000), kr(2_000), kr(0), kr(10_000)]
    );
}

#[tokio::test]
async fn panelen_visar_kontots_verifikat_med_löpande_saldo() {
    let (db, anna, företag) = två_år().await;
    let år = date(2026, 1, 1);
    let nr = |n: u16| Kontonummer::new(n).unwrap();

    let br = urval(Rapporttyp::Balansräkning, date(2026, 3, 1));
    let utdrag = rapport::kontoutdrag(&db, &anna, företag, år, nr(1930), &br)
        .await
        .unwrap();
    assert_eq!(utdrag.kontonamn, "Företagskonto");
    assert_eq!((utdrag.från, utdrag.till), (år, date(2026, 3, 31)));
    assert_eq!(utdrag.ingående, Some(Saldo::från_öre(5_000_000)));
    let rader: Vec<_> = utdrag
        .rader
        .iter()
        .map(|r| {
            (
                r.nummer.to_string(),
                r.datum.to_string(),
                r.debet.öre(),
                r.kredit.öre(),
                r.saldo.öre(),
            )
        })
        .collect();
    // Utkastet den 25 mars räknas inte.
    assert_eq!(
        rader,
        [
            ("V1".into(), "2026-01-10".into(), 1_000_000, 0, 6_000_000),
            ("V3".into(), "2026-03-20".into(), 0, 300_000, 5_700_000),
        ]
    );
    assert_eq!(utdrag.utgående, Saldo::från_öre(5_700_000));
    let v1 = verifikat::lista(&db, &anna, företag, verifikat::Urval::Bokförda)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.datum == Some(date(2026, 1, 10)))
        .unwrap();
    assert_eq!(utdrag.rader[0].verifikat, v1.id);

    // Resultaträkningen som bara visar månaden: bara månadens verifikat, utan ingående balans.
    let mars = Urval {
        visning: Visning::Månad,
        ..urval(Rapporttyp::Resultaträkning, date(2026, 3, 1))
    };
    let utdrag = rapport::kontoutdrag(&db, &anna, företag, år, nr(3001), &mars)
        .await
        .unwrap();
    assert_eq!(utdrag.ingående, None);
    assert_eq!(utdrag.rader.len(), 1);
    assert_eq!(utdrag.rader[0].nummer.to_string(), "V2");
    assert_eq!(utdrag.utgående, Saldo::från_öre(-500_000));
}

#[tokio::test]
async fn csv_exporten_har_en_rad_per_konto_med_kolumnerna_som_visas() {
    let (db, anna, företag) = två_år().await;
    let år = date(2026, 1, 1);

    for (typ, förväntad) in [
        (
            Rapporttyp::Resultaträkning,
            include_str!("../fixtures/rapport/resultatrakning_5560360793_2026_202603.csv"),
        ),
        (
            Rapporttyp::Balansräkning,
            include_str!("../fixtures/rapport/balansrakning_5560360793_2026_202603.csv"),
        ),
    ] {
        // Nivån påverkar inte exporten: den har alltid en rad per konto.
        let urval = Urval {
            nivå: Nivå::Rubriker,
            ..urval(typ, date(2026, 3, 1))
        };
        let csv = rapport::csv(&db, &anna, företag, år, &urval).await.unwrap();

        assert_eq!(
            csv.filnamn,
            format!("{}_5560360793_2026_202603.csv", typ.kod())
        );
        let utan_bom = csv
            .innehåll
            .strip_prefix('\u{feff}')
            .expect("UTF-8 med BOM");
        assert_eq!(utan_bom, förväntad);
    }
}

#[tokio::test]
async fn pdf_exporten_är_pdfa_och_följer_nivå_och_kolumner() {
    let (db, anna, företag) = två_år().await;
    let klocka = Klocka::fast("2026-10-18T12:34:00Z".parse().unwrap());
    let år = date(2027, 1, 1);
    let br = Urval {
        jämförelse: true,
        ..urval(Rapporttyp::Balansräkning, date(2027, 2, 1))
    };

    let pdf = utskrift::rapport(&db, &klocka, &anna, företag, år, &br)
        .await
        .unwrap();

    assert_eq!(pdf.filnamn, "balansrakning_5560360793_2027_202702.pdf");
    är_pdfa_2b(&pdf.innehåll);
    let text = sidor(&pdf.innehåll).concat();
    for väntat in [
        "Balansräkning",
        "Exempel AB",
        "556036-0793",
        "januari–februari 2027",
        "Ingående balans är preliminär",
        "Föregående år",
        "2026-02-28",
        "Kassa, bank och redovisningsmedel",
        "1930",
        "Företagskonto",
        "59 000,00",
        "Årets resultat (ej bokfört på 2099)",
        "Tillgångar − eget kapital och skulder",
        "Utskrivet 2026-10-18 14:34 av anna@example.se",
    ] {
        assert!(innehåller(&text, väntat), "saknar {väntat:?} i {text}");
    }

    let rubriker = Urval {
        nivå: Nivå::Rubriker,
        visning: Visning::Månad,
        ..urval(Rapporttyp::Resultaträkning, date(2026, 3, 1))
    };
    let pdf = utskrift::rapport(&db, &klocka, &anna, företag, date(2026, 1, 1), &rubriker)
        .await
        .unwrap();
    är_pdfa_2b(&pdf.innehåll);
    let text = sidor(&pdf.innehåll).concat();
    assert!(innehåller(&text, "Rörelsens intäkter"), "{text}");
    assert!(innehåller(&text, "Mars 2026"), "{text}");
    assert!(!innehåller(&text, "Nettoomsättning"), "{text}");
    assert!(!innehåller(&text, "ackumulerat"), "{text}");
}

#[tokio::test]
async fn årets_resultat_räknas_utan_8999() {
    let (db, anna, företag) = exempel().await;
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-12-31",
        &[(8999, 1_200_000, 0), (2099, 0, 1_200_000)],
    )
    .await;
    let år = date(2026, 1, 1);
    let december = date(2026, 12, 1);
    let rr = rapport::rapport(
        &db,
        &anna,
        företag,
        år,
        &urval(Rapporttyp::Resultaträkning, december),
    )
    .await
    .unwrap();
    let br = rapport::rapport(
        &db,
        &anna,
        företag,
        år,
        &urval(Rapporttyp::Balansräkning, december),
    )
    .await
    .unwrap();

    assert_eq!(rad(&rr, "Årets resultat").värden, vec![kr(0), kr(12_000)]);
    assert!(
        rr.rader
            .iter()
            .all(|r| r.konto.map(|k| k.get()) != Some(8999))
    );
    // Ombokat på 2099: raden årets resultat försvinner och kontrollen är fortfarande 0.
    assert!(br.rader.iter().all(|r| r.slag != Radslag::ÅretsResultat));
    assert_eq!(konto(&br, 2099).värden, vec![kr(0), kr(12_000), kr(12_000)]);
    assert_eq!(br.rader.last().unwrap().värden, vec![kr(0); 3]);
}
