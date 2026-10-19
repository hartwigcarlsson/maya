//! Reglerna för utskick av kundfakturor (#138): ämnesraden, det förvalda meddelandet och sätten
//! att lämna en faktura på annat sätt.

use jiff::civil::date;
use maya_domain::utskick::{Lämningssätt, förvalt_meddelande, ämne};

#[test]
fn ämnesraden_har_fakturanumret_och_säljaren() {
    assert_eq!(ämne(1042, "Exempel AB"), "Faktura 1042 från Exempel AB");
}

#[test]
fn ämnesraden_får_inga_radbrytningar_ur_säljarens_namn() {
    assert_eq!(
        ämne(7, "Exempel\r\nBcc: x@y.se"),
        "Faktura 7 från Exempel Bcc: x@y.se"
    );
}

#[test]
fn standardmeddelandet_förvals_när_företaget_har_ett() {
    let text = förvalt_meddelande(
        "  Hej! Fakturan är bifogad.\n\nMvh Exempel AB  ",
        1042,
        1_187_500,
        date(2026, 3, 31),
        "Exempel AB",
    );
    assert_eq!(text, "Hej! Fakturan är bifogad.\n\nMvh Exempel AB");
}

#[test]
fn utan_standardmeddelande_förvals_en_text_med_nummer_belopp_och_förfallodag() {
    let text = förvalt_meddelande("", 1042, 1_187_500, date(2026, 3, 31), "Exempel AB");
    assert_eq!(
        text,
        "Hej!\n\nHär kommer faktura 1042 på 11 875,00 kr, att betala senast 2026-03-31. \
         Fakturan är bifogad som PDF.\n\nVänliga hälsningar\nExempel AB"
    );
}

#[test]
fn sätten_att_lämna_en_faktura_har_koder_och_namn() {
    let namn: Vec<(&str, &str)> = Lämningssätt::ALLA
        .iter()
        .map(|s| (s.kod(), s.namn()))
        .collect();
    assert_eq!(
        namn,
        [
            ("post", "Med post"),
            ("personligen", "Överlämnad personligen"),
            ("portal", "Uppladdad i kundens portal"),
        ]
    );
    for s in Lämningssätt::ALLA {
        assert_eq!(Lämningssätt::från_kod(s.kod()), Some(*s));
    }
    assert_eq!(Lämningssätt::från_kod("fax"), None);
}
