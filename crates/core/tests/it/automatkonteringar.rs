//! Tabellen över automatkonteringar i Systemdokumentationen är samma som konteringen (#155,
//! S2): varje flöde bokförs genom `core` mot en tillfällig databas, och varje rad i verifikatet
//! står på en sida och ett konto som tabellens rad anger, med nummer eller med sin roll. Varje
//! konto som raden anger med nummer bokförs också av något fall, så att tabellen inte nämner
//! ett konto som flödet inte använder.

use std::collections::{BTreeSet, HashMap};

use maya_core::aktor::Aktör;
use maya_core::betalning::{self, Faktura};
use maya_core::foretag::FöretagsId;
use maya_core::kundfaktura::{self, KundfakturaId};
use maya_core::leverantorsfaktura::{self, LeverantörsfakturaId};
use maya_core::verifikat::{self, Rättelseval, VerifikatId};
use maya_core::{
    Databas, Klocka, extern_kundfaktura, kontering, kundforlust, saldo, t2, testhjalp,
};
use maya_domain::automatkontering::{self, Automatkontering, Flöde, Roll};
use maya_domain::betalning::NyBetalning;
use maya_domain::kreditfaktura::NyKreditering;
use maya_domain::kundfaktura::{NyFakturarad, NyKundfaktura};
use maya_domain::kundforlust::NyKundförlust;
use maya_domain::leverantorsfaktura::{NyLeverantörsfaktura, NyRad};

/// Ett verifikats rader som (konto, debet, kredit) i öre, i kontoordning.
type Rader = Vec<(u16, i64, i64)>;

fn klocka() -> Klocka {
    Klocka::fast("2026-10-18T08:00:00Z".parse().unwrap())
}

/// Ett bokfört verifikat från ett flöde, med raderna i tabellen som ska förklara det: den
/// första är flödets egen, de andra står i samma verifikat (öresutjämning och kursdifferens i
/// betalningens, Rätta kontering med ett nytt utfärdande).
struct Fall {
    namn: &'static str,
    flöden: Vec<Flöde>,
    rader: Rader,
    /// Intäktskontona på fakturans rader.
    intäkt: Vec<u16>,
    /// Kostnadskontona på fakturans rader.
    kostnad: Vec<u16>,
    /// Verifikatet som flödet utgår från: originalet, förlusten eller registreringen.
    original: Rader,
}

impl Fall {
    fn nytt(namn: &'static str, flöden: &[Flöde], rader: Rader) -> Self {
        Fall {
            namn,
            flöden: flöden.to_vec(),
            rader,
            intäkt: Vec::new(),
            kostnad: Vec::new(),
            original: Vec::new(),
        }
    }

    fn intäkt(mut self, k: &[u16]) -> Self {
        self.intäkt = k.to_vec();
        self
    }

    fn kostnad(mut self, k: &[u16]) -> Self {
        self.kostnad = k.to_vec();
        self
    }

    fn original(mut self, r: Rader) -> Self {
        self.original = r;
        self
    }
}

/// Betalkontot i fallen.
const BETALKONTO: u16 = 1930;

async fn rader(db: &Databas, anna: &Aktör, id: FöretagsId, v: VerifikatId) -> Rader {
    let mut r: Rader = verifikat::verifikatet(db, anna, id, v)
        .await
        .unwrap()
        .unwrap()
        .rader
        .iter()
        .map(|r| (r.konto.unwrap().get(), r.debet.öre(), r.kredit.öre()))
        .collect();
    r.sort();
    r
}

fn vänd(r: &Rader) -> Rader {
    let mut v: Rader = r.iter().map(|&(k, d, c)| (k, c, d)).collect();
    v.sort();
    v
}

fn betalning(betaldag: &str, belopp: &str) -> NyBetalning {
    NyBetalning {
        betaldag: betaldag.into(),
        belopp: belopp.into(),
        konto: BETALKONTO.to_string(),
        ..Default::default()
    }
}

async fn betala(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    faktura: Faktura,
    b: &NyBetalning,
) -> betalning::Registrerad {
    betalning::registrera(db, &klocka(), anna, id, faktura, b)
        .await
        .unwrap()
}

