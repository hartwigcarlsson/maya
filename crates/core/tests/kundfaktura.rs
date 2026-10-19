//! Kundfakturor genom `core` mot en tillfällig databas: utkast, spärrarna vid utfärdande,
//! fakturaserien, OCR, PDF:en som underlag, verifikatet och behandlingshistoriken (#135).

use std::collections::BTreeSet;

use maya_core::atkomst::ÅtkomstFel;
use maya_core::kundfaktura::{self, KundfakturaFel, KundfakturaId, Status};
use maya_core::verifikat::{self, Flöde, Rättelseval, VerifikatFel};
use maya_core::{Databas, Klocka, behandlingshistorik, fakturauppgifter, foretag, kund, testhjalp};
use maya_core::{aktor::Aktör, foretag::FöretagsId};
use maya_domain::fakturauppgifter::Fakturauppgifter;
use maya_domain::kund::NyKund;
use maya_domain::kundfaktura::{NY, NyFakturarad, NyKundfaktura, UTFÄRDANDESPÄRRAR};
use maya_pdf::testhjalp::{innehåller, sidor, är_pdfa_2b};

const NU: &str = "2026-10-18T08:00:00Z";

fn klocka() -> Klocka {
    Klocka::fast(NU.parse().unwrap())
}

fn rad(beskrivning: &str, antal: &str, à_pris: &str, sats: &str, konto: &str) -> NyFakturarad {
    NyFakturarad {
        beskrivning: beskrivning.into(),
        antal: antal.into(),
        enhet: "st".into(),
        à_pris: à_pris.into(),
        rabatt: String::new(),
        momssats: sats.into(),
        konto: konto.into(),
    }
}

fn faktura(kund: kund::KundId) -> NyKundfaktura {
    NyKundfaktura {
        kund: kund.to_string(),
        fakturadatum: "2026-03-10".into(),
        er_referens: "Bo Ek".into(),
        rader: vec![
            rad("Konsulttimmar", "10", "950", "25", "3001"),
            rad("Handbok", "2", "200", "6", "3003"),
        ],
        ..Default::default()
    }
}

async fn utkast(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    formulär: &NyKundfaktura,
) -> KundfakturaId {
    kundfaktura::spara_utkast(db, &klocka(), anna, id, None, formulär)
        .await
        .unwrap()
}

async fn spärrkoder(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    f: KundfakturaId,
) -> Vec<&'static str> {
    match kundfaktura::utfärda(db, &klocka(), anna, id, f).await {
        Err(KundfakturaFel::Spärrat(spärrar)) => spärrar.into_iter().map(|s| s.kod).collect(),
        annat => panic!("{annat:?}"),
    }
}

#[tokio::test]
async fn ett_utkast_sparas_ändras_och_läses_tillbaka_som_formulär() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;

    let mut formulär = faktura(k);
    formulär.rader[0].antal = String::new();
    let f = utkast(&db, &anna, id, &formulär).await;

    let utkastet = kundfaktura::kundfakturan(&db, &anna, id, f).await.unwrap();
    assert_eq!(utkastet.status, Status::Utkast);
    assert_eq!(utkastet.kund, "Kund AB");
    assert_eq!(utkastet.formulär.rader.len(), 2);
    assert_eq!(utkastet.formulär.rader[0].antal, "");
    assert_eq!(utkastet.formulär.rader[1].à_pris, "200");
    assert!(utkastet.utfärdad.is_none());

    formulär.rader[0].antal = "12".into();
    formulär.rader.pop();
    kundfaktura::spara_utkast(&db, &klocka(), &anna, id, Some(f), &formulär)
        .await
        .unwrap();
    let utkastet = kundfaktura::kundfakturan(&db, &anna, id, f).await.unwrap();
    assert_eq!(utkastet.formulär.rader.len(), 1);
    assert_eq!(utkastet.formulär.rader[0].antal, "12");

    let lista = kundfaktura::lista(&db, &anna, id).await.unwrap();
    assert_eq!(lista.len(), 1);
    assert_eq!(lista[0].status, Status::Utkast);
    assert_eq!(lista[0].total, 1_425_000);
}

