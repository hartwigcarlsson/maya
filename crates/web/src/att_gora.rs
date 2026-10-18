//! Att göra, företagets startsida: agendan enligt variant C i `prototype/47-att-gora`, som skal.
//! Överst dagens datum, en sammanfattning och nyckeltalen ur bokföringen. Till vänster
//! tidslinjen med det som har ett datum: i etapp 1 bara utkast med kontantfrist, på fristens
//! dag eller som försenade. Till höger spalten Utan datum med utkasten per period
//! (50-dagarsvarningen i periodens rubrik) och de okopplade underlagen.
//!
//! Ett kort fälls ut på plats: ett utkast med verifikatformuläret, där Bokför går vidare till
//! nästa utkast, och ett underlag med "Vad är det här?". Sakernas sorter kommer från
//! `maya_core::att_gora::Sak`; en ny sort får ett eget kort här ([`Sort`]).
//!
//! Överst, före allt annat, varningen efter en återställning med `maya restore`, tills
//! användaren kvitterar den.
//!
//! Sidan läser sin data med `sidodata::sida`, så korten fälls ut i routern på klienten.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app::LoggaIn;
use crate::foretag::{Företagsval, IngenÅtkomst};
use crate::sidodata::sida;
use crate::underlag::{Förhandsvisning, Gem, UnderlagVy};
use crate::verifikat::{
    DokumentVy, Formulär, FormulärData, UtkastBadge, företagets_bas, inloggad
};

/// Det som Att göra visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttGöradata {
    /// Dagens datum med veckodag.
    pub dag: String,
    /// Varningarna efter en återställning som användaren inte har kvitterat.
    pub återställningar: Vec<ÅterställningVy>,
    pub försenade: usize,
    pub inom_sju_dagar: usize,
    pub utan_datum: usize,
    /// Det som är fel i driften, överst. Tomt när allt fungerar.
    pub driftvarningar: Vec<DriftvarningVy>,
    pub nyckeltal: Vec<NyckeltalVy>,
    pub tidslinje: Vec<DagVy>,
    pub utkast: Vec<PeriodVy>,
    pub underlag: Vec<KortVy>,
    /// Kortet som är utfällt och det som visas i det.
    pub öppet: Option<(String, Öppet)>,
    pub notis: Option<String>,
    pub fel: Option<String>,
}

/// Varningen efter en återställning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ÅterställningVy {
    pub id: String,
    pub text: String,
}

/// En driftvarning: samma i alla företag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftvarningVy {
    pub rubrik: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NyckeltalVy {
    pub namn: String,
    pub värde: String,
    pub under: String,
}

/// En dag i tidslinjen, eller Försenat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagVy {
    pub rubrik: String,
    /// "i dag", "om 3 dagar" eller tomt för Försenat.
    pub avstånd: String,
    pub försenat: bool,
    pub idag: bool,
    pub kort: Vec<KortVy>,
}

/// En period i spalten Utan datum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodVy {
    pub rubrik: String,
    /// 50-dagarsvarningen, t.ex. "slut för 75 dagar sedan".
    pub varning: Option<String>,
    pub kort: Vec<KortVy>,
}

/// Sorten av sak som ett kort visar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sort {
    Utkast,
    Underlag,
}

/// Ett kort i agendan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KortVy {
    /// `utkast-<id>` eller `underlag-<id>`: adressens `oppna` och kortets id.
    pub nyckel: String,
    pub sort: Sort,
    pub rubrik: String,
    pub belopp: Option<String>,
    /// Fristen eller åldern.
    pub när: String,
    /// `""`, `"hot"` (snart eller länge) eller `"bad"` (försenat).
    pub när_klass: String,
    /// Utkastet har kopplade underlag.
    pub gem: bool,
}

/// Det som ett utfällt kort visar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Öppet {
    Utkast {
        dokument: Box<DokumentVy>,
        formulär: FormulärData,
        okopplade: Vec<UnderlagVy>,
    },
    Underlag {
        underlag: Box<UnderlagVy>,
        /// Bokförda verifikat att komplettera: id och text.
        verifikat: Vec<(String, String)>,
        /// Frågan om underlaget ska tas bort visas.
        ta_bort: bool,
    },
}

