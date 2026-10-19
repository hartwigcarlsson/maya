//! Rättelse av ett bokfört verifikat (R1, R2, VF7), enligt tabellen över automatkonteringar:
//! för varje rad i originalet samma konto med debet och kredit bytta, och radtexten följer med.
//! Rättelsen är ett nytt verifikat; originalet ändras aldrig (L1).
//!
//! Ett verifikat kan rättas högst en gång och en rättelse kan inte rättas. Rättelsens datum är
//! tidigast originalets och prövas annars med samma spärrar som när ett verifikat bokförs.
//! Periodlåsningen (R3) kommer i etapp 3: då ska datumet också ligga i en period som inte är låst.

use std::fmt;

use jiff::civil::Date;

use crate::belopp::Belopp;
use crate::verifikat::{
    BokförbartVerifikat, Fält, Förutsättningar, NyRad, NyttVerifikat, Rad, Spärr,
};

/// Det som avgör om ett bokfört verifikat kan rättas, och rättelsen av det.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Original {
    /// Verifikationsnumret, t.ex. `V5`.
    pub nummer: String,
    pub datum: Date,
    pub rader: Vec<Rad>,
    /// Numret på rättelsen, om verifikatet redan är rättat.
    pub rättad_av: Option<String>,
    /// Numret på verifikatet som det här rättar, om det är en rättelse.
    pub rättar: Option<String>,
    /// Fakturan som verifikatet bokfördes från, t.ex. "leverantörsfaktura 8812 från Kontorsvaror
    /// AB", när det kommer från ett av Mayas flöden. Ett sådant verifikat rättas i fakturan.
    pub flöde: Option<String>,
    /// Verifikatet är ett T2-verifikat under kontantmetoden (#150), som aldrig vänds.
    pub t2: bool,
}

/// Varför ett verifikat inte kan rättas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rättelsespärr {
    /// Verifikatet är redan rättat, av rättelsen med numret.
    RedanRättat(String),
    /// Verifikatet är en rättelse av verifikatet med numret.
    ÄrRättelse(String),
    /// Verifikatet kommer från ett av Mayas flöden, fakturan i texten, och rättas där.
    FrånFlöde(String),
    /// Verifikatet är T2-verifikatet, som inte vänds i det nya året (#150).
    T2,
}

impl Rättelsespärr {
    pub fn kod(&self) -> &'static str {
        match self {
            Rättelsespärr::RedanRättat(_) => "redan_rattat",
            Rättelsespärr::ÄrRättelse(_) => "ar_rattelse",
            Rättelsespärr::FrånFlöde(_) => "fran_flode",
            Rättelsespärr::T2 => "t2_vands_inte",
        }
    }
}

impl fmt::Display for Rättelsespärr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rättelsespärr::RedanRättat(rättelse) => write!(
                f,
                "Redan rättat av {rättelse}. Ett verifikat kan bara rättas en gång."
            ),
            Rättelsespärr::ÄrRättelse(original) => write!(
                f,
                "Det här är en rättelse av {original} och kan inte rättas. Blev något fel, \
                 registrera ett nytt verifikat."
            ),
            Rättelsespärr::FrånFlöde(faktura) => write!(
                f,
                "Verifikatet bokfördes från {faktura}. Rättas i fakturan."
            ),
            Rättelsespärr::T2 => f.write_str(
                "T2-verifikatet vänds inte i det nya året. Fakturorna i det betalas mot 1510 \
                 och 2440.",
            ),
        }
    }
}

/// Beskrivningen av rättelsen: "Rättelse av V5" eller "Rättelse av V5, <orsak>".
pub fn beskrivning(nummer: &str, orsak: &str) -> String {
    match orsak.trim() {
        "" => format!("Rättelse av {nummer}"),
        orsak => format!("Rättelse av {nummer}, {orsak}"),
    }
}

/// Raderna med debet och kredit bytta, i samma ordning och med samma text.
pub fn vänd(rader: &[Rad]) -> Vec<Rad> {
    rader
        .iter()
        .map(|r| Rad {
            konto: r.konto,
            debet: r.kredit,
            kredit: r.debet,
            text: r.text.clone(),
        })
        .collect()
}