#[tokio::test]
async fn ett_utkast_som_inte_går_att_lagra_spärras() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let formulär = NyKundfaktura {
        fakturadatum: "i morgon".into(),
        ..Default::default()
    };
    let fel = kundfaktura::spara_utkast(&db, &klocka(), &anna, id, None, &formulär)
        .await
        .unwrap_err();
    let KundfakturaFel::Spärrat(spärrar) = fel else {
        panic!("{fel:?}")
    };
    assert_eq!(spärrar[0].kod, "fakturadatum_ogiltigt");
}

#[tokio::test]
async fn ett_utkast_raderas_och_raderingen_loggas_med_en_sammanfattning() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let f = utkast(&db, &anna, id, &faktura(k)).await;

    kundfaktura::radera_utkast(&db, &klocka(), &anna, id, f)
        .await
        .unwrap();

    assert!(kundfaktura::lista(&db, &anna, id).await.unwrap().is_empty());
    assert!(matches!(
        kundfaktura::kundfakturan(&db, &anna, id, f).await,
        Err(KundfakturaFel::FinnsInte)
    ));
    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let post = poster.last().unwrap();
    assert_eq!(post.typ, "fakturautkast raderat");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.beskrivning["sort"], "kundfaktura");
    assert_eq!(
        post.beskrivning["kundfaktura"],
        f.to_string().parse::<i64>().unwrap()
    );
    assert_eq!(post.beskrivning["kundnamn"], "Kund AB");
    assert_eq!(post.beskrivning["fakturadatum"], "2026-03-10");
    assert_eq!(post.beskrivning["summa_öre"], 1_229_900);
    assert_eq!(post.beskrivning["antal_rader"], 2);
}

#[tokio::test]
async fn utfärdandet_numrerar_arkiverar_pdf_en_bokför_och_loggar_i_en_transaktion() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let f = utkast(&db, &anna, id, &faktura(k)).await;

    let utfärdad = kundfaktura::utfärda(&db, &klocka(), &anna, id, f)
        .await
        .unwrap();
    assert_eq!(utfärdad.id, f);
    assert_eq!(utfärdad.nummer, 1);

    let faktura = kundfaktura::kundfakturan(&db, &anna, id, f).await.unwrap();
    assert_eq!(faktura.status, Status::Utfärdad);
    let u = faktura.utfärdad.unwrap();
    assert_eq!(u.nummer, 1);
    assert_eq!(u.referens, "1");
    assert_eq!(u.kund.namn, "Kund AB");
    assert_eq!(u.säljare.bankgiro, "5050-1055");
    assert_eq!(u.förfallodag.to_string(), "2026-04-09");
    assert_eq!(u.total, 1_229_900);
    assert_eq!(u.verifikat.nummer.unwrap().to_string(), "V1");
    assert_eq!(u.utfärdad.aktör, anna);
    assert_eq!(u.pdf.filnamn, "Kundfaktura_1.pdf");
    assert_eq!(u.pdf.sha256.len(), 64);
    assert_eq!(u.pdf.arkiverad.to_string(), NU);

    // Verifikatet följer automatkonteringen: 1510 mot intäkt per rad och moms per sats,
    // daterat på fakturadatumet, med PDF:en som underlag (VF5) och motparten i texten (VF2).
    let v = verifikat::verifikatet(&db, &anna, id, utfärdad.verifikat.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.datum.unwrap().to_string(), "2026-03-10");
    assert_eq!(v.beskrivning, "Kundfaktura 1, Kund AB");
    let rader: Vec<(u16, i64, i64)> = v
        .rader
        .iter()
        .map(|r| (r.konto.unwrap().get(), r.debet.öre(), r.kredit.öre()))
        .collect();
    assert_eq!(
        rader,
        [
            (1510, 1_229_900, 0),
            (3001, 0, 950_000),
            (3003, 0, 40_000),
            (2611, 0, 237_500),
            (2631, 0, 2_400),
        ]
    );
    assert_eq!(v.underlag.len(), 1);
    assert_eq!(v.underlag[0].id, u.pdf.underlag);
    assert!(!v.underlag[0].komplettering);
    assert_eq!(v.underlag[0].sha256, u.pdf.sha256);
    // Verifikatet länkar till fakturan.
    let flöde = v.flöde.as_ref().unwrap();
    assert_eq!(flöde.flöde, Flöde::Kundfaktura);
    assert_eq!(flöde.id.to_string(), f.to_string());
    assert_eq!(flöde.text, "kundfaktura 1 till Kund AB");

    // Behandlingshistoriken: underlaget, verifikatet och utfärdandet, med aktör och tid.
    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let typer: Vec<&str> = poster
        .iter()
        .rev()
        .take(3)
        .map(|p| p.typ.as_str())
        .collect();
    assert_eq!(
        typer,
        [
            "kundfaktura utfärdad",
            "verifikat bokfört",
            "underlag uppladdat"
        ]
    );
    let post = poster.last().unwrap();
    assert_eq!(post.objekt, "1");
    assert_eq!(post.aktör, anna);
    assert_eq!(post.tid.to_string(), NU);
    assert_eq!(post.beskrivning["nummer"], 1);
    assert_eq!(post.beskrivning["kund"]["namn"], "Kund AB");
    assert_eq!(post.beskrivning["summa_öre"], 1_229_900);
    assert_eq!(post.beskrivning["verifikat"]["nummer"], "V1");
    assert_eq!(post.beskrivning["pdf"]["sha256"], u.pdf.sha256.as_str());

    let lista = kundfaktura::lista(&db, &anna, id).await.unwrap();
    assert_eq!(lista[0].nummer, Some(1));
    assert_eq!(lista[0].status, Status::Utfärdad);
}

