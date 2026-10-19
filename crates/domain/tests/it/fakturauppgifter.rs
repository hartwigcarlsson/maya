use maya_domain::fakturauppgifter::{
    Fakturauppgifter, Fält, STÖRSTA_LOGOTYP, kontrollera_logotyp
};

fn koder(f: &Fakturauppgifter) -> Vec<(Fält, &'static str)> {
    f.kontrollera()
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|f| (f.fält, f.kod))
        .collect()
}

#[test]
fn tomma_fakturauppgifter_godtas_men_saknar_betalsätt() {
    let tomma = Fakturauppgifter::default().kontrollera().unwrap();
    assert!(!tomma.har_betalsätt());
    assert!(!tomma.ocr);
}

#[test]
fn betalsätten_skrivs_på_ett_enhetligt_sätt() {
    let f = Fakturauppgifter {
        bankgiro: "50501055".into(),
        plusgiro: "123455".into(),
        iban: "se4550000000058398257466".into(),
        bic: "essesess".into(),
        ocr: true,
        svarsadress: " Faktura@Exempel.se ".into(),
        standardmeddelande: "  Hej! Här kommer fakturan.\n".into(),
    }
    .kontrollera()
    .unwrap();
    assert_eq!(f.bankgiro, "5050-1055");
    assert_eq!(f.plusgiro, "12345-5");
    assert_eq!(f.iban, "SE45 5000 0000 0583 9825 7466");
    assert_eq!(f.bic, "ESSESESS");
    assert!(f.ocr);
    assert_eq!(f.svarsadress, "faktura@exempel.se");
    assert_eq!(f.standardmeddelande, "Hej! Här kommer fakturan.");
    assert!(f.har_betalsätt());
}

#[test]
fn ett_av_betalsätten_räcker() {
    for f in [
        Fakturauppgifter {
            bankgiro: "5050-1055".into(),
            ..Default::default()
        },
        Fakturauppgifter {
            plusgiro: "12345-5".into(),
            ..Default::default()
        },
        Fakturauppgifter {
            iban: "SE4550000000058398257466".into(),
            bic: "ESSESESS".into(),
            ..Default::default()
        },
    ] {
        assert!(f.kontrollera().unwrap().har_betalsätt(), "{f:?}");
    }
}

#[test]
fn felaktiga_betalsätt_och_svarsadress_ger_koder_vid_fälten_i_ordning() {
    let f = Fakturauppgifter {
        bankgiro: "5050-1056".into(),
        plusgiro: "1".into(),
        iban: "SE46 5000 0000 0583 9825 7466".into(),
        bic: "ESS".into(),
        svarsadress: "faktura".into(),
        ..Default::default()
    };
    assert_eq!(
        koder(&f),
        [
            (Fält::Bankgiro, "bankgiro_kontrollsiffra"),
            (Fält::Plusgiro, "plusgiro_ogiltigt"),
            (Fält::Iban, "iban_kontrollsiffra"),
            (Fält::Bic, "bic_ogiltig"),
            (Fält::Svarsadress, "svarsadress_ogiltig"),
        ]
    );
}

#[test]
fn ett_iban_nummer_kräver_en_bic_och_en_bic_ett_iban_nummer() {
    let utan_bic = Fakturauppgifter {
        iban: "SE4550000000058398257466".into(),
        ..Default::default()
    };
    assert_eq!(koder(&utan_bic), [(Fält::Bic, "bic_saknas")]);
    let utan_iban = Fakturauppgifter {
        bic: "ESSESESS".into(),
        ..Default::default()
    };
    assert_eq!(koder(&utan_iban), [(Fält::Iban, "iban_saknas")]);
}

#[test]
fn fältens_namn_är_formulärets() {
    let namn: Vec<&str> = [
        Fält::Bankgiro,
        Fält::Plusgiro,
        Fält::Iban,
        Fält::Bic,
        Fält::Ocr,
        Fält::Svarsadress,
        Fält::Standardmeddelande,
        Fält::Logotyp,
    ]
    .into_iter()
    .map(Fält::namn)
    .collect();
    assert_eq!(
        namn,
        [
            "bankgiro",
            "plusgiro",
            "iban",
            "bic",
            "ocr",
            "svarsadress",
            "standardmeddelande",
            "logotyp"
        ]
    );
}

#[test]
fn en_logotyp_är_png_jpg_eller_svg_efter_innehållet() {
    assert_eq!(
        kontrollera_logotyp(b"\x89PNG\r\n\x1a\n rest"),
        Ok("image/png")
    );
    assert_eq!(
        kontrollera_logotyp(b"\xff\xd8\xff\xe0 rest"),
        Ok("image/jpeg")
    );
    assert_eq!(
        kontrollera_logotyp(br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#),
        Ok("image/svg+xml")
    );
    assert_eq!(
        kontrollera_logotyp(
            "\u{feff}<?xml version=\"1.0\"?>\n<!-- logga -->\n<svg viewBox=\"0 0 1 1\"/>"
                .as_bytes()
        ),
        Ok("image/svg+xml")
    );
}

#[test]
fn andra_format_än_png_jpg_och_svg_nekas() {
    for fil in [
        &b"GIF89a rest"[..],
        b"%PDF-1.7",
        b"<html><body><svg></svg></body></html>",
        b"RIFF\0\0\0\0WEBPVP8 ",
        b"bara text",
        b"\xff\xfe<\0s\0v\0g\0",
    ] {
        let fel = kontrollera_logotyp(fil).unwrap_err();
        assert_eq!(
            (fel.fält, fel.kod),
            (Fält::Logotyp, "logotyp_format"),
            "{fil:?}"
        );
    }
    let fel = kontrollera_logotyp(b"").unwrap_err();
    assert_eq!(fel.kod, "logotyp_saknas");
}

#[test]
fn en_för_stor_logotyp_nekas() {
    let mut stor = b"\x89PNG\r\n\x1a\n".to_vec();
    stor.resize(STÖRSTA_LOGOTYP, 0);
    assert!(kontrollera_logotyp(&stor).is_ok());
    stor.push(0);
    assert_eq!(
        kontrollera_logotyp(&stor).unwrap_err().kod,
        "logotyp_for_stor"
    );
}
