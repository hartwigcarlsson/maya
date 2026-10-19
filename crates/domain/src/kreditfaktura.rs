//! Kreditfakturor till kund (#140): *Kreditfaktura…* på en utfärdad faktura väljer hela
//! fakturan eller ett antal per rad och skapar ett utkast med negativa rader och en hänvisning
//! till originalet. Utkastet är en vanlig [`NyKundfaktura`] med [`NyKundfaktura::krediterar`]
//! satt, och utfärdas med samma regler och i samma fakturaserie som en faktura.
//!
//! En kreditrad har originalradens beskrivning, à-pris, rabatt, momssats och intäktskonto (det
//! som gäller nu, efter en Rätta kontering) och ett negativt antal, så att radens netto och
//! momsen per sats blir negativa och utfärdandets kontering speglar originalet: intäktskontona
//! och utgående moms i debet, 1510 i kredit. Krediteras ett antal per rad blir beloppen
//! proportionella mot det krediterade antalet.
//!
//! Kreditfakturan får originalets momsläge, också om företagets momsregistrering eller kundens
//! förval har ändrats sedan, och originalets kund som den kopierades in. Den spärras om summan
//! inte är negativ eller om den krediterar mer än det som finns kvar att kreditera: originalets
//! total minus de kreditfakturor som redan är utfärdade.

use jiff::civil::Date;

use crate::belopp::Saldo;
use crate::faktura::{Antal, Rabatt};
use crate::kundfaktura::{Fakturakund, Fakturarad, Momsläge, NyFakturarad, NyKundfaktura};

/// Originalet som en kreditfaktura krediterar, läst när utkastet läses eller utfärdas.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Hänvisning {
    /// Originalets id.
    pub faktura: i64,
    /// Originalets nummer i fakturaserien.
    pub nummer: u64,
    /// Originalets fakturadatum, `ÅÅÅÅ-MM-DD`.
    pub fakturadatum: String,
    /// Originalets momsläge, som kreditfakturan får.
    pub momsläge: Momsläge,
    /// Kunden som den kopierades in på originalet.
    pub kund: Fakturakund,
    /// Originalets total i öre.
    pub total: i64,
    /// Det som finns kvar att kreditera i öre: totalen minus de utfärdade kreditfakturorna.
    pub kvar: i64,
}

impl Hänvisning {
    /// Hänvisningen som fakturan skriver den: "Krediterar faktura 1007 daterad 2026-03-10".
    pub fn text(&self) -> String {
        format!(
            "Krediterar faktura {} daterad {}",
            self.nummer, self.fakturadatum
        )
    }
}

/// Valet i panelen Kreditfaktura…: hela fakturan, eller antalet att kreditera per rad i
/// originalets ordning.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NyKreditering {
    pub hela: bool,
    /// Antalet att kreditera per rad, positivt; tom text är 0.
    pub antal: Vec<String>,
}

/// Spärrarna när kreditfakturans utkast skapas, som Systemdokumentationen återger (S2).
pub const KREDITERINGSSPÄRRAR: &[(&str, &str)] = &[
    ("redan_krediterad", "Fakturan har något kvar att kreditera."),
    (
        "kredit_antal_ogiltigt",
        "Antalet att kreditera på en rad är från 0 till radens antal, med högst två decimaler.",
    ),
    ("kredit_saknas", "Minst en rad krediteras."),
    (
        "kredit_overstiger",
        "Kreditfakturan krediterar högst det som finns kvar att kreditera på originalet.",
    ),
];

/// Något som hindrar att utkastet skapas: vid en rad, eller för valet som helhet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kreditspärr {
    pub rad: Option<usize>,
    pub kod: &'static str,
    pub text: String,
}

impl NyKreditering {
    /// Prövar valet mot originalets rader och skapar kreditfakturans utkast, daterat `idag`
    /// med förfallodag samma dag.
    pub fn utkast(
        &self,
        h: &Hänvisning,
        rader: &[Fakturarad],
        idag: Date,
    ) -> Result<NyKundfaktura, Vec<Kreditspärr>> {
        let mut spärrar = Vec::new();
        if h.kvar <= 0 {
            spärrar.push(Kreditspärr {
                rad: None,
                kod: "redan_krediterad",
                text: format!("Faktura {} är redan krediterad.", h.nummer),
            });
            return Err(spärrar);
        }
        let mut antal = Vec::new();
        for (i, r) in rader.iter().enumerate() {
            let text = if self.hela {
                r.antal.to_string()
            } else {
                self.antal.get(i).cloned().unwrap_or_default()
            };
            let a = match text.trim() {
                "" => Some(Antal::från_hundradelar(0)),
                t => Antal::tolka(t),
            };
            match a {
                Some(a) if (0..=r.antal.hundradelar()).contains(&a.hundradelar()) => antal.push(a),
                _ => spärrar.push(Kreditspärr {
                    rad: Some(i),
                    kod: "kredit_antal_ogiltigt",
                    text: format!("Ange ett antal från 0 till {}.", r.antal),
                }),
            }
        }
        if !spärrar.is_empty() {
            return Err(spärrar);
        }
        let kreditrader: Vec<NyFakturarad> = rader
            .iter()
            .zip(&antal)
            .filter(|(_, a)| a.hundradelar() > 0)
            .map(|(r, a)| kreditrad(r, *a))
            .collect();
        if kreditrader.is_empty() {
            return Err(vec![Kreditspärr {
                rad: None,
                kod: "kredit_saknas",
                text: "Välj minst en rad att kreditera.".into(),
            }]);
        }
        let utkast = NyKundfaktura {
            kund: h.kund.id.to_string(),
            fakturadatum: idag.to_string(),
            förfallodag: idag.to_string(),
            omvänd_skattskyldighet: h.momsläge.omvänd().kod().to_string(),
            rader: kreditrader,
            krediterar: Some(h.clone()),
            ..NyKundfaktura::default()
        };
        let total = utkast.beräkna_med(h.momsläge).total;
        if -total > h.kvar {
            return Err(vec![överstiger(h)]);
        }
        Ok(utkast)
    }
}

/// Spärren när kreditfakturan krediterar mer än det som finns kvar.
pub(crate) fn överstiger(h: &Hänvisning) -> Kreditspärr {
    Kreditspärr {
        rad: None,
        kod: "kredit_overstiger",
        text: format!(
            "Kreditfakturan krediterar mer än det som finns kvar att kreditera på faktura {}, \
             {} kr.",
            h.nummer,
            Saldo::från_öre(h.kvar)
        ),
    }
}

/// Originalraden krediterad med `antal`: samma rad med negativt antal.
fn kreditrad(r: &Fakturarad, antal: Antal) -> NyFakturarad {
    NyFakturarad {
        beskrivning: r.beskrivning.clone(),
        antal: Antal::från_hundradelar(-antal.hundradelar()).to_string(),
        enhet: r.enhet.clone(),
        à_pris: crate::belopp::formulärtext(r.à_pris),
        rabatt: if r.rabatt == Rabatt::INGEN {
            String::new()
        } else {
            r.rabatt.to_string()
        },
        momssats: r.momssats.procent().to_string(),
        konto: r.konto.to_string(),
    }
}