#[tokio::test]
async fn ett_verifikat_från_en_kundfaktura_rättas_i_fakturan_och_inte_med_rätta() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let u = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;

    let v = verifikat::verifikatet(&db, &anna, id, u.verifikat.id)
        .await
        .unwrap()
        .unwrap();
    let spärr = v.som_original().unwrap().kan_rättas().unwrap_err();
    assert_eq!(spärr.kod(), "fran_flode");

    let fel = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        u.verifikat.id,
        "2026-10-18",
        "",
        Rättelseval::BaraRättelse,
    )
    .await
    .unwrap_err();
    assert!(matches!(fel, VerifikatFel::InteRättbart(_)), "{fel:?}");
}

#[tokio::test]
async fn den_arkiverade_pdf_en_är_pdfa_med_fakturans_uppgifter_och_samma_efter_omstart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("maya.db");
    let db = Databas::öppna(&path).await.unwrap();
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let u = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    let fil = maya_core::underlag::filen(&db, &anna, id, u.underlag)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fil.mime, "application/pdf");
    är_pdfa_2b(&fil.innehåll);
    let text = sidor(&fil.innehåll).join("");
    for uppgift in [
        "Faktura 1",
        "Fakturadatum 2026-03-10",
        "Förfallodag 2026-04-09",
        "Exempel AB",
        "Org.nr 556036-0793",
        "Styrelsens säte Stockholm",
        "Momsreg.nr SE556036079301",
        "Godkänd för F-skatt",
        "Kund AB",
        "Storgatan 1",
        "Konsulttimmar",
        "Moms 25 % på 9 500,00",
        "Att betala 11 875,00 kr",
        "Bankgiro 5050-1055",
    ] {
        assert!(innehåller(&text, uppgift), "{uppgift} saknas i {text}");
    }
    let sha = kundfaktura::kundfakturan(&db, &anna, id, u.id)
        .await
        .unwrap()
        .utfärdad
        .unwrap()
        .pdf
        .sha256;
    db.stäng().await;

    let db = Databas::öppna(&path).await.unwrap();
    let igen = maya_core::underlag::filen(&db, &anna, id, u.underlag)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(igen.innehåll, fil.innehåll);
    let efter = kundfaktura::kundfakturan(&db, &anna, id, u.id)
        .await
        .unwrap()
        .utfärdad
        .unwrap()
        .pdf
        .sha256;
    assert_eq!(efter, sha);
    use sha2::Digest;
    assert_eq!(hex::encode(sha2::Sha256::digest(&igen.innehåll)), sha);
    db.stäng().await;
}

#[tokio::test]
async fn logotypen_kommer_med_på_den_utfärdade_pdf_en() {
    const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90\x77\x53\xde\x00\x00\x00\x0c\x49\x44\x41\x54\x78\x9c\x63\xf8\xdf\xc0\x00\x00\x04\x01\x01\x80\xc5\x2a\x18\x5d\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    fakturauppgifter::byt_logotyp(&db, &klocka(), &anna, id, "logga.png", PNG)
        .await
        .unwrap();
    let u = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    let fil = maya_core::underlag::filen(&db, &anna, id, u.underlag)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(maya_pdf::testhjalp::bilder(&fil.innehåll), 1);
}

