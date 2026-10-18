//! PDF:erna läses tillbaka: PDF/A-2b-märkningen, inbäddade typsnitt och texten per sida.

use jiff::Timestamp;
use maya_domain::belopp::Belopp;
use maya_domain::kontoplan::Kontonummer;
use maya_pdf::testhjalp::{innehåller, sidor, är_pdfa_2b};
use maya_pdf::{
    Dokument, Företag, Historikpost, Konto, Kontoplan, Rad, Sidfot, Signatur, Underlag, Verifikat,
    skriv,
};

fn sidfot() -> Sidfot {
    Sidfot {
        tidpunkt: "2026-10-18T12:34:00Z".parse().unwrap(),
        användare: "Anna Andersson".into(),
    }
}

fn företag() -> Företag {
    Företag {
        namn: "Exempel AB".into(),
        orgnr: "556036-0793".into(),
    }
}

fn nr(n: u16) -> Kontonummer {
    Kontonummer::new(n).unwrap()
}

fn kr(öre: i64) -> Belopp {
    Belopp::från_öre(öre)
}

fn tid(s: &str) -> Timestamp {
    s.parse().unwrap()
}

fn kontoplan(antal: u16) -> Kontoplan {
    let mut konton: Vec<Konto> = (0..antal)
        .map(|i| Konto {
            nummer: nr(1000 + i * 7),
            namn: format!("Konto nummer {i}"),
            aktiv: i % 3 != 0,
        })
        .collect();
    konton.push(Konto {
        nummer: nr(1930),
        namn: "Företagskonto/checkkonto/affärskonto".into(),
        aktiv: true,
    });
    Kontoplan {
        företag: företag(),
        bas_version: "BAS 2026".into(),
        konton,
    }
}

#[test]
fn kontoplanen_är_pdfa_2b_med_sidfot_på_varje_sida() {
    let pdf = skriv(&Dokument::Kontoplan(kontoplan(200)), &sidfot()).unwrap();

    är_pdfa_2b(&pdf);
    let sidor = sidor(&pdf);
    assert!(sidor.len() >= 2, "200 konton ryms inte på en sida");
    let antal = sidor.len();
    for (i, sida) in sidor.iter().enumerate() {
        // 12:34 UTC är 14:34 i Stockholm i oktober.
        assert!(innehåller(sida, "2026-10-18 14:34"), "tidpunkt på sida {i}");
        assert!(innehåller(sida, "Anna Andersson"), "användare på sida {i}");
        assert!(
            innehåller(sida, &format!("Maya {}", env!("CARGO_PKG_VERSION"))),
            "version på sida {i}"
        );
        assert!(
            innehåller(sida, maya_domain::kontoplan::ink2::UTGÅVA),
            "INK2-utgåva på sida {i}"
        );
        assert!(
            innehåller(sida, &format!("Sida {} av {antal}", i + 1)),
            "sidnummer på sida {i}: {sida}"
        );
    }
}

#[test]
fn kontoplanen_har_konton_klasser_och_ink2_rader() {
    let pdf = skriv(&Dokument::Kontoplan(kontoplan(3)), &sidfot()).unwrap();
    let text = sidor(&pdf).concat();

    assert!(innehåller(&text, "Kontoplan"));
    assert!(innehåller(&text, "Exempel AB"));
    assert!(innehåller(&text, "556036-0793"));
    assert!(innehåller(&text, "BAS 2026"));
    assert!(innehåller(&text, "1 Tillgångar"));
    assert!(innehåller(
        &text,
        "1930 Företagskonto/checkkonto/affärskonto"
    ));
    // 1930 hör till INK2-raden 2.26 Kassa, bank och redovisningsmedel.
    assert!(innehåller(&text, "2.26"), "{text}");
    assert!(innehåller(&text, "Inaktiv"));
}