/// Datan för `/foretag/<id>`.
#[server(prefix = "/api", endpoint = "att_gora", input = server_fn::codec::GetUrl)]
pub async fn hämta_att_göra(adress: String) -> Result<AttGöradata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::att_göra(&begäran).await,
        None => Ok(AttGöradata::default()),
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use crate::svar::{ingen_åtkomst, serverfel};
    use std::sync::Arc;

    use axum::extract::{Path, State};

    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use jiff::civil::Date;
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::att_gora::{self, Sak, Utkastsak, ÅterställningsId, Återställningsvarning};
    use maya_core::config::Config;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::underlag::{self, Underlag, UnderlagFel, UnderlagId};
    use maya_core::verifikat::{self, Urval, VerifikatFel, VerifikatId};
    use maya_core::{Databas, Klocka};
    use maya_domain::att_gora::{
        Frist, dag_lång, dagar_mellan, länge_okopplat, ordna_utkast, ålder,
    };
    use maya_domain::belopp::Saldo;
    use maya_domain::rakenskapsar::periodnamn;
    use maya_domain::verifikat::NyttVerifikat;
    use serde::Deserialize;

    use super::{
        AttGöradata, DagVy, DriftvarningVy, KortVy, NyckeltalVy, PeriodVy, Sort, ÅterställningVy,
        Öppet,
    };
    use crate::sidodata::{self, Sidbegäran};
    use crate::underlag::server::vy;

    #[derive(Clone)]
    pub(crate) struct AttGöraState {
        pub config: Arc<Config>,
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: AttGöraState) -> Router {
        Router::new()
            .route("/foretag/{id}/att-gora/underlag/{u}/kvitto", post(kvitto))
            // #145 Vad är det här? → Leverantörsfaktura
            .route(
                "/foretag/{id}/att-gora/underlag/{u}/leverantorsfaktura",
                post(leverantörsfaktura),
            )
            .route(
                "/foretag/{id}/att-gora/underlag/{u}/komplettera",
                post(komplettera),
            )
            .route("/foretag/{id}/att-gora/underlag/{u}/ta-bort", post(ta_bort))
            .route(
                "/foretag/{id}/att-gora/aterstallning/{a}/kvittera",
                post(kvittera),
            )
            .with_state(Arc::new(state))
    }

    /// Hämtar det som Att göra visar, för `/foretag/<id>`. Utan åtkomst blir datan tom; sidan
    /// visar det ur företagsvalet.
    pub(crate) async fn att_göra(b: &Sidbegäran) -> Result<AttGöradata, ServerFnError> {
        let delar = b.delar();
        let ["foretag", id] = delar.as_slice() else {
            return Ok(AttGöradata::default());
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Ok(AttGöradata::default());
        };
        let query: Vec<(String, String)> = url::form_urlencoded::parse(b.fråga().as_bytes())
            .into_owned()
            .collect();
        let state = AttGöraState {
            config: b.config.clone(),
            db: b.db.clone(),
            klocka: b.klocka.clone(),
        };
        match hämta(&state, &b.inloggad, företag, &query).await {
            Ok(data) => Ok(data),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                Ok(AttGöradata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    async fn hämta(
        state: &AttGöraState,
        inloggad: &Inloggad,
        företag: FöretagsId,
        query: &[(String, String)],
    ) -> Result<AttGöradata, ÅtkomstFel> {
        let fråga = |namn: &str| {
            query
                .iter()
                .find(|(k, _)| k == namn)
                .map(|(_, v)| v.as_str())
        };
        let (db, aktör) = (&state.db, &inloggad.aktör);
        let a = att_gora::att_göra(db, &state.klocka, &state.config, aktör, företag).await?;
        let idag = a.idag;
        let mut driftvarningar = Vec::new();
        let mut utkast = Vec::new();
        let mut underlagen = Vec::new();
        let mut återställningar = Vec::new();
        for sak in a.saker {
            match sak {
                Sak::Återställning(v) => återställningar.push(återställning(&v)),
                Sak::Driftvarning(v) => driftvarningar.push(DriftvarningVy {
                    rubrik: v.rubrik().into(),
                    text: v.text(),
                }),
                Sak::Utkast(u) => utkast.push(u),
                Sak::Underlag(u) => underlagen.push(u),
            }
        }
        let antal_utkast = utkast.len();
        let ordning = ordna_utkast(utkast, Utkastsak::läge, idag);
        // Ordningen som Bokför går vidare i: tidslinjen och sedan perioderna.
        let i_ordning: Vec<&Utkastsak> = ordning
            .kontanta
            .iter()
            .map(|(_, u)| u)
            .chain(ordning.perioder.iter().flat_map(|p| p.utkast.iter()))
            .collect();

        let mut notis = None;
        let mut öppna = fråga("oppna").map(str::to_string);
        if let Some(id) = fråga("bokford").and_then(|v| v.parse::<VerifikatId>().ok())
            && let Some(v) = verifikat::verifikatet(db, aktör, företag, id).await?
            && let Some(nummer) = v.nummer
        {
            notis = Some(format!("Bokförd som {nummer}. {antal_utkast} utkast kvar."));
            if öppna.is_none() {
                öppna = i_ordning.first().map(|u| format!("utkast-{}", u.id));
            }
        } else if fråga("sparat").is_some() {
            notis = Some("Utkastet sparades.".into());
        } else if fråga("kvitto").is_some() {
            notis = Some("Ett verifikatutkast skapades med underlaget kopplat.".into());
        } else if let Some(v) = fråga("kompletterat") {
            notis = Some(format!(
                "{v} kompletterades med underlaget. Kompletteringen loggades och kan inte tas bort."
            ));
        } else if fråga("borttaget").is_some() {
            notis = Some("Underlaget togs bort, och borttagningen loggades.".into());
        } else if fråga("kvitterad").is_some() {
            notis = Some("Varningen är kvitterad.".into());
        }
        let fel = fråga("fel")
            .and_then(|kod| match kod {
                "inget_verifikat" => Some("Välj ett bokfört verifikat att komplettera."),
                kod => crate::underlag::feltext(kod),
            })
            .map(str::to_string);

        let kort_för_utkast = |u: &Utkastsak| {
            let (när, klass) = match u.frist(idag) {
                Frist::Kontantfrist { senast, försenad } => {
                    let dagar = dagar_mellan(idag, senast);
                    if försenad {
                        (
                            format!("kontantfrist {}, {} d sedan", senast, -dagar),
                            "bad",
                        )
                    } else if dagar == 0 {
                        ("kontantfrist i dag".to_string(), "hot")
                    } else {
                        (format!("kontantfrist {}", dag_lång(senast)), "hot")
                    }
                }
                _ => (ålder(tid_datum(u.skapad), idag), ""),
            };
            KortVy {
                nyckel: format!("utkast-{}", u.id),
                sort: Sort::Utkast,
                rubrik: if u.beskrivning.trim().is_empty() {
                    "Utkast utan beskrivning".into()
                } else {
                    u.beskrivning.clone()
                },
                belopp: Some(u.summa.to_string()),
                när,
                när_klass: klass.into(),
                gem: u.underlag > 0,
            }
        };

        let mut tidslinje: Vec<DagVy> = Vec::new();
        let mut försenade = 0;
        let mut inom_sju_dagar = 0;
        for (senast, u) in &ordning.kontanta {
            let försenat = *senast < idag;
            let dagar = dagar_mellan(idag, *senast);
            if försenat {
                försenade += 1;
            } else if dagar <= 7 {
                inom_sju_dagar += 1;
            }
            let rubrik = if försenat {
                "Försenat".to_string()
            } else {
                stor_bokstav(&dag_lång(*senast))
            };
            let kort = kort_för_utkast(u);
            match tidslinje.last_mut() {
                Some(d) if d.rubrik == rubrik => d.kort.push(kort),
                _ => tidslinje.push(DagVy {
                    rubrik,
                    avstånd: match dagar {
                        _ if försenat => String::new(),
                        0 => "i dag".into(),
                        1 => "i morgon".into(),
                        n => format!("om {n} dagar"),
                    },
                    försenat,
                    idag: dagar == 0,
                    kort: vec![kort],
                }),
            }
        }
        let perioder: Vec<PeriodVy> = ordning
            .perioder
            .iter()
            .map(|p| PeriodVy {
                rubrik: p
                    .månad
                    .map(|m| stor_bokstav(&periodnamn(m)))
                    .unwrap_or_else(|| "Utan verifikatdatum".into()),
                varning: p.varning.map(|d| format!("slut för {d} dagar sedan")),
                kort: p.utkast.iter().map(kort_för_utkast).collect(),
            })
            .collect();
        let underlag: Vec<KortVy> = underlagen
            .iter()
            .map(|u| {
                let uppladdad = tid_datum(u.uppladdad.tid);
                let länge = länge_okopplat(uppladdad, idag);
                KortVy {
                    nyckel: format!("underlag-{}", u.id),
                    sort: Sort::Underlag,
                    rubrik: u.filnamn.clone(),
                    belopp: None,
                    när: if länge {
                        format!("{} · 30+ dagar", ålder(uppladdad, idag))
                    } else {
                        ålder(uppladdad, idag)
                    },
                    när_klass: if länge { "hot" } else { "" }.into(),
                    gem: false,
                }
            })
            .collect();
        let utan_datum = perioder.iter().map(|p| p.kort.len()).sum::<usize>() + underlag.len();

        let öppet = match öppna.as_deref().and_then(|n| n.split_once('-')) {
            Some(("utkast", id)) => öppet_utkast(state, inloggad, företag, id).await?,
            Some(("underlag", id)) => {
                let ta_bort = fråga("ta_bort").is_some();
                öppet_underlag(state, inloggad, företag, &underlagen, id, ta_bort).await?
            }
            _ => None,
        }
        .map(|ö| (öppna.unwrap_or_default(), ö));

        Ok(AttGöradata {
            dag: stor_bokstav(&dag_lång(idag)),
            återställningar,
            försenade,
            inom_sju_dagar,
            utan_datum,
            driftvarningar,
            nyckeltal: nyckeltal(&a.nyckeltal),
            tidslinje,
            utkast: perioder,
            underlag,
            öppet,
            notis,
            fel,
        })
    }

    fn återställning(v: &Återställningsvarning) -> ÅterställningVy {
        let tid = |t: jiff::Timestamp| maya_domain::tid::minut(t);
        let backup = tid(v.backup_tid);
        ÅterställningVy {
            id: v.id.to_string(),
            text: format!(
                "Installationen återställdes {} ur backupen från {backup}. Det som \
                 registrerades efter {backup} finns inte längre i Maya och måste registreras om.",
                tid(v.tid)
            ),
        }
    }

    fn nyckeltal(n: &att_gora::Nyckeltal) -> Vec<NyckeltalVy> {
        let kronor = |s: Saldo| format!("{s} kr");
        vec![
            NyckeltalVy {
                namn: "Företagskontot".into(),
                värde: kronor(n.företagskontot),
                under: "1930 per i dag · ur bokföringen".into(),
            },
            NyckeltalVy {
                namn: "Resultat hittills i år".into(),
                värde: kronor(n.resultat_i_år),
                under: match n.år {
                    Some(år) => format!("{} – {} · ur bokföringen", år.start(), år.slut()),
                    None => "inget räkenskapsår i dag · ur bokföringen".into(),
                },
            },
        ]
    }

    /// Ett utkast med formuläret, som i fliken Verifikat.
    async fn öppet_utkast(
        state: &AttGöraState,
        inloggad: &Inloggad,
        företag: FöretagsId,
        id: &str,
    ) -> Result<Option<Öppet>, ÅtkomstFel> {
        let data = crate::verifikat::server::hämta(
            &state.db,
            state.klocka.idag(),
            inloggad,
            företag,
            Urval::Utkast,
            Some(id.to_string()),
            false,
            None,
        )
        .await?;
        Ok(match (data.dokument, data.formulär) {
            (Some(dokument), Some(mut formulär)) if dokument.nummer.is_none() => {
                formulär.idag = state.klocka.idag().to_string();
                Some(Öppet::Utkast {
                    dokument: Box::new(dokument),
                    formulär,
                    okopplade: data.okopplade,
                })
            }
            _ => None,
        })
    }

    /// Ett okopplat underlag med "Vad är det här?".
    async fn öppet_underlag(
        state: &AttGöraState,
        inloggad: &Inloggad,
        företag: FöretagsId,
        okopplade: &[Underlag],
        id: &str,
        ta_bort: bool,
    ) -> Result<Option<Öppet>, ÅtkomstFel> {
        let Some(u) = okopplade.iter().find(|u| u.id.to_string() == id) else {
            return Ok(None);
        };
        let verifikat = verifikat::lista(&state.db, &inloggad.aktör, företag, Urval::Bokförda)
            .await?
            .into_iter()
            .map(|v| {
                let nummer = v.nummer.map(|n| n.to_string()).unwrap_or_default();
                let datum = v.datum.map(|d| d.to_string()).unwrap_or_default();
                (
                    v.id.to_string(),
                    format!("{nummer} · {datum} · {} · {} kr", v.beskrivning, v.summa),
                )
            })
            .collect();
        Ok(Some(Öppet::Underlag {
            underlag: Box::new(vy(u)),
            verifikat,
            ta_bort,
        }))
    }

    fn tid_datum(t: jiff::Timestamp) -> Date {
        maya_domain::tid::datum(t)
    }

    fn stor_bokstav(text: &str) -> String {
        let mut tecken = text.chars();
        match tecken.next() {
            Some(c) => c.to_uppercase().chain(tecken).collect(),
            None => String::new(),
        }
    }

    type Underlagsväg = Path<(String, String)>;

    fn tolka(id: &str, u: &str) -> Option<(FöretagsId, UnderlagId)> {
        Some((id.parse().ok()?, u.parse().ok()?))
    }

    /// Kvitto: ett nytt, tomt verifikatutkast med underlaget kopplat, utfällt i Att göra.
    async fn kvitto(
        State(state): State<Arc<AttGöraState>>,
        Path((id, u)): Underlagsväg,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, u)) = tolka(&id, &u) else {
            return ingen_åtkomst();
        };
        match verifikat::spara_utkast_med_underlag(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            None,
            &NyttVerifikat::default(),
            &[u],
        )
        .await
        {
            Ok(utkast) => Redirect::to(&format!(
                "/foretag/{företag}?oppna=utkast-{utkast}&kvitto=1"
            ))
            .into_response(),
            Err(VerifikatFel::OkäntUnderlag) => {
                Redirect::to(&format!("/foretag/{företag}?fel=okant_underlag")).into_response()
            }
            Err(VerifikatFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    /// Leverantörsfaktura: ett utkast med underlaget som original, öppnat i fliken
    /// Leverantörsfakturor med originalet till vänster.
    async fn leverantörsfaktura(
        State(state): State<Arc<AttGöraState>>,
        Path((id, u)): Underlagsväg,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, u)) = tolka(&id, &u) else {
            return ingen_åtkomst();
        };
        match maya_core::leverantorsfaktura::skapa_utkast(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            u,
        )
        .await
        {
            Ok(f) => Redirect::to(&format!(
                "/foretag/{företag}/leverantorsfakturor?f={f}&skapat=1"
            ))
            .into_response(),
            Err(maya_core::leverantorsfaktura::FakturaFel::InteOkopplat) => {
                Redirect::to(&format!("/foretag/{företag}?fel=okant_underlag")).into_response()
            }
            Err(maya_core::leverantorsfaktura::FakturaFel::Åtkomst(
                ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte,
            )) => ingen_åtkomst(),
            Err(e) => serverfel(e),
        }
    }

    #[derive(Deserialize)]
    struct Kompletteringsval {
        #[serde(default)]
        verifikat: String,
    }

    /// Komplettering: underlaget kopplas till ett bokfört verifikat (VF6).
    async fn komplettera(
        State(state): State<Arc<AttGöraState>>,
        Path((id, u)): Underlagsväg,
        inloggad: Option<Extension<Inloggad>>,
        Form(val): Form<Kompletteringsval>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, u)) = tolka(&id, &u) else {
            return ingen_åtkomst();
        };
        let tillbaka = format!("/foretag/{företag}?oppna=underlag-{u}");
        let Ok(v) = val.verifikat.parse::<VerifikatId>() else {
            return Redirect::to(&format!("{tillbaka}&fel=inget_verifikat")).into_response();
        };
        let (db, aktör) = (&state.db, &inloggad.aktör);
        match underlag::komplettera(db, &state.klocka, aktör, företag, v, u).await {
            Ok(()) => {
                let nummer = match verifikat::verifikatet(db, aktör, företag, v).await {
                    Ok(Some(v)) => v.nummer.map(|n| n.to_string()).unwrap_or_default(),
                    _ => String::new(),
                };
                Redirect::to(&format!("/foretag/{företag}?kompletterat={nummer}")).into_response()
            }
            Err(UnderlagFel::IngetBokförtVerifikat) => {
                Redirect::to(&format!("{tillbaka}&fel=inget_verifikat")).into_response()
            }
            Err(UnderlagFel::RedanKopplat) => {
                Redirect::to(&format!("{tillbaka}&fel=redan_kopplat")).into_response()
            }
            Err(e) => crate::underlag::server::underlagsfel(e),
        }
    }

    /// Dubblett eller fel fil: underlaget tas bort, och borttagningen loggas.
    async fn ta_bort(
        State(state): State<Arc<AttGöraState>>,
        Path((id, u)): Underlagsväg,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, u)) = tolka(&id, &u) else {
            return ingen_åtkomst();
        };
        match underlag::ta_bort(&state.db, &state.klocka, &inloggad.aktör, företag, u).await {
            Ok(()) => Redirect::to(&format!("/foretag/{företag}?borttaget=1")).into_response(),
            Err(UnderlagFel::HörTillBokfört) => Redirect::to(&format!(
                "/foretag/{företag}?oppna=underlag-{u}&fel=hor_till_bokfort"
            ))
            .into_response(),
            Err(e) => crate::underlag::server::underlagsfel(e),
        }
    }

    /// Kvitterar varningen efter en återställning för den inloggade användaren.
    async fn kvittera(
        State(state): State<Arc<AttGöraState>>,
        Path((id, a)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let (Ok(företag), Ok(a)) = (id.parse::<FöretagsId>(), a.parse::<ÅterställningsId>())
        else {
            return ingen_åtkomst();
        };
        match att_gora::kvittera_återställning(&state.db, &state.klocka, &inloggad.aktör, a).await
        {
            Ok(()) => Redirect::to(&format!("/foretag/{företag}?kvitterad=1")).into_response(),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => ingen_åtkomst(),
            Err(e) => serverfel(e),
        }
    }
}