#[tokio::test]
async fn fakturaserien_börjar_på_startnumret_och_är_obruten() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let mut första = faktura(k);
    första.startnummer = "1007".into();
    let a = utkast(&db, &anna, id, &första).await;
    let b = utkast(&db, &anna, id, &faktura(k)).await;
    assert_eq!(
        kundfaktura::förutsättningar(&db, &anna, id)
            .await
            .unwrap()
            .startnummer,
        1007
    );

    let nummer_b = kundfaktura::utfärda(&db, &klocka(), &anna, id, b)
        .await
        .unwrap()
        .nummer;
    let nummer_a = kundfaktura::utfärda(&db, &klocka(), &anna, id, a)
        .await
        .unwrap()
        .nummer;
    assert_eq!((nummer_b, nummer_a), (1007, 1008));

    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let ändrat = poster
        .iter()
        .find(|p| p.typ == "startnummer ändrat")
        .unwrap();
    assert_eq!(ändrat.beskrivning["från"], 1);
    assert_eq!(ändrat.beskrivning["till"], 1007);
}

#[tokio::test]
async fn startnumret_kan_inte_ändras_efter_första_utfärdandet() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    let k = testhjalp::kund(&db, &anna, id, "Kund 2 AB").await;
    let mut f = faktura(k);
    f.startnummer = "500".into();

    let fel = kundfaktura::spara_utkast(&db, &klocka(), &anna, id, None, &f)
        .await
        .unwrap_err();
    assert!(matches!(fel, KundfakturaFel::StartnummerLåst), "{fel:?}");
    // Schemat avvisar det också.
    let fel = testhjalp::kör_sql(&db, "UPDATE fakturaserie SET startnummer = 500")
        .await
        .unwrap_err();
    assert!(
        fel.to_string().contains("startnumret går inte att ändra"),
        "{fel}"
    );
}

#[tokio::test]
async fn samtidiga_utfärdanden_ger_en_obruten_serie_utan_dubbletter() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let mut utkasten = Vec::new();
    for _ in 0..6 {
        utkasten.push(utkast(&db, &anna, id, &faktura(k)).await);
    }

    let mut jobb = Vec::new();
    for f in utkasten {
        let db = db.clone();
        jobb.push(tokio::spawn(async move {
            kundfaktura::utfärda(&db, &klocka(), &anna, id, f)
                .await
                .unwrap()
                .nummer
        }));
    }
    let mut nummer = BTreeSet::new();
    for j in jobb {
        nummer.insert(j.await.unwrap());
    }
    assert_eq!(nummer, (1..=6).collect());
}