fn verifikat() -> Verifikat {
    let anna = |t: &str| Signatur {
        tid: tid(t),
        namn: "Anna Andersson".into(),
        kanal: "webb".into(),
    };
    Verifikat {
        företag: företag(),
        nummer: Some("V5".into()),
        beskrivning: "Kontantförsäljning på marknaden".into(),
        datum: Some("2026-03-14".parse().unwrap()),
        period: Some("mars 2026".into()),
        skapad: anna("2026-03-14T09:00:00Z"),
        bokförd: Some(anna("2026-03-14T09:05:00Z")),
        rader: vec![
            Rad {
                konto: Some(nr(1930)),
                kontonamn: "Företagskonto".into(),
                text: Some("Swish".into()),
                debet: kr(1_250_000),
                kredit: Belopp::NOLL,
            },
            Rad {
                konto: Some(nr(3001)),
                kontonamn: "Försäljning inom Sverige, 25 % moms".into(),
                text: None,
                debet: Belopp::NOLL,
                kredit: kr(1_250_000),
            },
        ],
        underlag: vec![
            Underlag {
                filnamn: "kvitto-marknad.pdf".into(),
                sha256: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".into(),
                storlek: 48_213,
                komplettering: None,
            },
            Underlag {
                filnamn: "sent-kvitto.jpg".into(),
                sha256: "60303ae22b998861bce3b28f33eec1be758a213c86c93c076dbe9f558c11c752".into(),
                storlek: 1_234_567,
                komplettering: Some(Signatur {
                    tid: tid("2026-04-02T10:00:00Z"),
                    namn: "Bertil Berg".into(),
                    kanal: "webb".into(),
                }),
            },
        ],
        historik: vec![Historikpost {
            tid: tid("2026-03-14T09:05:00Z"),
            typ: "verifikat bokfört".into(),
            namn: "Anna Andersson".into(),
            kanal: "webb".into(),
        }],
    }
}

#[test]
fn verifikatet_är_pdfa_2b_med_sidfot() {
    let pdf = skriv(&Dokument::Verifikat(verifikat()), &sidfot()).unwrap();

    är_pdfa_2b(&pdf);
    let sidor = sidor(&pdf);
    assert_eq!(sidor.len(), 1);
    assert!(innehåller(&sidor[0], "2026-10-18 14:34"));
    assert!(innehåller(&sidor[0], "Anna Andersson"));
    assert!(innehåller(&sidor[0], maya_domain::kontoplan::ink2::UTGÅVA));
    assert!(innehåller(&sidor[0], "Sida 1 av 1"));
}

#[test]
fn verifikatet_har_rubrik_metadata_rader_underlag_och_historik() {
    let pdf = skriv(&Dokument::Verifikat(verifikat()), &sidfot()).unwrap();
    let text = sidor(&pdf).concat();

    for väntat in [
        // Rubrik, nummer och metadata.
        "V5",
        "Kontantförsäljning på marknaden",
        "Exempel AB",
        "2026-03-14",
        "mars 2026",
        "12 500,00",
        "2026-03-14 10:05",
        // Raderna.
        "1930",
        "Företagskonto",
        "Swish",
        "3001",
        "Försäljning inom Sverige, 25 % moms",
        // Underlagsförteckningen med hash, och kompletteringen med vem och när.
        "kvitto-marknad.pdf",
        "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        "sent-kvitto.jpg",
        "60303ae22b998861bce3b28f33eec1be758a213c86c93c076dbe9f558c11c752",
        "Komplettering 2026-04-02 12:00 av Bertil Berg",
        // Historiken.
        "verifikat bokfört",
    ] {
        assert!(innehåller(&text, väntat), "saknar {väntat:?} i {text}");
    }
}

#[test]
fn utkast_utan_underlag_säger_det() {
    let mut v = verifikat();
    v.nummer = None;
    v.bokförd = None;
    v.underlag.clear();
    let text = sidor(&skriv(&Dokument::Verifikat(v), &sidfot()).unwrap()).concat();

    assert!(innehåller(&text, "Utkast"));
    assert!(innehåller(&text, "Inga underlag"));
}

#[test]
fn samma_indata_ger_samma_bytes() {
    let a = skriv(&Dokument::Verifikat(verifikat()), &sidfot()).unwrap();
    let b = skriv(&Dokument::Verifikat(verifikat()), &sidfot()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn text_ur_indata_tolkas_aldrig_som_typst() {
    let mut v = verifikat();
    v.beskrivning = "#panic(\"x\") *fet* $x$ <etikett> @ref".into();
    let text = sidor(&skriv(&Dokument::Verifikat(v), &sidfot()).unwrap()).concat();

    assert!(innehåller(&text, "#panic(\"x\") *fet* $x$ <etikett> @ref"));
}