async fn kreditera_hela(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    f: KundfakturaId,
) -> kundfaktura::Utfärdad {
    let utkast = kundfaktura::kreditera(
        db,
        &klocka(),
        anna,
        id,
        f,
        &NyKreditering {
            hela: true,
            antal: Vec::new(),
        },
    )
    .await
    .unwrap();
    kundfaktura::utfärda(db, &klocka(), anna, id, utkast)
        .await
        .unwrap()
}

/// Ett företag med `metod` och en utfärdad kundfaktura på 11 875 kr (3001 netto 9 500 kr, 2611
/// 2 375 kr) daterad 2026-03-10.
async fn kundfaktura(db: &Databas, metod: &str) -> (Aktör, FöretagsId, kundfaktura::Utfärdad) {
    let (anna, id) = testhjalp::fakturaföretag_med_metod(db, metod).await;
    let f = testhjalp::utfärdad_kundfaktura(db, &anna, id, "Kund AB", "2026-03-10").await;
    (anna, id, f)
}

async fn registrera(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    formulär: NyLeverantörsfaktura,
) -> (LeverantörsfakturaId, leverantorsfaktura::Registrerat) {
    let (f, _) = leverantorsfaktura::ladda_upp(
        db,
        &klocka(),
        anna,
        id,
        (
            &format!("{}.pdf", formulär.fakturanummer),
            "application/pdf",
            format!("%PDF-1.4 {}", formulär.fakturanummer).as_bytes(),
        ),
    )
    .await
    .unwrap();
    let r = leverantorsfaktura::registrera(db, &klocka(), anna, id, f, &formulär)
        .await
        .unwrap();
    (f, r)
}

fn rad(konto: &str, netto: &str, momssats: &str) -> NyRad {
    NyRad {
        konto: konto.into(),
        netto: netto.into(),
        momssats: momssats.into(),
        moms: String::new(),
    }
}

/// En leverantörsfaktura `nummer` från `leverantör` på 5410, fakturerad 2026-03-14.
fn leverantörsfaktura(
    leverantör: &str,
    nummer: &str,
    total: &str,
    omvänd: &str,
    originalvaluta: &str,
    rader: Vec<NyRad>,
) -> NyLeverantörsfaktura {
    NyLeverantörsfaktura {
        leverantör: leverantör.into(),
        fakturanummer: nummer.into(),
        fakturadatum: "2026-03-14".into(),
        förfallodag: "2026-04-13".into(),
        totalbelopp: total.into(),
        omvänd_skattskyldighet: omvänd.into(),
        originalvaluta: originalvaluta.into(),
        rader,
        ..Default::default()
    }
}

/// En vanlig leverantörsfaktura på 1 250 kr: 5410 netto 1 000 kr och 250 kr moms.
fn vanlig(leverantör: &str, nummer: &str, originalvaluta: &str) -> NyLeverantörsfaktura {
    leverantörsfaktura(
        leverantör,
        nummer,
        "1 250",
        "",
        originalvaluta,
        vec![rad("5410", "1 000", "25")],
    )
}

/// En kreditfaktura på 500 kr som hänvisar till `original`, avskriven som 5410 500 kr utan moms:
/// verifikatet speglar ändå registreringen, 5410 400 kr och 100 kr moms.
fn leverantörskredit(leverantör: &str, original: LeverantörsfakturaId) -> NyLeverantörsfaktura {
    NyLeverantörsfaktura {
        fakturadatum: "2026-05-02".into(),
        förfallodag: "2026-05-02".into(),
        krediterar: original.to_string(),
        ..leverantörsfaktura(
            leverantör,
            "K-1",
            "-500",
            "",
            "",
            vec![rad("5410", "-500", "0")],
        )
    }
}

// Etapp 1