/// Att göra, företagets startsida.
#[component]
pub fn AttGöra() -> impl IntoView {
    sida(hämta_att_göra, att_göra)
}

fn att_göra(data: AttGöradata) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let företaget = {
        let val = use_context::<Företagsval>().unwrap_or_default();
        val.0
            .into_iter()
            .find(|f| f.href() == bas)
            .map(|f| format!("{} · {}", f.namn, f.orgnr))
            .unwrap_or_default()
    };
    let mut sammanfattning = Vec::new();
    match data.driftvarningar.len() {
        0 => {}
        1 => sammanfattning.push("1 driftvarning".to_string()),
        n => sammanfattning.push(format!("{n} driftvarningar")),
    }
    if data.försenade > 0 {
        sammanfattning.push(format!("{} försenade", data.försenade));
    }
    sammanfattning.push(format!("{} inom 7 dagar", data.inom_sju_dagar));
    sammanfattning.push(format!("{} utan datum", data.utan_datum));
    let sammanfattning = sammanfattning.join(" · ");
    let öppet = data.öppet.clone();
    let kort = {
        let bas = bas.clone();
        move |k: KortVy| {
            let innehåll = öppet
                .as_ref()
                .filter(|(nyckel, _)| *nyckel == k.nyckel)
                .map(|(_, ö)| ö.clone());
            view! { <Kort bas=bas.clone() k=k öppet=innehåll /> }
        }
    };
    let sida_öppen = data
        .öppet
        .as_ref()
        .is_some_and(|(_, ö)| matches!(ö, Öppet::Underlag { .. }))
        || data.öppet.as_ref().is_some_and(|(nyckel, _)| {
            data.utkast
                .iter()
                .flat_map(|p| p.kort.iter())
                .any(|k| k.nyckel == *nyckel)
        });
    let tidslinje = if data.tidslinje.is_empty() {
        view! { <p class="diskret ag-tom">"Inget med datum."</p> }.into_any()
    } else {
        data.tidslinje
            .iter()
            .cloned()
            .map(|d| {
                let klass = if d.försenat {
                    "ag-day bad"
                } else if d.idag {
                    "ag-day idag"
                } else {
                    "ag-day"
                };
                let kort = d.kort.into_iter().map(kort.clone()).collect_view();
                view! {
                    <div class=klass>
                        {d.rubrik}
                        <span class="diskret">{d.avstånd}</span>
                    </div>
                    {kort}
                }
            })
            .collect_view()
            .into_any()
    };
    let antal_utkast: usize = data.utkast.iter().map(|p| p.kort.len()).sum();
    let utkast = (antal_utkast > 0).then(|| {
        let perioder = data
            .utkast
            .iter()
            .cloned()
            .map(|p| {
                let varning = p.varning.map(|v| {
                    view! { <span class="badge badge-varning">{v}</span> }
                });
                let kort = p.kort.into_iter().map(kort.clone()).collect_view();
                view! {
                    <div class="per">{p.rubrik} {varning}</div>
                    {kort}
                }
            })
            .collect_view();
        view! {
            <div class="ig">
                "Utkast"
                <span class="n">{antal_utkast}</span>
            </div>
            {perioder}
        }
    });
    let underlag = (!data.underlag.is_empty()).then(|| {
        let n = data.underlag.len();
        let kort = data
            .underlag
            .iter()
            .cloned()
            .map(kort.clone())
            .collect_view();
        view! {
            <div class="ig">
                "Okopplade underlag"
                <span class="n">{n}</span>
            </div>
            {kort}
        }
    });
    let tomt = (data.utan_datum == 0).then(|| view! { <p class="diskret">"Inget."</p> });
    let nyckeltal = data
        .nyckeltal
        .iter()
        .cloned()
        .map(|n| {
            view! {
                <div class="stat">
                    <span class="diskret">{n.namn}</span>
                    <b>{n.värde}</b>
                    <span class="sub">{n.under}</span>
                </div>
            }
        })
        .collect_view();
    let driftvarningar = (!data.driftvarningar.is_empty()).then(|| {
        let varningar = data
            .driftvarningar
            .iter()
            .cloned()
            .map(|v| view! { <Driftvarning v=v /> })
            .collect_view();
        view! { <div class="driftvarningar">{varningar}</div> }
    });
    let notis = data
        .notis
        .clone()
        .map(|t| view! { <p class="notis" role="status">{t}</p> });
    let fel = data
        .fel
        .clone()
        .map(|t| view! { <p class="fel" role="alert">{t}</p> });
    let återställningar = data
        .återställningar
        .iter()
        .cloned()
        .map(|v| view! { <VarningEfterÅterställning bas=bas.clone() v=v /> })
        .collect_view();

    view! {
        <section class="agenda">
            {återställningar}
            {notis}
            {fel}
            <div class="ag-rubrik">
                <p class="diskret">{företaget}</p>
                <h1>{data.dag.clone()}</h1>
                <p class="diskret">{sammanfattning}</p>
            </div>
            {driftvarningar}
            <div class="nyck">{nyckeltal}</div>
            <div class=if sida_öppen { "ag-grid sida-oppen" } else { "ag-grid" }>
                <div class="ag-tidslinje">{tidslinje}</div>
                <div class="ag-side">
                    <div class="ag-day">
                        "Utan datum"
                        <span class="diskret">{data.utan_datum}</span>
                    </div>
                    {utkast}
                    {underlag}
                    {tomt}
                </div>
            </div>
        </section>
    }
    .into_any()
}

