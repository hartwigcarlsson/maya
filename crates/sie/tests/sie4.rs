//! SIE4-skrivaren mot en förväntad fil, och läsaren tillbaka.

use jiff::civil::date;
use maya_sie::{Konto, Räkenskapsår, Saldopost, Sie4, Transaktion, Verifikation};

/// En liten 4E-fil med två år, tecken utanför ASCII och citattecken i texterna.
fn exempel() -> Sie4 {
    Sie4 {
        program: "Maya".into(),
        version: "0.1.0".into(),
        genererad: date(2026, 10, 18),
        genererad_av: Some("Anna Andersson".into()),
        orgnr: Some("556036-0793".into()),
        företagsform: Some("AB".into()),
        företagsnamn: "Exempel \"Kaffe\" AB".into(),
        kontoplanstyp: Some("BAS2026".into()),
        räkenskapsår: vec![
            Räkenskapsår {
                årsnr: 0,
                start: date(2026, 1, 1),
                slut: date(2026, 12, 31),
            },
            Räkenskapsår {
                årsnr: -1,
                start: date(2025, 1, 1),
                slut: date(2025, 12, 31),
            },
        ],
        konton: vec![
            Konto {
                nummer: 1930,
                namn: "Företagskonto / checkkonto".into(),
                sru: Some(7281),
            },
            Konto {
                nummer: 2099,
                namn: "Årets resultat".into(),
                sru: Some(7302),
            },
            Konto {
                nummer: 3001,
                namn: "Försäljning inom Sverige, 25 % moms".into(),
                sru: Some(7410),
            },
        ],
        ingående: vec![
            saldo(0, 1930, 100_000),
            saldo(0, 2099, -100_000),
            saldo(-1, 1930, 50_000),
            saldo(-1, 2099, -50_000),
        ],
        utgående: vec![
            saldo(0, 1930, 112_550),
            saldo(0, 2099, -100_000),
            saldo(-1, 1930, 100_000),
            saldo(-1, 2099, -50_000),
        ],
        resultat: vec![saldo(0, 3001, -12_550), saldo(-1, 3001, -50_000)],
        verifikat: vec![
            Verifikation {
                serie: "V".into(),
                nummer: 1,
                datum: date(2026, 3, 14),
                text: "Kaffe – café".into(),
                regdatum: Some(date(2026, 3, 15)),
                sign: Some("Anna Andersson".into()),
                transaktioner: vec![
                    Transaktion {
                        konto: 1930,
                        öre: 12_500,
                        text: None,
                    },
                    Transaktion {
                        konto: 3001,
                        öre: -12_500,
                        text: Some("Kaffe till kontoret".into()),
                    },
                ],
            },
            Verifikation {
                serie: "V".into(),
                nummer: 2,
                datum: date(2026, 3, 20),
                text: "Ränta".into(),
                regdatum: Some(date(2026, 3, 20)),
                sign: Some("Anna Andersson".into()),
                transaktioner: vec![
                    Transaktion {
                        konto: 1930,
                        öre: 50,
                        text: None,
                    },
                    Transaktion {
                        konto: 3001,
                        öre: -50,
                        text: None,
                    },
                ],
            },
        ],
    }
}

fn saldo(årsnr: i32, konto: u32, öre: i64) -> Saldopost {
    Saldopost {
        årsnr, konto, öre
    }
}

#[test]
fn filen_skrivs_som_den_förväntade_i_cp437_med_citering_och_ksumma() {
    let förväntad = include_bytes!("golden/exempel.se");
    let skriven = maya_sie::skriv(&exempel());
    assert_eq!(
        String::from_utf8_lossy(&skriven),
        String::from_utf8_lossy(förväntad)
    );
    assert_eq!(skriven, förväntad);
}

#[test]
fn den_förväntade_filen_läses_tillbaka_till_samma_innehåll() {
    let mut förväntat = exempel();
    // Tankstrecket finns inte i CP437 och skrevs som bindestreck.
    förväntat.verifikat[0].text = "Kaffe - café".into();
    let läst = maya_sie::läs(include_bytes!("golden/exempel.se")).unwrap();
    assert_eq!(läst, förväntat);
}

#[test]
fn en_ändrad_post_ger_fel_i_ksumman() {
    let fil = include_bytes!("golden/exempel.se")
        .split(|&b| b == b'\n')
        .map(|rad| match rad {
            b"#TRANS 1930 {} 0.50\r" => b"#TRANS 1930 {} 0.60\r".to_vec(),
            b"#TRANS 3001 {} -0.50\r" => b"#TRANS 3001 {} -0.60\r".to_vec(),
            rad => rad.to_vec(),
        })
        .collect::<Vec<_>>()
        .join(&b'\n');
    let fel = maya_sie::läs(&fil).unwrap_err();
    assert!(fel.to_string().contains("KSUMMA"), "{fel}");
}

#[test]
fn läsaren_tål_okända_poster_tomma_rader_lf_och_citerade_tal() {
    let fil = "#FLAGGA 1\n\n#PROGRAM Annat 2.0\n#FORMAT PC8\n#GEN \"20250102\"\n#SIETYP \"4\"\n\
               #FNAMN \"Ett AB\"\n#PROSA \"okänd post\"\n#KONTO 1930 Bank extra fält\n\
               #VER A 7 20250105 Text\n{\n#RTRANS 1930 {} 1.00\n#TRANS 1930 {\"1\" \"4 5\"} 1.00\n\
               #BTRANS 1930 {} 2\n#TRANS 2440 {} -1\n}\n";
    let läst = maya_sie::läs(fil.as_bytes()).unwrap();
    assert_eq!(läst.program, "Annat");
    assert_eq!(läst.genererad, date(2025, 1, 2));
    assert_eq!(läst.konton[0].namn, "Bank");
    assert_eq!(
        läst.verifikat[0].transaktioner,
        [
            Transaktion {
                konto: 1930,
                öre: 100,
                text: None
            },
            Transaktion {
                konto: 2440,
                öre: -100,
                text: None
            }
        ]
    );
}

#[test]
fn belopp_med_fler_än_två_decimaler_avvisas() {
    let fil = "#FLAGGA 0\n#PROGRAM Maya 1\n#FORMAT PC8\n#GEN 20250102\n#SIETYP 4\n#FNAMN X\n\
               #IB 0 1930 1.005\n";
    let fel = maya_sie::läs(fil.as_bytes()).unwrap_err();
    assert!(fel.to_string().contains("rad 7"), "{fel}");
}

#[test]
fn en_påbörjad_ksumma_som_inte_avslutas_avvisas() {
    let fil = "#FLAGGA 0\n#KSUMMA\n#PROGRAM Maya 1\n#FORMAT PC8\n#GEN 20250102\n#SIETYP 4\n\
               #FNAMN X\n";
    let fel = maya_sie::läs(fil.as_bytes()).unwrap_err();
    assert!(fel.to_string().contains("KSUMMA"), "{fel}");
}