async fn rättelsen(fall: &mut Vec<Fall>) {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let original = testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(1930, 125_000, 0), (3001, 0, 100_000), (2611, 0, 25_000)],
    )
    .await;
    let r = verifikat::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        original.id,
        "2026-03-15",
        "",
        Rättelseval::RegistreraOm,
    )
    .await
    .unwrap();
    let o = rader(&db, &anna, id, original.id).await;
    fall.push(
        Fall::nytt(
            "rättelse",
            &[Flöde::Rättelse],
            rader(&db, &anna, id, r.rättelse.id).await,
        )
        .original(o),
    );
    // Registrera om bokför inget: kopian är ett utkast.
    let kopia = verifikat::verifikatet(&db, &anna, id, r.kopia.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kopia.nummer, None, "registrera om");
    assert_eq!(
        verifikat::lista(&db, &anna, id, verifikat::Urval::Bokförda)
            .await
            .unwrap()
            .len(),
        2
    );
}

async fn preliminär_ingående_balans(rad: &Automatkontering) {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(1930, 125_000, 0), (3001, 0, 100_000), (2611, 0, 25_000)],
    )
    .await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2027-01-10",
        &[(6570, 100, 0), (1930, 0, 100)],
    )
    .await;
    let sb = saldo::saldobalans(&db, &anna, id, "2027-01-01".parse().unwrap())
        .await
        .unwrap();
    let ib = |konto: u16| {
        sb.konton
            .iter()
            .find(|k| k.konto.get() == konto)
            .map_or(0, |k| k.ingående.öre())
    };
    assert_eq!((ib(1930), ib(2611)), (125_000, -25_000));
    // Resultatet som inte är ombokat ligger på kontot som villkoret anger.
    let konton: Vec<u16> = rad.villkor.konton().iter().map(|k| k.get()).collect();
    assert_eq!(konton, [2099, 2099]);
    assert_eq!(ib(2099), -100_000);
    assert_eq!(ib(3001), 0);
}

// Kundfakturor