#[tokio::test]
async fn med_ocr_på_får_fakturan_en_ocr_referens() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    fakturauppgifter::ändra(
        &db,
        &klocka(),
        &anna,
        id,
        &Fakturauppgifter {
            bankgiro: "5050-1055".into(),
            ocr: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let mut f = faktura(k);
    f.startnummer = "1007".into();
    let f = utkast(&db, &anna, id, &f).await;
    kundfaktura::utfärda(&db, &klocka(), &anna, id, f)
        .await
        .unwrap();

    let u = kundfaktura::kundfakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .utfärdad
        .unwrap();
    assert_eq!(u.referens, "100768");
    assert!(u.säljare.ocr);
}

#[tokio::test]
async fn varje_spärr_vid_utfärdande_stoppar_utfärdandet_och_ändrar_ingenting() {
    let db = Databas::tillfällig().await;
    // Ett företag utan fakturauppgifter och en kund utan adress.
    let (anna, id) = testhjalp::företag(&db).await;
    let utan_adress = kund::skapa(
        &db,
        &klocka(),
        &anna,
        id,
        &maya_domain::kund::NyKund {
            namn: "Adresslös AB".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut alla = BTreeSet::new();
    let mut fall = vec![NyKundfaktura::default(), faktura(utan_adress)];
    let mut fel = faktura(utan_adress);
    fel.förfallodag = "2026-03-01".into();
    fel.rader = vec![
        rad("", "0", "", "25", ""),
        rad("x", "1", "1", "25", "3910"),
        rad("x", "1", "1", "25", "1930"),
        rad("x", "", "1", "25", "3005"),
    ];
    fall.push(fel);
    for (datum, förfallo) in [("2025-12-31", ""), ("2028-01-01", ""), ("10 mars", "")] {
        let mut f = faktura(utan_adress);
        f.fakturadatum = datum.into();
        f.förfallodag = förfallo.into();
        fall.push(f);
    }
    let mut noll = faktura(utan_adress);
    noll.rader = vec![NyFakturarad {
        rabatt: "100".into(),
        ..rad("x", "1", "1", "25", "3001")
    }];
    fall.push(noll);

    for formulär in &fall {
        // Ett utkast med ett fel som inte går att lagra sparas utan det och prövas här.
        let lagrat = match formulär.som_utkast() {
            Ok(_) => formulär.clone(),
            Err(_) => {
                let mut f = formulär.clone();
                f.fakturadatum = "2026-03-10".into();
                f
            }
        };
        let f = utkast(&db, &anna, id, &lagrat).await;
        alla.extend(spärrkoder(&db, &anna, id, f).await);
        let kvar = kundfaktura::kundfakturan(&db, &anna, id, f).await.unwrap();
        assert_eq!(kvar.status, Status::Utkast);
    }
    // Spärrarna som bara ett formulär kan ge (ogiltiga datum och tal) prövas när utkastet
    // sparas, med samma kod.
    for (fält, värde, kod) in [
        ("fakturadatum", "10 mars", "fakturadatum_ogiltigt"),
        ("förfallodag", "snart", "forfallodag_ogiltig"),
        ("startnummer", "noll", "startnummer_ogiltigt"),
        ("antal", "många", "rad_antal_ogiltigt"),
        ("à_pris", "-5", "rad_pris_ogiltigt"),
        ("momssats", "8", "rad_momssats_ogiltig"),
        ("rabatt", "200", "rad_rabatt_ogiltig"),
        ("ny_kund", "inte en adress", "ny_kund"),
    ] {
        let mut f = faktura(utan_adress);
        match fält {
            "fakturadatum" => f.fakturadatum = värde.into(),
            "förfallodag" => f.förfallodag = värde.into(),
            "startnummer" => f.startnummer = värde.into(),
            "antal" => f.rader[0].antal = värde.into(),
            "à_pris" => f.rader[0].à_pris = värde.into(),
            "rabatt" => f.rader[0].rabatt = värde.into(),
            "ny_kund" => {
                f.kund = NY.into();
                f.ny_kund = NyKund {
                    namn: "Ny AB".into(),
                    epost: värde.into(),
                    ..NyKund::default()
                };
            }
            _ => f.rader[0].momssats = värde.into(),
        }
        let KundfakturaFel::Spärrat(spärrar) =
            kundfaktura::spara_utkast(&db, &klocka(), &anna, id, None, &f)
                .await
                .unwrap_err()
        else {
            panic!("{fält}");
        };
        assert_eq!(spärrar[0].kod, kod);
        alla.insert(kod);
    }
    // Inget utfärdades: ingen faktura har ett nummer och inget verifikat är bokfört.
    let lista = kundfaktura::lista(&db, &anna, id).await.unwrap();
    assert!(lista.iter().all(|f| f.nummer.is_none()));
    assert!(
        verifikat::lista(&db, &anna, id, verifikat::Urval::Bokförda)
            .await
            .unwrap()
            .is_empty()
    );
    let dokumenterade: BTreeSet<&str> = UTFÄRDANDESPÄRRAR.iter().map(|(k, _)| *k).collect();
    assert_eq!(alla, dokumenterade);
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_skapa_eller_utfärda_en_kundfaktura() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let f = utkast(&db, &anna, id, &faktura(k)).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;

    assert!(matches!(
        kundfaktura::utfärda(&db, &klocka(), &bo, id, f).await,
        Err(KundfakturaFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        kundfaktura::spara_utkast(&db, &klocka(), &bo, id, None, &faktura(k)).await,
        Err(KundfakturaFel::Åtkomst(ÅtkomstFel::IngenÅtkomst))
    ));
    assert!(matches!(
        kundfaktura::lista(&db, &bo, id).await,
        Err(ÅtkomstFel::IngenÅtkomst)
    ));
    // Bara webben utfärdar.
    let cli = Aktör {
        kanal: maya_core::aktor::Kanal::Cli,
        ..anna
    };
    assert!(matches!(
        kundfaktura::utfärda(&db, &klocka(), &cli, id, f).await,
        Err(KundfakturaFel::Åtkomst(ÅtkomstFel::KanalenFårInte))
    ));
}

#[tokio::test]
async fn en_utfärdad_faktura_kan_inte_utfärdas_igen_eller_raderas_som_utkast() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let u = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    assert!(matches!(
        kundfaktura::utfärda(&db, &klocka(), &anna, id, u.id).await,
        Err(KundfakturaFel::IngetUtkast)
    ));
    assert!(matches!(
        kundfaktura::radera_utkast(&db, &klocka(), &anna, id, u.id).await,
        Err(KundfakturaFel::IngetUtkast)
    ));
    let k = testhjalp::kund(&db, &anna, id, "Kund 2 AB").await;
    assert!(matches!(
        kundfaktura::spara_utkast(&db, &klocka(), &anna, id, Some(u.id), &faktura(k)).await,
        Err(KundfakturaFel::IngetUtkast)
    ));
}

#[tokio::test]
async fn triggers_avvisar_ändring_och_radering_av_en_utfärdad_faktura() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;

    for sql in [
        "UPDATE kundfaktura SET nummer = 7",
        "UPDATE kundfaktura SET total = 1",
        "UPDATE kundfaktura SET status = 'utkast'",
        "UPDATE kundfaktura SET underlag_id = NULL",
        "DELETE FROM kundfaktura",
        "UPDATE kundfakturarad SET netto = 1",
        "DELETE FROM kundfakturarad",
        "INSERT INTO kundfakturarad (faktura_id, radnr, beskrivning, enhet, momssats)
         SELECT id, 9, 'x', '', 25 FROM kundfaktura",
    ] {
        let fel = testhjalp::kör_sql(&db, sql).await.unwrap_err();
        assert!(
            fel.to_string()
                .contains("en utfärdad kundfaktura går inte att"),
            "{sql}: {fel}"
        );
    }
}

#[tokio::test]
async fn schemat_avvisar_ett_nummer_som_inte_är_nästa_i_serien() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    let k = testhjalp::kund(&db, &anna, id, "Kund 2 AB").await;
    testhjalp::kundfakturautkast(&db, &anna, id, k, "2026-03-11").await;

    let fel = testhjalp::kör_sql(
        &db,
        "UPDATE kundfaktura SET status = 'utfardad', nummer = 3, referens = '3',
             kunduppgifter = '{}', saljaruppgifter = '{}', netto = 1, moms = 0, total = 1,
             underlag_id = (SELECT max(id) FROM underlag),
             verifikat_id = (SELECT max(id) FROM verifikat), forfallodag = fakturadatum,
             utfardad = 'x', utfardad_kanal = 'webb'
         WHERE status = 'utkast'",
    )
    .await
    .unwrap_err();
    assert!(
        fel.to_string().contains("nästa nummer i fakturaserien"),
        "{fel}"
    );
    let fel = testhjalp::kör_sql(
        &db,
        "INSERT INTO kundfaktura (foretag_id, status, nummer, skapad, skapad_kanal)
         SELECT foretag_id, 'utfardad', 2, 'x', 'webb' FROM kundfaktura LIMIT 1",
    )
    .await
    .unwrap_err();
    assert!(fel.to_string().contains("utkast"), "{fel}");
}

#[tokio::test]
async fn under_kontantmetoden_utfärdas_ingen_faktura_ännu() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let byte = maya_domain::foretag::Byte {
        gäller_från: "2027-01-01".into(),
        val: "kontant".into(),
    };
    foretag::byt_bokföringsmetod(&db, &klocka(), &anna, id, &byte)
        .await
        .unwrap();
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let mut f = faktura(k);
    f.fakturadatum = "2027-02-10".into();
    let f = utkast(&db, &anna, id, &f).await;
    assert!(matches!(
        kundfaktura::utfärda(&db, &klocka(), &anna, id, f).await,
        Err(KundfakturaFel::Kontantmetoden)
    ));
}

