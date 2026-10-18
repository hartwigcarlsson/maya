//! Kundfakturans PDF: PDF/A-2b, momslagens fakturauppgifter i texten och logotypen.

use jiff::civil::date;
use maya_domain::foretag::Momsperiod;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kundfaktura::{
    Fakturakund, Förutsättningar, NyFakturarad, NyKundfaktura, Säljare, Utfärdbar,
};
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_pdf::testhjalp::{bilder, innehåller, sidor, är_pdfa_2b};
use maya_pdf::{Dokument, Kundfaktura, Logotyp, Sidfot, skriv};

/// En PNG med en enda pixel.
const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x01\x00\x00\x00\x01\x08\x02\x00\x00\x00\x90\x77\x53\xde\x00\x00\x00\x0c\x49\x44\x41\x54\x78\x9c\x63\xf8\xdf\xc0\x00\x00\x04\x01\x01\x80\xc5\x2a\x18\x5d\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

fn säljare() -> Säljare {
    Säljare {
        namn: "Exempel AB".into(),
        orgnr: "556036-0793".into(),
        gatuadress: "Exempelgatan 1".into(),
        postnummer: "123 45".into(),
        postort: "Exempelstad".into(),
        säte: "Stockholm".into(),
        momsregnr: "SE556036079301".into(),
        f_skatt: true,
        bankgiro: "5050-1055".into(),
        plusgiro: String::new(),
        iban: "SE45 5000 0000 0583 9825 7466".into(),
        bic: "ESSESESS".into(),
        ocr: true,
    }
}

fn faktura() -> Utfärdbar {
    let f = Förutsättningar {
        kunder: vec![Fakturakund {
            id: 1,
            namn: "Kund AB".into(),
            identitet: "556677-8899".into(),
            adress: "Storgatan 1\n111 22 Stockholm".into(),
            epost: String::new(),
            momsregnr: String::new(),
            betalningsvillkor: 30,
        }],
        säljare: säljare(),
        konton: [3001, 3003]
            .into_iter()
            .map(|n| (Kontonummer::new(n).unwrap(), true))
            .collect(),
        räkenskapsår: vec![Räkenskapsår::befintligt(
            date(2026, 1, 1),
            date(2026, 12, 31),
        )],
        momsperioder: vec![(date(2026, 1, 1), Momsperiod::Kvartal)],
        nästa_nummer: Some(1007),
        startnummer: 1,
    };
    NyKundfaktura {
        kund: "1".into(),
        fakturadatum: "2026-03-10".into(),
        leveransdatum: "februari 2026".into(),
        förfallodag: String::new(),
        er_referens: "Bo Ek".into(),
        startnummer: String::new(),
        rader: vec![
            NyFakturarad {
                beskrivning: "Konsulttimmar".into(),
                antal: "10".into(),
                enhet: "tim".into(),
                à_pris: "950".into(),
                rabatt: "10".into(),
                momssats: "25".into(),
                konto: "3001".into(),
            },
            NyFakturarad {
                beskrivning: "Handbok".into(),
                antal: "2".into(),
                enhet: "st".into(),
                à_pris: "200".into(),
                rabatt: String::new(),
                momssats: "6".into(),
                konto: "3003".into(),
            },
        ],
    }
    .kontrollera(&f)
    .unwrap()
}

fn sidfot() -> Sidfot {
    Sidfot {
        tidpunkt: "2026-03-10T09:15:00Z".parse().unwrap(),
        användare: "Anna Andersson".into(),
    }
}

fn dokument(logotyp: Option<Logotyp>) -> Dokument {
    Dokument::Kundfaktura(Kundfaktura {
        faktura: faktura(),
        säljare: säljare(),
        logotyp,
        utkast: false,
    })
}

#[test]
fn kundfakturan_är_pdfa_2b_med_momslagens_fakturauppgifter() {
    let pdf = skriv(&dokument(None), &sidfot()).unwrap();
    är_pdfa_2b(&pdf);
    let text = sidor(&pdf).join("");

    for uppgift in [
        // Fakturadatum, löpnummer och förfallodag.
        "Faktura 1007",
        "Fakturadatum 2026-03-10",
        "Förfallodag 2026-04-09",
        // Leveransdatum eller period.
        "Leveransdatum februari 2026",
        // Säljaren: namn, adress, orgnr, säte, momsregistreringsnummer och F-skatt.
        "Exempel AB",
        "Exempelgatan 1",
        "123 45 Exempelstad",
        "Org.nr 556036-0793",
        "Styrelsens säte Stockholm",
        "Momsreg.nr SE556036079301",
        "Godkänd för F-skatt",
        // Köparen: namn och adress.
        "Kund AB",
        "Storgatan 1",
        "111 22 Stockholm",
        "Er referens Bo Ek",
        // Art och omfattning, à-pris exklusive moms, rabatt och momssats.
        "Konsulttimmar",
        "10 tim",
        "950,00",
        "10 %",
        "8 550,00",
        "Handbok",
        // Beskattningsunderlag och moms per momssats, och att betala.
        "Moms 25 % på 8 550,00",
        "2 137,50",
        "Moms 6 % på 400,00",
        "24,00",
        "Att betala 11 111,50 kr",
        // Betalsätt och referens.
        "Bankgiro 5050-1055",
        "IBAN SE45 5000 0000 0583 9825 7466",
        "BIC ESSESESS",
        "OCR 100768",
    ] {
        assert!(innehåller(&text, uppgift), "{uppgift} saknas i {text}");
    }
    assert!(!innehåller(&text, "UTKAST"));
    // En faktura har ingen utskriftssidfot: den är kundens exemplar.
    assert!(!innehåller(&text, "Utskrivet"));
}

#[test]
fn utan_ocr_är_fakturanumret_referensen() {
    let mut s = säljare();
    s.ocr = false;
    let mut f = faktura();
    f.referens = "1007".into();
    let pdf = skriv(
        &Dokument::Kundfaktura(Kundfaktura {
            faktura: f,
            säljare: s,
            logotyp: None,
            utkast: false,
        }),
        &sidfot(),
    )
    .unwrap();
    let text = sidor(&pdf).join("");
    assert!(
        innehåller(&text, "Ange fakturanummer 1007 vid betalning"),
        "{text}"
    );
    assert!(!innehåller(&text, "OCR"));
}

#[test]
fn logotypen_kommer_med_och_samma_indata_ger_samma_fil() {
    let logotyp = || {
        Some(Logotyp {
            mime: "image/png".into(),
            innehåll: PNG.to_vec(),
        })
    };
    let med = skriv(&dokument(logotyp()), &sidfot()).unwrap();
    let utan = skriv(&dokument(None), &sidfot()).unwrap();
    är_pdfa_2b(&med);
    assert_eq!(bilder(&med), 1);
    assert_eq!(bilder(&utan), 0);
    assert_eq!(med, skriv(&dokument(logotyp()), &sidfot()).unwrap());
}

#[test]
fn en_förhandsvisning_av_ett_utkast_är_märkt() {
    let pdf = skriv(
        &Dokument::Kundfaktura(Kundfaktura {
            faktura: faktura(),
            säljare: säljare(),
            logotyp: None,
            utkast: true,
        }),
        &sidfot(),
    )
    .unwrap();
    assert!(innehåller(&sidor(&pdf).join(""), "UTKAST"));
}