/// Varningen efter en återställning, med kvittensen.
#[component]
fn VarningEfterÅterställning(bas: String, v: ÅterställningVy) -> impl IntoView {
    view! {
        <div class="aterstallning" role="alert">
            <div>
                <b>"Installationen är återställd"</b>
                <p>{v.text}</p>
            </div>
            <form method="post" action=format!("{bas}/att-gora/aterstallning/{}/kvittera", v.id)>
                <button class="knapp" type="submit">
                    "Kvittera"
                </button>
            </form>
        </div>
    }
}

/// En driftvarning, med en länk till sidan Installation.
#[component]
fn Driftvarning(v: DriftvarningVy) -> impl IntoView {
    view! {
        <div class="driftvarning" role="alert">
            <span class="badge badge-drift">"drift"</span>
            <div>
                <b>{v.rubrik}</b>
                <p>{v.text}</p>
            </div>
            <a class="knapp knapp-diskret" href="/installation">
                "Installation"
            </a>
        </div>
    }
}

/// Ett kort i agendan. Rubriken fäller ut och ihop kortet.
#[component]
fn Kort(bas: String, k: KortVy, öppet: Option<Öppet>) -> impl IntoView {
    let href = if öppet.is_some() {
        bas.clone()
    } else {
        format!("{bas}?oppna={}", k.nyckel)
    };
    let märke = match k.sort {
        Sort::Utkast => view! { <UtkastBadge /> }.into_any(),
        Sort::Underlag => view! { <span class="badge badge-okopplat">"okopplat"</span> }.into_any(),
    };
    let gem = k.gem.then(|| view! { <Gem /> });
    let belopp = k
        .belopp
        .clone()
        .map(|b| view! { <span class="belopp">{b}</span> });
    let klass = if öppet.is_some() {
        "acard open"
    } else {
        "acard"
    };
    let innehåll = öppet.map(|ö| {
        let innehåll = match ö {
            Öppet::Utkast {
                dokument,
                formulär,
                okopplade,
            } => view! {
                <Formulär
                    bas=bas.clone()
                    data=formulär
                    okopplade=okopplade
                    utkast=Some(*dokument)
                    efter="att_gora"
                />
            }
            .into_any(),
            Öppet::Underlag {
                underlag,
                verifikat,
                ta_bort,
            } => view! { <VadÄrDetHär bas=bas.clone() u=*underlag verifikat=verifikat ta_bort=ta_bort /> }
                .into_any(),
        };
        view! { <div class="ab">{innehåll}</div> }
    });
    view! {
        <div class=klass id=k.nyckel.clone()>
            <a class="ah" href=href>
                <span class="ah-text">
                    <b>{k.rubrik.clone()}</b>
                    <span class="s">{märke} {gem}</span>
                </span>
                <span class="ah-hoger">
                    {belopp}
                    <span class=format!("when {}", k.när_klass)>{k.när.clone()}</span>
                </span>
            </a>
            {innehåll}
        </div>
    }
}