#[tokio::test]
async fn en_kund_med_fakturor_kan_arkiveras_men_inte_raderas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    utkast(&db, &anna, id, &faktura(k)).await;

    assert!(kund::har_fakturor(&db, &anna, id, k).await.unwrap());
    let fel = kund::radera(&db, &klocka(), &anna, id, k)
        .await
        .unwrap_err();
    assert!(matches!(fel, kund::KundFel::KanInteRaderas(_)), "{fel:?}");
    kund::arkivera(&db, &klocka(), &anna, id, k).await.unwrap();
}

#[tokio::test]
async fn ett_företag_med_bara_utkast_till_kundfakturor_kan_raderas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    utkast(&db, &anna, id, &faktura(k)).await;
    kundfaktura::spara_utkast(&db, &klocka(), &anna, id, None, &{
        let mut f = faktura(k);
        f.startnummer = "10".into();
        f
    })
    .await
    .unwrap();

    foretag::radera(&db, &klocka(), &anna, id).await.unwrap();
}

#[tokio::test]
async fn ett_företag_med_en_utfärdad_kundfaktura_kan_inte_raderas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    assert!(matches!(
        foretag::radera(&db, &klocka(), &anna, id).await,
        Err(foretag::FöretagsFel::HarRäkenskapsinformation)
    ));
}