async fn utfärdandet(fall: &mut Vec<Fall>) {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let kund = testhjalp::kund(&db, &anna, id, "Kund AB").await;
    let rad = |sats: &str, konto: &str| NyFakturarad {
        beskrivning: format!("Tjänst {sats} %"),
        antal: "1".into(),
        à_pris: "1000".into(),
        momssats: sats.into(),
        konto: konto.into(),
        ..Default::default()
    };
    let utkast = kundfaktura::spara_utkast(
        &db,
        &klocka(),
        &anna,
        id,
        None,
        &NyKundfaktura {
            kund: kund.to_string(),
            fakturadatum: "2026-03-10".into(),
            rader: vec![
                rad("25", "3001"),
                rad("12", "3002"),
                rad("6", "3003"),
                rad("0", "3004"),
            ],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let f = kundfaktura::utfärda(&db, &klocka(), &anna, id, utkast)
        .await
        .unwrap();
    fall.push(
        Fall::nytt(
            "utfärda med fyra momssatser",
            &[Flöde::UtfärdaKundfaktura],
            rader(&db, &anna, id, f.verifikat.unwrap().id).await,
        )
        .intäkt(&[3001, 3002, 3003, 3004]),
    );

    // Omvänd skattskyldighet ger ingen momsrad.
    let eu = maya_core::kund::skapa(
        &db,
        &klocka(),
        &anna,
        id,
        &maya_domain::kund::NyKund {
            namn: "Cloud GmbH".into(),
            adress: "Hauptstraße 1\n10115 Berlin".into(),
            momsregnr: "DE123456789".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let utkast = kundfaktura::spara_utkast(
        &db,
        &klocka(),
        &anna,
        id,
        None,
        &NyKundfaktura {
            kund: eu.to_string(),
            fakturadatum: "2026-03-11".into(),
            omvänd_skattskyldighet: "eu_tjanst".into(),
            rader: vec![NyFakturarad {
                beskrivning: "Konsulttjänst".into(),
                antal: "1".into(),
                à_pris: "1000".into(),
                momssats: "0".into(),
                konto: "3308".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let f = kundfaktura::utfärda(&db, &klocka(), &anna, id, utkast)
        .await
        .unwrap();
    fall.push(
        Fall::nytt(
            "utfärda med omvänd skattskyldighet",
            &[Flöde::UtfärdaKundfaktura],
            rader(&db, &anna, id, f.verifikat.unwrap().id).await,
        )
        .intäkt(&[3308]),
    );

    // En extern kundfaktura registreras som ett utfärdande.
    let e = testhjalp::registrerad_extern_kundfaktura(&db, &anna, id, kund, "A-17").await;
    let v = extern_kundfaktura::fakturan(&db, &anna, id, e)
        .await
        .unwrap()
        .unwrap()
        .verifikat
        .unwrap();
    fall.push(
        Fall::nytt(
            "registrera extern kundfaktura",
            &[Flöde::UtfärdaKundfaktura],
            rader(&db, &anna, id, v.id).await,
        )
        .intäkt(&[3001]),
    );
}

async fn kundbetalningar(fall: &mut Vec<Fall>) {
    for (metod, flöde) in [
        ("faktura", Flöde::KundbetalningBokförd),
        ("kontant", Flöde::KundbetalningInteBokförd),
    ] {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = kundfaktura(&db, metod).await;
        let b = betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f.id),
            &betalning("2026-04-01", "5000"),
        )
        .await;
        fall.push(
            Fall::nytt(
                "betalning av kundfaktura",
                &[flöde],
                rader(&db, &anna, id, b.verifikat.id).await,
            )
            .intäkt(&[3001]),
        );
        // Rätta betalning vänder betalningens verifikat.
        let r = betalning::rätta(&db, &klocka(), &anna, id, b.id, "2026-10-18", "")
            .await
            .unwrap();
        fall.push(
            Fall::nytt(
                "rätta betalning",
                &[Flöde::RättaIFlödet],
                rader(&db, &anna, id, r.rättelse.id).await,
            )
            .original(rader(&db, &anna, id, b.verifikat.id).await),
        );
    }

    // Öresutjämning: resten på 10 kr i samma verifikat som betalningen.
    let db = Databas::tillfällig().await;
    let (anna, id, f) = kundfaktura(&db, "faktura").await;
    let b = NyBetalning {
        öresutjämning: true,
        ..betalning("2026-04-01", "11 865")
    };
    let b = betala(&db, &anna, id, Faktura::Kund(f.id), &b).await;
    fall.push(Fall::nytt(
        "öresutjämning vid betalning av kundfaktura",
        &[Flöde::ÖresutjämningKund, Flöde::KundbetalningBokförd],
        rader(&db, &anna, id, b.verifikat.id).await,
    ));
}

async fn kundkrediter(fall: &mut Vec<Fall>) {
    for (metod, flöde) in [
        ("faktura", Flöde::KundkreditBokförd),
        ("kontant", Flöde::KundkreditInteBokförd),
    ] {
        let db = Databas::tillfällig().await;
        let (anna, id, f) = kundfaktura(&db, metod).await;
        // 5 000 kr är betalt när hela fakturan krediteras, så kunden får 5 000 kr tillgodo.
        betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f.id),
            &betalning("2026-04-01", "5000"),
        )
        .await;
        let k = kreditera_hela(&db, &anna, id, f.id).await;
        fall.push(
            Fall::nytt(
                "kreditfaktura till kund",
                &[flöde],
                rader(&db, &anna, id, k.verifikat.unwrap().id).await,
            )
            .intäkt(&[3001]),
        );
        let å = betala(
            &db,
            &anna,
            id,
            Faktura::Kund(f.id),
            &betalning("2026-10-18", "5000"),
        )
        .await;
        fall.push(Fall::nytt(
            "återbetalning till kund",
            &[Flöde::ÅterbetalningTillKund],
            rader(&db, &anna, id, å.verifikat.id).await,
        ));
    }
}

async fn kundförlusten(fall: &mut Vec<Fall>) {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = kundfaktura(&db, "faktura").await;
    let förlust = kundforlust::bokför(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &NyKundförlust {
            skäl: "konkurs".into(),
            annat: String::new(),
            datum: "2026-09-30".into(),
        },
    )
    .await
    .unwrap();
    let förlustens = rader(&db, &anna, id, förlust.verifikat.unwrap().id).await;
    fall.push(Fall::nytt(
        "kundförlust",
        &[Flöde::Kundförlust],
        förlustens.clone(),
    ));
    let å = kundforlust::återkalla(&db, &klocka(), &anna, id, förlust.id)
        .await
        .unwrap();
    fall.push(
        Fall::nytt(
            "återkalla kundförlust",
            &[Flöde::ÅterkallaKundförlust],
            rader(&db, &anna, id, å.verifikat.unwrap().id).await,
        )
        .original(förlustens),
    );
}

async fn rätta_kontering_och_registrering(fall: &mut Vec<Fall>) {
    let db = Databas::tillfällig().await;
    let (anna, id, f) = kundfaktura(&db, "faktura").await;
    let utfärdandet = rader(&db, &anna, id, f.verifikat.unwrap().id).await;
    let r = kontering::rätta(
        &db,
        &klocka(),
        &anna,
        id,
        f.id,
        &maya_domain::kontering::NyKontering {
            konton: vec!["3002".into()],
            orsak: "fel momssats".into(),
        },
    )
    .await
    .unwrap();
    fall.push(
        Fall::nytt(
            "rätta kontering: rättelsen",
            &[Flöde::RättaIFlödet],
            rader(&db, &anna, id, r.rättelse.id).await,
        )
        .original(utfärdandet),
    );
    fall.push(
        Fall::nytt(
            "rätta kontering: det nya utfärdandet",
            &[Flöde::UtfärdaKundfaktura],
            rader(&db, &anna, id, r.verifikat.id).await,
        )
        .intäkt(&[3002]),
    );

    let kund = testhjalp::kund(&db, &anna, id, "Extern AB").await;
    let e = testhjalp::registrerad_extern_kundfaktura(&db, &anna, id, kund, "A-17").await;
    let v = extern_kundfaktura::fakturan(&db, &anna, id, e)
        .await
        .unwrap()
        .unwrap()
        .verifikat
        .unwrap();
    let registreringen = rader(&db, &anna, id, v.id).await;
    let r = extern_kundfaktura::rätta_registrering(&db, &klocka(), &anna, id, e, "fel datum")
        .await
        .unwrap();
    fall.push(
        Fall::nytt(
            "rätta registrering",
            &[Flöde::RättaIFlödet],
            rader(&db, &anna, id, r.rättelse.unwrap().id).await,
        )
        .original(registreringen),
    );
}

// Leverantörsfakturor

async fn leverantörsregistreringar(fall: &mut Vec<Fall>) {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB")
        .await
        .to_string();
    let (f, r) = registrera(&db, &anna, id, vanlig(&l, "8812", "")).await;
    let registreringen = rader(&db, &anna, id, r.verifikat.unwrap().id).await;
    fall.push(
        Fall::nytt(
            "registrera leverantörsfaktura",
            &[Flöde::RegistreraLeverantörsfaktura],
            registreringen.clone(),
        )
        .kostnad(&[5410]),
    );
    let a = leverantorsfaktura::rätta_avskrift(&db, &klocka(), &anna, id, f, "fel konto")
        .await
        .unwrap();
    fall.push(
        Fall::nytt(
            "rätta avskrift",
            &[Flöde::RättaIFlödet],
            rader(&db, &anna, id, a.rättelse.unwrap().id).await,
        )
        .original(registreringen),
    );

    for (omvänd, nummer, rader_) in [
        (
            "eu_tjanst",
            "EU-1",
            vec![
                rad("5410", "1 000", "25"),
                rad("5410", "1 000", "12"),
                rad("5410", "1 000", "6"),
            ],
        ),
        ("bygg", "B-1", vec![rad("5410", "1 000", "25")]),
    ] {
        let total = format!("{} 000", rader_.len());
        let (_, r) = registrera(
            &db,
            &anna,
            id,
            leverantörsfaktura(&l, nummer, &total, omvänd, "", rader_),
        )
        .await;
        fall.push(
            Fall::nytt(
                "registrera med omvänd skattskyldighet",
                &[Flöde::RegistreraOmvändSkattskyldighet],
                rader(&db, &anna, id, r.verifikat.unwrap().id).await,
            )
            .kostnad(&[5410]),
        );
    }
}

async fn leverantörsbetalningar(fall: &mut Vec<Fall>) {
    for (metod, flöde) in [
        ("faktura", Flöde::LeverantörsbetalningBokförd),
        ("kontant", Flöde::LeverantörsbetalningInteBokförd),
    ] {
        let db = Databas::tillfällig().await;
        let (anna, id) = testhjalp::företag_med_metod(&db, metod).await;
        let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB")
            .await
            .to_string();
        let (f, _) = registrera(&db, &anna, id, vanlig(&l, "8812", "")).await;
        let b = betala(
            &db,
            &anna,
            id,
            Faktura::Leverantör(f),
            &betalning("2026-04-13", "500"),
        )
        .await;
        fall.push(
            Fall::nytt(
                "betalning av leverantörsfaktura",
                &[flöde],
                rader(&db, &anna, id, b.verifikat.id).await,
            )
            .kostnad(&[5410]),
        );
        // Med omvänd skattskyldighet: den fiktiva momsen när skulden inte är bokförd.
        let (f, _) = registrera(
            &db,
            &anna,
            id,
            leverantörsfaktura(
                &l,
                "EU-1",
                "1 000",
                "eu_tjanst",
                "",
                vec![rad("5410", "1 000", "25")],
            ),
        )
        .await;
        let b = betala(
            &db,
            &anna,
            id,
            Faktura::Leverantör(f),
            &betalning("2026-04-13", "1 000"),
        )
        .await;
        fall.push(
            Fall::nytt(
                "betalning av leverantörsfaktura med omvänd skattskyldighet",
                &[flöde],
                rader(&db, &anna, id, b.verifikat.id).await,
            )
            .kostnad(&[5410]),
        );
        // Kursdifferens i betalningens verifikat: vinst och förlust.
        for (nummer, belopp) in [("V-1", "1 240"), ("V-2", "1 260")] {
            let (f, _) = registrera(&db, &anna, id, vanlig(&l, nummer, "EUR 109,50")).await;
            let b = NyBetalning {
                kursdifferens: true,
                ..betalning("2026-04-13", belopp)
            };
            let b = betala(&db, &anna, id, Faktura::Leverantör(f), &b).await;
            fall.push(
                Fall::nytt(
                    "kursdifferens",
                    &[Flöde::Kursdifferens, flöde],
                    rader(&db, &anna, id, b.verifikat.id).await,
                )
                .kostnad(&[5410]),
            );
        }
    }

    // Öresutjämning: resten på 5 kr i samma verifikat som betalningen.
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::företag(&db).await;
    let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB")
        .await
        .to_string();
    let (f, _) = registrera(&db, &anna, id, vanlig(&l, "8812", "")).await;
    let b = NyBetalning {
        öresutjämning: true,
        ..betalning("2026-04-13", "1 245")
    };
    let b = betala(&db, &anna, id, Faktura::Leverantör(f), &b).await;
    fall.push(Fall::nytt(
        "öresutjämning vid betalning av leverantörsfaktura",
        &[
            Flöde::ÖresutjämningLeverantör,
            Flöde::LeverantörsbetalningBokförd,
        ],
        rader(&db, &anna, id, b.verifikat.id).await,
    ));
}

async fn leverantörskrediter(fall: &mut Vec<Fall>) {
    for (metod, flöde) in [
        ("faktura", Flöde::LeverantörskreditBokförd),
        ("kontant", Flöde::LeverantörskreditInteBokförd),
    ] {
        let db = Databas::tillfällig().await;
        let (anna, id) = testhjalp::företag_med_metod(&db, metod).await;
        let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB")
            .await
            .to_string();
        let (f, r) = registrera(&db, &anna, id, vanlig(&l, "8812", "")).await;
        // Registreringens debetrader, eller under kontantmetoden fakturans kontering som den
        // hade blivit.
        let registreringen = match r.verifikat {
            Some(v) => rader(&db, &anna, id, v.id).await,
            None => vec![(2440, 0, 125_000), (2641, 25_000, 0), (5410, 100_000, 0)],
        };
        // 1 000 kr betalt och 250 kr obetalt när krediten på 500 kr kommer.
        betala(
            &db,
            &anna,
            id,
            Faktura::Leverantör(f),
            &betalning("2026-04-13", "1 000"),
        )
        .await;
        let (_, k) = registrera(&db, &anna, id, leverantörskredit(&l, f)).await;
        let kreditens = rader(&db, &anna, id, k.verifikat.unwrap().id).await;
        // Registreringens debetrader i proportion: hela krediten när skulden är bokförd, annars
        // de 250 kr som överstiger det obetalda.
        let väntat = match metod {
            "faktura" => vec![(2440, 50_000, 0), (2641, 0, 10_000), (5410, 0, 40_000)],
            _ => vec![(2440, 25_000, 0), (2641, 0, 5_000), (5410, 0, 20_000)],
        };
        assert_eq!(kreditens, väntat, "{metod}");
        fall.push(
            Fall::nytt("kreditfaktura från leverantör", &[flöde], kreditens)
                .kostnad(&[5410])
                .original(registreringen),
        );
        let å = betala(
            &db,
            &anna,
            id,
            Faktura::Leverantör(f),
            &betalning("2026-05-10", "250"),
        )
        .await;
        fall.push(Fall::nytt(
            "återbetalning från leverantör",
            &[Flöde::ÅterbetalningFrånLeverantör],
            rader(&db, &anna, id, å.verifikat.id).await,
        ));
    }
}

// T2

async fn t2_verifikatet(fall: &mut Vec<Fall>) {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, "kontant").await;
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB")
        .await
        .to_string();
    registrera(&db, &anna, id, vanlig(&l, "8812", "")).await;
    registrera(
        &db,
        &anna,
        id,
        leverantörsfaktura(
            &l,
            "EU-1",
            "1 000",
            "eu_tjanst",
            "",
            vec![rad("5410", "1 000", "25")],
        ),
    )
    .await;
    let nyårsdagen = Klocka::fast("2027-01-01T03:00:00Z".parse().unwrap());
    let skapade = t2::kör(&db, &nyårsdagen).await.unwrap();
    let [(_, utkast)] = skapade[..] else {
        panic!("ett T2-utkast: {skapade:?}");
    };
    let v = t2::bokför(&db, &nyårsdagen, &anna, id, utkast)
        .await
        .unwrap();
    fall.push(
        Fall::nytt("T2", &[Flöde::T2], rader(&db, &anna, id, v.id).await)
            .intäkt(&[3001])
            .kostnad(&[5410]),
    );
}

// Prövningen

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Sida {
    Debet,
    Kredit,
}

/// Kontona som raden tillåter på sidan i fallet, med nummer och med roll.
fn tillåtna(rad: &Automatkontering, sida: Sida, fall: &Fall) -> BTreeSet<u16> {
    let (egen, andra) = match sida {
        Sida::Debet => (&rad.debet, &rad.kredit),
        Sida::Kredit => (&rad.kredit, &rad.debet),
    };
    let på = |rader: &Rader, debet: bool| -> Vec<u16> {
        rader
            .iter()
            .filter(|&&(_, d, c)| if debet { d > 0 } else { c > 0 })
            .map(|&(k, _, _)| k)
            .collect()
    };
    let mut ut: BTreeSet<u16> = egen.konton().iter().map(|k| k.get()).collect();
    for roll in egen.roller() {
        let konton = match roll {
            Roll::Intäkt => fall.intäkt.clone(),
            Roll::Kostnad => fall.kostnad.clone(),
            Roll::Betalkonto => vec![BETALKONTO],
            Roll::OriginaletsDebet => på(&fall.original, true),
            Roll::OriginaletsKredit => på(&fall.original, false),
            Roll::Rättelse => fall.original.iter().map(|&(k, _, _)| k).collect(),
            r => r.konton().iter().map(|k| k.get()).collect(),
        };
        ut.extend(konton);
    }
    // Den fiktiva momsen är ett par som står på båda sidor, och en rättelse vänder varje rad.
    for roll in andra.roller() {
        match roll {
            Roll::FiktivMoms => ut.extend(roll.konton().iter().map(|k| k.get())),
            Roll::Rättelse => ut.extend(fall.original.iter().map(|&(k, _, _)| k)),
            _ => {}
        }
    }
    ut
}

fn bokförda(rader: &Rader, sida: Sida) -> BTreeSet<u16> {
    rader
        .iter()
        .filter(|&&(_, d, c)| match sida {
            Sida::Debet => d > 0,
            Sida::Kredit => c > 0,
        })
        .map(|&(k, _, _)| k)
        .collect()
}

#[tokio::test]
async fn varje_rad_i_tabellen_är_det_som_flödet_bokför() {
    let tabell = automatkontering::tabell();
    let raden = |f: Flöde| tabell.iter().find(|r| r.flöde == f).unwrap();

    let mut fall = Vec::new();
    rättelsen(&mut fall).await;
    utfärdandet(&mut fall).await;
    kundbetalningar(&mut fall).await;
    kundkrediter(&mut fall).await;
    kundförlusten(&mut fall).await;
    rätta_kontering_och_registrering(&mut fall).await;
    leverantörsregistreringar(&mut fall).await;
    leverantörsbetalningar(&mut fall).await;
    leverantörskrediter(&mut fall).await;
    t2_verifikatet(&mut fall).await;

    // Varje rad i verifikatet står på en sida och ett konto som någon av fallets rader i
    // tabellen tillåter.
    let mut täckta: HashMap<(Flöde, Sida), BTreeSet<u16>> = HashMap::new();
    for f in &fall {
        assert!(!f.rader.is_empty(), "{}: tomt verifikat", f.namn);
        for sida in [Sida::Debet, Sida::Kredit] {
            let tillåtet: BTreeSet<u16> = f
                .flöden
                .iter()
                .flat_map(|&flöde| tillåtna(raden(flöde), sida, f))
                .collect();
            let bokfört = bokförda(&f.rader, sida);
            let utanför: Vec<_> = bokfört.difference(&tillåtet).collect();
            assert!(
                utanför.is_empty(),
                "{}: {sida:?} på {utanför:?} står inte i raden {:?} (tillåtet {tillåtet:?}): {:?}",
                f.namn,
                f.flöden,
                f.rader
            );
            täckta
                .entry((f.flöden[0], sida))
                .or_default()
                .extend(bokfört);
        }
        // En rättelse vänder varje rad i flödets verifikat.
        let flödet = raden(f.flöden[0]);
        let speglar = flödet
            .debet
            .roller()
            .iter()
            .chain(flödet.kredit.roller().iter())
            .any(|r| matches!(r, Roll::Rättelse | Roll::OriginaletsKredit));
        if speglar {
            assert_eq!(f.rader, vänd(&f.original), "{}", f.namn);
        }
    }

    // Varje rad har fall, och varje konto som raden anger med nummer bokförs av ett fall.
    for flöde in Flöde::ALLA {
        let rad = raden(flöde);
        match flöde {
            Flöde::RegistreraOm => continue, // prövas i rättelsen: inget bokförs
            Flöde::PreliminärIngåendeBalans => {
                preliminär_ingående_balans(rad).await;
                continue;
            }
            Flöde::Rättelse
            | Flöde::UtfärdaKundfaktura
            | Flöde::KundbetalningBokförd
            | Flöde::KundbetalningInteBokförd
            | Flöde::KundkreditBokförd
            | Flöde::KundkreditInteBokförd
            | Flöde::ÅterbetalningTillKund
            | Flöde::Kundförlust
            | Flöde::ÅterkallaKundförlust
            | Flöde::RegistreraLeverantörsfaktura
            | Flöde::RegistreraOmvändSkattskyldighet
            | Flöde::LeverantörsbetalningBokförd
            | Flöde::LeverantörsbetalningInteBokförd
            | Flöde::LeverantörskreditBokförd
            | Flöde::LeverantörskreditInteBokförd
            | Flöde::ÅterbetalningFrånLeverantör
            | Flöde::ÖresutjämningKund
            | Flöde::ÖresutjämningLeverantör
            | Flöde::Kursdifferens
            | Flöde::RättaIFlödet
            | Flöde::T2 => {}
        }
        assert!(
            fall.iter().any(|f| f.flöden[0] == flöde),
            "inget fall för {flöde:?}"
        );
        for (sida, ruta) in [(Sida::Debet, &rad.debet), (Sida::Kredit, &rad.kredit)] {
            let bokfört = täckta.get(&(flöde, sida)).cloned().unwrap_or_default();
            for k in ruta.konton() {
                assert!(
                    bokfört.contains(&k.get()),
                    "{flöde:?}: {sida:?} {} bokförs inte av något fall ({bokfört:?})",
                    k.get()
                );
            }
        }
    }
}