/// "Vad är det här?" för ett okopplat underlag: Kvitto, Komplettering eller Dubblett eller fel
/// fil.
#[component]
fn VadÄrDetHär(
    bas: String,
    u: UnderlagVy,
    verifikat: Vec<(String, String)>,
    ta_bort: bool,
) -> impl IntoView {
    let väg = format!("{bas}/att-gora/underlag/{}", u.id);
    let komplettering = if verifikat.is_empty() {
        view! {
            <p class="diskret">"Komplettering: det finns inget bokfört verifikat att komplettera."</p>
        }
        .into_any()
    } else {
        let val = verifikat
            .into_iter()
            .map(|(id, text)| view! { <option value=id>{text}</option> })
            .collect_view();
        view! {
            <form class="tchoice-form" method="post" action=format!("{väg}/komplettera")>
                <label for="komplettera-verifikat">"Komplettering av"</label>
                <select id="komplettera-verifikat" name="verifikat">
                    {val}
                </select>
                <button class="knapp" type="submit">
                    <Gem />
                    "Komplettering"
                </button>
            </form>
        }
        .into_any()
    };
    let borttagning = if ta_bort {
        view! {
            <div class="inl">
                <b>"Dubblett eller fel fil"</b>
                <p class="diskret">"Underlaget tas bort ur arkivet. Borttagningen loggas."</p>
                <form method="post" action=format!("{väg}/ta-bort") class="knappar">
                    <button class="knapp knapp-fara" type="submit">
                        "Ta bort underlaget"
                    </button>
                    <a class="knapp" href=format!("{bas}?oppna=underlag-{}", u.id)>
                        "Avbryt"
                    </a>
                </form>
            </div>
        }
        .into_any()
    } else {
        view! {
            <a
                class="knapp knapp-diskret"
                href=format!("{bas}?oppna=underlag-{}&ta_bort=1", u.id)
            >
                "Dubblett eller fel fil…"
            </a>
        }
        .into_any()
    };
    let (uppladdad, av) = u.uppladdad.clone();
    view! {
        <div class="triage">
            <div class="triage-visning">
                <Förhandsvisning bas=bas.clone() u=u.clone() />
            </div>
            <div class="tchoice">
                <h2>"Vad är det här?"</h2>
                <form method="post" action=format!("{väg}/kvitto")>
                    <button class="knapp" type="submit">
                        "Kvitto"
                    </button>
                    <span class="diskret">" Ett nytt verifikatutkast med underlaget."</span>
                </form>
                <form method="post" action=format!("{väg}/leverantorsfaktura")>
                    <button class="knapp" type="submit">
                        "Leverantörsfaktura"
                    </button>
                    <span class="diskret">
                        " Något som ska betalas. Ett utkast med underlaget som original."
                    </span>
                </form>
                {komplettering}
                {borttagning}
                <p class="diskret">
                    {format!("Uppladdat {uppladdad} av {av} · arkiverat med SHA-256")}
                </p>
            </div>
        </div>
    }
}