#[tokio::test]
async fn förhandsvisningen_är_fakturan_som_den_blir_märkt_utkast_och_arkiveras_inte() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let f = utkast(&db, &anna, id, &faktura(k)).await;

    let pdf = kundfaktura::förhandsvisning(&db, &klocka(), &anna, id, f)
        .await
        .unwrap();
    är_pdfa_2b(&pdf.innehåll);
    let text = sidor(&pdf.innehåll).join("");
    assert!(innehåller(&text, "Faktura 1"));
    assert!(innehåller(&text, "UTKAST"));
    assert!(
        maya_core::underlag::lista(&db, &anna, id, maya_core::underlag::Urval::Alla)
            .await
            .unwrap()
            .is_empty()
    );
}

// #137 Ny kund från fakturautkastet

fn med_ny_kund(k: kund::KundId) -> NyKundfaktura {
    NyKundfaktura {
        kund: NY.into(),
        ny_kund: NyKund {
            namn: "Nya Kunden AB".into(),
            adress: "Lillgatan 2\n222 33 Malmö".into(),
            betalningsvillkor: "10".into(),
            ..NyKund::default()
        },
        ..faktura(k)
    }
}

async fn registret(db: &Databas, anna: &Aktör, id: FöretagsId) -> Vec<String> {
    kund::kunder(db, anna, id)
        .await
        .unwrap()
        .into_iter()
        .map(|k| k.namn)
        .collect()
}

#[tokio::test]
async fn en_ny_kund_från_utkastet_hamnar_i_kundregistret_först_när_fakturan_utfärdas() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let mut formulär = med_ny_kund(k);
    formulär.ny_kund.adress = String::new();
    let f = utkast(&db, &anna, id, &formulär).await;

    // Den nya kunden hör till utkastet och syns varken i registret eller i väljarna.
    assert_eq!(registret(&db, &anna, id).await, ["Kund AB"]);
    assert_eq!(
        kundfaktura::förutsättningar(&db, &anna, id)
            .await
            .unwrap()
            .kunder
            .len(),
        1
    );
    let sparat = kundfaktura::kundfakturan(&db, &anna, id, f).await.unwrap();
    assert_eq!(sparat.kund, "Nya Kunden AB");
    assert_eq!(sparat.formulär.kund, NY);
    assert_eq!(sparat.formulär.ny_kund.betalningsvillkor, "10");
    let rad = &kundfaktura::lista(&db, &anna, id).await.unwrap()[0];
    assert_eq!((rad.kund.as_str(), rad.ny_kund), ("Nya Kunden AB", true));

    // Ett utfärdande som spärras lämnar kunden ny.
    assert_eq!(spärrkoder(&db, &anna, id, f).await, ["kund_adress_saknas"]);
    assert_eq!(registret(&db, &anna, id).await, ["Kund AB"]);

    let mut formulär = sparat.formulär;
    formulär.ny_kund.adress = "Lillgatan 2\n222 33 Malmö".into();
    kundfaktura::spara_utkast(&db, &klocka(), &anna, id, Some(f), &formulär)
        .await
        .unwrap();
    kundfaktura::utfärda(&db, &klocka(), &anna, id, f)
        .await
        .unwrap();

    // Utfärdandet gjorde kunden verklig: i registret, på fakturan och i historiken.
    assert_eq!(
        registret(&db, &anna, id).await,
        ["Kund AB", "Nya Kunden AB"]
    );
    let ny = kund::kunder(&db, &anna, id)
        .await
        .unwrap()
        .into_iter()
        .find(|k| k.namn == "Nya Kunden AB")
        .unwrap();
    assert_eq!(ny.status, kund::Kundstatus::Aktiv);
    assert_eq!(ny.betalningsvillkor, 10);
    assert!(kund::har_fakturor(&db, &anna, id, ny.id).await.unwrap());
    let u = kundfaktura::kundfakturan(&db, &anna, id, f)
        .await
        .unwrap()
        .utfärdad
        .unwrap();
    assert_eq!(u.kund.id.to_string(), ny.id.to_string());
    assert_eq!(u.kund.namn, "Nya Kunden AB");
    assert_eq!(u.förfallodag.to_string(), "2026-03-20");
    assert!(!kundfaktura::lista(&db, &anna, id).await.unwrap()[0].ny_kund);
    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let skapad = poster
        .iter()
        .find(|p| p.typ == "kund skapad" && p.objekt == "Nya Kunden AB")
        .unwrap();
    assert_eq!(skapad.aktör, anna);
    assert_eq!(skapad.beskrivning["id"].to_string(), ny.id.to_string());
    // Kunden blev verklig i utfärdandets transaktion, före fakturans poster.
    let ordning: Vec<&str> = poster
        .iter()
        .rev()
        .take(4)
        .map(|p| p.typ.as_str())
        .collect();
    assert_eq!(
        ordning,
        [
            "kundfaktura utfärdad",
            "verifikat bokfört",
            "underlag uppladdat",
            "kund skapad"
        ]
    );
}