impl Original {
    /// Om verifikatet kan rättas: det är inte redan rättat och inte självt en rättelse.
    pub fn kan_rättas(&self) -> Result<(), Rättelsespärr> {
        if let Some(faktura) = &self.flöde {
            return Err(Rättelsespärr::FrånFlöde(faktura.clone()));
        }
        self.kan_rättas_i_flödet()
    }

    /// Om verifikatet kan rättas inifrån sitt fakturaflöde (Rätta kontering, avskrift,
    /// registrering eller betalning): som [`Original::kan_rättas`], utan spärren mot verifikat
    /// från ett flöde.
    pub fn kan_rättas_i_flödet(&self) -> Result<(), Rättelsespärr> {
        if self.t2 {
            return Err(Rättelsespärr::T2);
        }
        if let Some(original) = &self.rättar {
            return Err(Rättelsespärr::ÄrRättelse(original.clone()));
        }
        if let Some(rättelse) = &self.rättad_av {
            return Err(Rättelsespärr::RedanRättat(rättelse.clone()));
        }
        Ok(())
    }

    /// Prövar rättelsen daterad `datum` med den valfria `orsak`en. Spärrarna har samma koder som
    /// formuläret för ett verifikat, och `fore_originalet` när datumet är före originalets.
    /// Förutsätter att [`Original::kan_rättas`] gäller.
    pub fn rättelse(
        &self,
        datum: &str,
        orsak: &str,
        förutsättningar: &Förutsättningar,
    ) -> Result<BokförbartVerifikat, Vec<Spärr>> {
        if let Ok(d) = datum.trim().parse::<Date>()
            && d < self.datum
        {
            return Err(vec![Spärr {
                fält: Fält::Datum,
                kod: "fore_originalet",
                text: format!("Tidigast {} (originalets datum).", self.datum),
            }]);
        }
        // En rättelse vänder originalets rader, också på konton som har inaktiverats sedan
        // originalet bokfördes (R1 kräver inga aktiva konton).
        let mut förutsättningar = förutsättningar.clone();
        for (konto, aktiv) in &mut förutsättningar.konton {
            if self.rader.iter().any(|r| r.konto == *konto) {
                *aktiv = true;
            }
        }
        let text = |b: Belopp| {
            if b == Belopp::NOLL {
                String::new()
            } else {
                b.to_string()
            }
        };
        NyttVerifikat {
            datum: datum.to_string(),
            beskrivning: beskrivning(&self.nummer, orsak),
            rader: vänd(&self.rader)
                .into_iter()
                .map(|r| NyRad {
                    konto: r.konto.to_string(),
                    debet: text(r.debet),
                    kredit: text(r.kredit),
                    text: r.text.unwrap_or_default(),
                })
                .collect(),
        }
        .kontrollera(&förutsättningar)
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;

    use super::*;
    use crate::kontoplan::Kontonummer;
    use crate::rakenskapsar::Räkenskapsår;

    fn konto(nr: u16) -> Kontonummer {
        Kontonummer::new(nr).unwrap()
    }

    fn original() -> Original {
        Original {
            nummer: "V5".into(),
            datum: date(2026, 10, 2),
            rader: vec![
                Rad {
                    konto: konto(6212),
                    debet: Belopp::från_öre(59_900),
                    kredit: Belopp::NOLL,
                    text: Some("Telia".into()),
                },
                Rad {
                    konto: konto(1930),
                    debet: Belopp::NOLL,
                    kredit: Belopp::från_öre(59_900),
                    text: None,
                },
            ],
            rättad_av: None,
            rättar: None,
            flöde: None,
            t2: false,
        }
    }

    #[test]
    fn t2_verifikatet_vänds_aldrig_varken_för_hand_eller_i_ett_flöde() {
        let t2 = Original {
            t2: true,
            ..original()
        };
        assert_eq!(t2.kan_rättas().unwrap_err().kod(), "t2_vands_inte");
        assert_eq!(t2.kan_rättas_i_flödet().unwrap_err(), Rättelsespärr::T2);
    }

    fn förutsättningar() -> Förutsättningar {
        Förutsättningar {
            konton: vec![(konto(6212), true), (konto(1930), true)],
            räkenskapsår: vec![Räkenskapsår::befintligt(
                date(2026, 1, 1),
                date(2026, 12, 31),
            )],
        }
    }

    #[test]
    fn beskrivningen_följer_mallen_med_och_utan_orsak() {
        assert_eq!(beskrivning("V5", ""), "Rättelse av V5");
        assert_eq!(beskrivning("V5", "  "), "Rättelse av V5");
        assert_eq!(
            beskrivning("V5", " fel konto "),
            "Rättelse av V5, fel konto"
        );
    }

    #[test]
    fn rättelsen_vänder_raderna_och_behåller_texten() {
        let r = original()
            .rättelse("2026-10-18", "fel konto", &förutsättningar())
            .unwrap();
        assert_eq!(r.beskrivning, "Rättelse av V5, fel konto");
        assert_eq!(r.datum, date(2026, 10, 18));
        assert_eq!(r.rader, vänd(&original().rader));
        assert_eq!(r.rader[0].kredit, Belopp::från_öre(59_900));
        assert_eq!(r.rader[0].text.as_deref(), Some("Telia"));
        assert_eq!(r.rader[1].debet, Belopp::från_öre(59_900));
    }

    #[test]
    fn datum_före_originalets_nekas() {
        let spärrar = original()
            .rättelse("2026-10-01", "", &förutsättningar())
            .unwrap_err();
        assert_eq!(spärrar.len(), 1);
        assert_eq!(spärrar[0].kod, "fore_originalet");
        assert_eq!(spärrar[0].fält, Fält::Datum);
        assert!(
            original()
                .rättelse("2026-10-02", "", &förutsättningar())
                .is_ok()
        );
    }

    #[test]
    fn rättelsen_prövas_med_samma_spärrar_som_ett_verifikat() {
        let spärrar = original()
            .rättelse("2028-01-01", "", &förutsättningar())
            .unwrap_err();
        assert_eq!(spärrar[0].kod, "saknat_rakenskapsar");
        let spärrar = original().rättelse("", "", &förutsättningar()).unwrap_err();
        assert_eq!(spärrar[0].kod, "datum_saknas");
    }

    #[test]
    fn ett_rättat_verifikat_och_en_rättelse_kan_inte_rättas() {
        assert_eq!(original().kan_rättas(), Ok(()));
        let rättat = Original {
            rättad_av: Some("V7".into()),
            ..original()
        };
        let spärr = rättat.kan_rättas().unwrap_err();
        assert_eq!(spärr.kod(), "redan_rattat");
        assert_eq!(
            spärr.to_string(),
            "Redan rättat av V7. Ett verifikat kan bara rättas en gång."
        );
        let rättelse = Original {
            rättar: Some("V3".into()),
            ..original()
        };
        assert_eq!(rättelse.kan_rättas().unwrap_err().kod(), "ar_rattelse");
    }

    #[test]
    fn ett_verifikat_från_en_faktura_rättas_i_fakturan() {
        let från_flöde = Original {
            flöde: Some("leverantörsfaktura 8812 från Kontorsvaror AB".into()),
            ..original()
        };
        let spärr = från_flöde.kan_rättas().unwrap_err();
        assert_eq!(spärr.kod(), "fran_flode");
        assert_eq!(
            spärr.to_string(),
            "Verifikatet bokfördes från leverantörsfaktura 8812 från Kontorsvaror AB. Rättas i \
             fakturan."
        );
    }

    #[test]
    fn i_fakturan_rättas_verifikatet_en_gång_och_aldrig_en_rättelse() {
        let flöde = Some("kundfaktura 1 till Kund AB".to_string());
        let från_flöde = Original {
            flöde: flöde.clone(),
            ..original()
        };
        assert_eq!(från_flöde.kan_rättas_i_flödet(), Ok(()));
        let rättat = Original {
            flöde: flöde.clone(),
            rättad_av: Some("V6".into()),
            ..original()
        };
        assert_eq!(
            rättat.kan_rättas_i_flödet().unwrap_err().kod(),
            "redan_rattat"
        );
        let rättelse = Original {
            flöde,
            rättar: Some("V4".into()),
            ..original()
        };
        assert_eq!(
            rättelse.kan_rättas_i_flödet().unwrap_err().kod(),
            "ar_rattelse"
        );
    }
}