#[tokio::test]
async fn en_ny_kund_raderas_med_utkastet_loggat_med_maya_som_aktör() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let f = utkast(&db, &anna, id, &med_ny_kund(k)).await;
    // Ett utkast som byter till en kund i registret släpper den nya, som raderas.
    kundfaktura::spara_utkast(&db, &klocka(), &anna, id, Some(f), &faktura(k))
        .await
        .unwrap();
    kundfaktura::spara_utkast(&db, &klocka(), &anna, id, Some(f), &med_ny_kund(k))
        .await
        .unwrap();
    // Att spara igen skriver om samma nya kund i stället för att lägga upp en till.
    let mut ändrad = med_ny_kund(k);
    ändrad.ny_kund.epost = "faktura@nya.se".into();
    kundfaktura::spara_utkast(&db, &klocka(), &anna, id, Some(f), &ändrad)
        .await
        .unwrap();
    kundfaktura::radera_utkast(&db, &klocka(), &anna, id, f)
        .await
        .unwrap();

    let poster = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    let raderade: Vec<_> = poster.iter().filter(|p| p.typ == "kund raderad").collect();
    assert_eq!(raderade.len(), 2);
    assert!(raderade.iter().all(|p| p.aktör == Aktör::maya()));
    assert!(raderade.iter().all(|p| p.objekt == "Nya Kunden AB"));
    assert_eq!(raderade[1].beskrivning["status"], "ny");
    assert_eq!(raderade[1].beskrivning["epost"], "faktura@nya.se");
    let radering = poster
        .iter()
        .find(|p| p.typ == "fakturautkast raderat")
        .unwrap();
    assert_eq!(radering.aktör, anna);
    assert_eq!(radering.beskrivning["kundnamn"], "Nya Kunden AB");
    // Ingen kund skapades, och registret är som förut.
    assert!(
        !poster
            .iter()
            .any(|p| p.typ == "kund skapad" && p.objekt == "Nya Kunden AB")
    );
    assert_eq!(registret(&db, &anna, id).await, ["Kund AB"]);
}

#[tokio::test]
async fn en_ny_kund_som_inte_klarar_registrets_regler_spärrar_utkastet() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let k = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let mut formulär = med_ny_kund(k);
    formulär.ny_kund.epost = "inte en adress".into();
    match kundfaktura::spara_utkast(&db, &klocka(), &anna, id, None, &formulär).await {
        Err(KundfakturaFel::Spärrat(s)) => {
            assert_eq!(s.iter().map(|s| s.kod).collect::<Vec<_>>(), ["ny_kund"]);
        }
        annat => panic!("{annat:?}"),
    }
    assert!(kundfaktura::lista(&db, &anna, id).await.unwrap().is_empty());
}
