//! Leverantörsregistret: listan över företagets leverantörer och den valda leverantören som
//! dokument, med samma mönster som Verifikat. Lägga upp och ändra görs i formuläret i
//! dokumentets plats; arkivera och radera bekräftas i dokumentet.
//!
//! Formuläret prövas med `maya_domain::leverantor` i webbläsaren och av `core` på servern, med
//! samma koder. Utan JavaScript skickas det till servern, som skickar tillbaka hit med det
//! användaren skrev; sidan prövar det igen och visar felen vid fälten.

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::leverantor::{Fält, Fältfel, NyLeverantör, OmvändSkattskyldighet};
use serde::{Deserialize, Serialize};

use crate::app::LoggaIn;
use crate::foretag::IngenÅtkomst;
use crate::sidodata::sida;
use crate::verifikat::{företagets_bas, inloggad};

/// En leverantör som sidan visar den.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeverantörVy {
    pub id: String,
    pub namn: String,
    pub orgnr: String,
    pub gatuadress: String,
    pub postnummer: String,
    pub postort: String,
    pub land: String,
    pub momsregnr: String,
    pub bankgiro: String,
    pub plusgiro: String,
    pub iban: String,
    pub bic: String,
    /// Kontonumret, eller tomt.
    pub standardkonto: String,
    /// Kontots namn i kontoplanen.
    pub standardkontots_namn: String,
    /// Koden för den förvalda omvända skattskyldigheten.
    pub omvänd_skattskyldighet: String,
    pub arkiverad: bool,
    pub kan_raderas: bool,
}

/// En post i leverantörens historik.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistorikVy {
    pub tid: String,
    pub text: String,
    pub namn: String,
}

/// Det som sidan Leverantörer visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leverantörsdata {
    pub lista: Vec<LeverantörVy>,
    pub valt: Option<LeverantörVy>,
    pub historik: Vec<HistorikVy>,
    /// Kontoplanen som formuläret prövar standardkontot mot: nummer och om kontot är aktivt.
    pub konton: Vec<(u16, bool)>,
}

/// Datan för `/foretag/<id>/leverantorer` och `/foretag/<id>/leverantorer/ny`.
#[server(prefix = "/api", endpoint = "leverantorsdata", input = server_fn::codec::GetUrl)]
pub async fn hämta_leverantörsdata(adress: String) -> Result<Leverantörsdata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::leverantörsdata(&begäran).await,
        None => Ok(Leverantörsdata::default()),
    }
}

/// Felkoderna i adressen och deras texter.
fn feltext(kod: &str) -> Option<&'static str> {
    Some(match kod {
        "har_fakturor" => "Leverantören har fakturor och kan inte raderas. Arkivera den i stället.",
        _ => return None,
    })
}

/// Formulärets fält som de heter i formuläret, i ordning.
pub(crate) const FÄLT: [&str; 13] = [
    "namn",
    "orgnr",
    "gatuadress",
    "postnummer",
    "postort",
    "land",
    "momsregnr",
    "bankgiro",
    "plusgiro",
    "iban",
    "bic",
    "standardkonto",
    "omvand_skattskyldighet",
];

/// Formuläret ur fältens värden, med namnen i [`FÄLT`].
pub(crate) fn formulär(värde: impl Fn(&str) -> String) -> NyLeverantör {
    NyLeverantör {
        namn: värde("namn"),
        orgnr: värde("orgnr"),
        gatuadress: värde("gatuadress"),
        postnummer: värde("postnummer"),
        postort: värde("postort"),
        land: värde("land"),
        momsregnr: värde("momsregnr"),
        bankgiro: värde("bankgiro"),
        plusgiro: värde("plusgiro"),
        iban: värde("iban"),
        bic: värde("bic"),
        standardkonto: värde("standardkonto"),
        omvänd_skattskyldighet: värde("omvand_skattskyldighet"),
    }
}

impl LeverantörVy {
    fn som_formulär(&self) -> NyLeverantör {
        NyLeverantör {
            namn: self.namn.clone(),
            orgnr: self.orgnr.clone(),
            gatuadress: self.gatuadress.clone(),
            postnummer: self.postnummer.clone(),
            postort: self.postort.clone(),
            land: self.land.clone(),
            momsregnr: self.momsregnr.clone(),
            bankgiro: self.bankgiro.clone(),
            plusgiro: self.plusgiro.clone(),
            iban: self.iban.clone(),
            bic: self.bic.clone(),
            standardkonto: self.standardkonto.clone(),
            omvänd_skattskyldighet: self.omvänd_skattskyldighet.clone(),
        }
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::leverantor::{
        self, Leverantör, LeverantörsFel, LeverantörsId, Status, Urval
    };
    use maya_core::{Databas, Klocka, kontoplan};

    use super::{FÄLT, HistorikVy, LeverantörVy, Leverantörsdata, formulär};
    use crate::sidodata::{self, Sidbegäran};
    use crate::svar::{ingen_åtkomst, serverfel};

    #[derive(Clone)]
    pub(crate) struct LeverantörsState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: LeverantörsState) -> Router {
        Router::new()
            .route("/foretag/{id}/leverantorer", post(skapa))
            .route("/foretag/{id}/leverantorer/{l}", post(ändra))
            .route("/foretag/{id}/leverantorer/{l}/arkivera", post(arkivera))
            .route("/foretag/{id}/leverantorer/{l}/radera", post(radera))
            .with_state(Arc::new(state))
    }

    /// Hämtar det som sidan visar. Utan åtkomst blir datan tom; sidan visar det ur
    /// företagsvalet.
    pub(crate) async fn leverantörsdata(
        b: &Sidbegäran,
    ) -> Result<Leverantörsdata, ServerFnError> {
        let företag = match b.delar().as_slice() {
            ["foretag", id, "leverantorer"] | ["foretag", id, "leverantorer", "ny"] => {
                id.parse::<FöretagsId>().ok()
            }
            _ => None,
        };
        let Some(företag) = företag else {
            return Ok(Leverantörsdata::default());
        };
        let urval = b
            .värde("visa")
            .and_then(|v| Urval::från_kod(&v))
            .unwrap_or_default();
        let valt = b.värde("l").and_then(|l| l.parse::<LeverantörsId>().ok());
        match hämta(&b.db, &b.inloggad, företag, urval, valt).await {
            Ok(data) => Ok(data),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                Ok(Leverantörsdata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    async fn hämta(
        db: &Databas,
        inloggad: &Inloggad,
        företag: FöretagsId,
        urval: Urval,
        valt: Option<LeverantörsId>,
    ) -> Result<Leverantörsdata, ÅtkomstFel> {
        let aktör = &inloggad.aktör;
        let konton = kontoplan::konton(db, aktör, företag).await?;
        let namn = |nr: Option<maya_domain::kontoplan::Kontonummer>| {
            nr.and_then(|nr| konton.iter().find(|k| k.nummer == nr))
                .map(|k| k.namn.clone())
                .unwrap_or_default()
        };
        let vy = |l: &Leverantör| vy(l, namn(l.uppgifter.standardkonto));
        let lista = leverantor::lista(db, aktör, företag, urval).await?;
        let (valt, historik) = match valt {
            Some(id) => match leverantor::leverantören(db, aktör, företag, id).await? {
                Some(l) => {
                    let historik = leverantor::historik(db, aktör, företag, id).await?;
                    (Some(vy(&l)), historik)
                }
                None => (None, Vec::new()),
            },
            None => (None, Vec::new()),
        };
        Ok(Leverantörsdata {
            lista: lista.iter().map(vy).collect(),
            valt,
            historik: historik
                .iter()
                .map(|p| HistorikVy {
                    tid: maya_domain::tid::minut(p.tid),
                    text: p.text(),
                    namn: p.aktörsnamn(),
                })
                .collect(),
            konton: konton.iter().map(|k| (k.nummer.get(), k.aktiv)).collect(),
        })
    }

    fn vy(l: &Leverantör, standardkontots_namn: String) -> LeverantörVy {
        let u = &l.uppgifter;
        LeverantörVy {
            id: l.id.to_string(),
            namn: u.namn.clone(),
            orgnr: u.orgnr.clone(),
            gatuadress: u.gatuadress.clone(),
            postnummer: u.postnummer.clone(),
            postort: u.postort.clone(),
            land: u.land.clone(),
            momsregnr: u.momsregnr.clone(),
            bankgiro: u.bankgiro.clone(),
            plusgiro: u.plusgiro.clone(),
            iban: u.iban.clone(),
            bic: u.bic.clone(),
            standardkonto: u
                .standardkonto
                .map(|k| k.get().to_string())
                .unwrap_or_default(),
            standardkontots_namn,
            omvänd_skattskyldighet: u.omvänd_skattskyldighet.kod().into(),
            arkiverad: l.status == Status::Arkiverad,
            kan_raderas: l.kan_raderas,
        }
    }

    type Fält = Vec<(String, String)>;

    fn värde(fält: &Fält, namn: &str) -> String {
        fält
            .iter()
            .find(|(k, _)| k == namn)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    /// Tillbaka till formuläret `sida` med det användaren skrev; sidan prövar det igen.
    fn tillbaka(sida: &str, fält: &Fält) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("tillbaka", "1");
        for namn in FÄLT {
            query.append_pair(namn, &värde(fält, namn));
        }
        let skiljetecken = if sida.contains('?') { '&' } else { '?' };
        Redirect::to(&format!("{sida}{skiljetecken}{}", query.finish())).into_response()
    }

    fn tolka(id: &str, l: Option<&str>) -> Option<(FöretagsId, Option<LeverantörsId>)> {
        let företag = id.parse().ok()?;
        match l {
            Some(l) => Some((företag, Some(l.parse().ok()?))),
            None => Some((företag, None)),
        }
    }

    /// Svaret på ett fel från `core`: tillbaka till formuläret, en felkod i dokumentet, 404
    /// eller 403.
    fn svar(
        e: LeverantörsFel,
        formuläret: impl FnOnce() -> Response,
        dokumentet: &str,
    ) -> Response {
        match e {
            LeverantörsFel::Ogiltigt(_) => formuläret(),
            LeverantörsFel::HarFakturor => {
                Redirect::to(&format!("{dokumentet}&fel=har_fakturor")).into_response()
            }
            LeverantörsFel::FinnsInte => crate::svar::text(
                axum::http::StatusCode::NOT_FOUND,
                "Leverantören finns inte.\n",
            ),
            LeverantörsFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            e => serverfel(e),
        }
    }

    async fn skapa(
        State(state): State<Arc<LeverantörsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, _)) = tolka(&id, None) else {
            return ingen_åtkomst();
        };
        let sida = format!("/foretag/{företag}/leverantorer");
        let ny = formulär(|n| värde(&fält, n));
        match leverantor::skapa(&state.db, &state.klocka, &inloggad.aktör, företag, &ny).await {
            Ok(l) => Redirect::to(&format!("{sida}?l={l}&sparat=1")).into_response(),
            Err(e) => svar(e, || tillbaka(&format!("{sida}/ny"), &fält), &sida),
        }
    }

    async fn ändra(
        State(state): State<Arc<LeverantörsState>>,
        Path((id, l)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, Some(l))) = tolka(&id, Some(&l)) else {
            return ingen_åtkomst();
        };
        let dokumentet = format!("/foretag/{företag}/leverantorer?l={l}");
        let ny = formulär(|n| värde(&fält, n));
        match leverantor::ändra(&state.db, &state.klocka, &inloggad.aktör, företag, l, &ny).await
        {
            Ok(()) => Redirect::to(&format!("{dokumentet}&sparat=1")).into_response(),
            Err(e) => svar(
                e,
                || tillbaka(&format!("{dokumentet}&andra=1"), &fält),
                &dokumentet,
            ),
        }
    }

    async fn arkivera(
        State(state): State<Arc<LeverantörsState>>,
        Path((id, l)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, Some(l))) = tolka(&id, Some(&l)) else {
            return ingen_åtkomst();
        };
        let dokumentet = format!("/foretag/{företag}/leverantorer?l={l}");
        match leverantor::arkivera(&state.db, &state.klocka, &inloggad.aktör, företag, l).await {
            Ok(()) => {
                Redirect::to(&format!("{dokumentet}&visa=arkiverade&arkiverad=1")).into_response()
            }
            Err(e) => svar(e, || Redirect::to(&dokumentet).into_response(), &dokumentet),
        }
    }

    async fn radera(
        State(state): State<Arc<LeverantörsState>>,
        Path((id, l)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, Some(l))) = tolka(&id, Some(&l)) else {
            return ingen_åtkomst();
        };
        let dokumentet = format!("/foretag/{företag}/leverantorer?l={l}");
        match leverantor::radera(&state.db, &state.klocka, &inloggad.aktör, företag, l).await {
            Ok(()) => {
                Redirect::to(&format!("/foretag/{företag}/leverantorer?raderad=1")).into_response()
            }
            Err(e) => svar(e, || Redirect::to(&dokumentet).into_response(), &dokumentet),
        }
    }
}

/// Fliken Leverantörer: listan och, när en leverantör är vald, dess dokument.
#[component]
pub fn LeverantörerSida() -> impl IntoView {
    sida(hämta_leverantörsdata, |d| leverantörssida(d, false))
}

/// Formuläret för en ny leverantör, bredvid listan.
#[component]
pub fn NyLeverantörSida() -> impl IntoView {
    sida(hämta_leverantörsdata, |d| leverantörssida(d, true))
}

fn leverantörssida(data: Leverantörsdata, ny: bool) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let visa = fråga("visa").unwrap_or_default();
    let arkiverade = visa == "arkiverade";
    let notis = if fråga("sparat").is_some() {
        Some("Sparat.")
    } else if fråga("arkiverad").is_some() {
        Some("Leverantören arkiverades och visas inte längre i väljarna.")
    } else if fråga("raderad").is_some() {
        Some("Leverantören raderades och raderingen loggades.")
    } else {
        None
    }
    .map(|text| view! { <p class="notis" role="status">{text}</p> });
    let fel = fråga("fel")
        .and_then(|k| feltext(&k))
        .map(|text| view! { <p class="fel" role="alert">{text}</p> });
    let filter = [
        ("", "Aktiva", !arkiverade),
        ("?visa=arkiverade", "Arkiverade", arkiverade),
    ]
    .into_iter()
    .map(|(q, namn, aktuellt)| {
        view! {
            <a href=format!("{bas}/leverantorer{q}") aria-current=aktuellt.then_some("page")>
                {namn}
            </a>
        }
    })
    .collect_view();
    let valt_id = data.valt.as_ref().map(|l| l.id.clone());
    let urval = if arkiverade { "&visa=arkiverade" } else { "" };
    let rader = data
        .lista
        .iter()
        .map(|l| {
            let aktuellt = (Some(&l.id) == valt_id.as_ref()).then_some("page");
            let under = [l.orgnr.clone(), l.postort.clone()]
                .into_iter()
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            view! {
                <li>
                    <a href=format!("{bas}/leverantorer?l={}{urval}", l.id) aria-current=aktuellt>
                        <span class="verifikatlista-topp">
                            <span>{l.namn.clone()}</span>
                            <span class="diskret">{l.standardkonto.clone()}</span>
                        </span>
                        <span class="diskret">{under}</span>
                    </a>
                </li>
            }
        })
        .collect_view();
    let tom = data.lista.is_empty().then(|| {
        view! {
            <p class="diskret">
                {if arkiverade { "Inga arkiverade leverantörer." } else { "Inga leverantörer än." }}
            </p>
        }
    });
    let konton: Vec<(Kontonummer, bool)> = data
        .konton
        .iter()
        .filter_map(|&(nr, aktiv)| Some((Kontonummer::new(nr)?, aktiv)))
        .collect();
    let tillbaka = fråga("tillbaka").is_some();
    let från_frågan = || formulär(|n| fråga(n).unwrap_or_default());
    let detalj = if ny {
        let start = if tillbaka {
            från_frågan()
        } else {
            NyLeverantör {
                land: "SE".into(),
                ..NyLeverantör::default()
            }
        };
        view! {
            <LeverantörsFormulär
                rubrik="Ny leverantör"
                action=format!("{bas}/leverantorer")
                avbryt=format!("{bas}/leverantorer")
                start=start
                konton=konton
                visa=tillbaka
            />
        }
        .into_any()
    } else {
        match data.valt.clone() {
            Some(l) if fråga("andra").is_some() => {
                let start = if tillbaka {
                    från_frågan()
                } else {
                    l.som_formulär()
                };
                view! {
                    <LeverantörsFormulär
                        rubrik="Ändra leverantör"
                        action=format!("{bas}/leverantorer/{}", l.id)
                        avbryt=format!("{bas}/leverantorer?l={}", l.id)
                        start=start
                        konton=konton
                        visa=tillbaka
                    />
                }
                .into_any()
            }
            Some(l) => view! {
                <Leverantörsdokument
                    bas=bas.clone()
                    l=l
                    historik=data.historik.clone()
                    arkivera=fråga("arkivera").is_some()
                    radera=fråga("radera").is_some()
                />
            }
            .into_any(),
            None => view! {
                <div class="tom">
                    <p>"Välj en leverantör i listan, eller lägg upp en ny."</p>
                </div>
            }
            .into_any(),
        }
    };
    view! {
        <div class="verifikat-delad">
            <div class="verifikat-spalt">
                <div class="verifikat-spalt-rubrik">
                    <h1>"Leverantörer"</h1>
                    <a class="knapp" href=format!("{bas}/leverantorer/ny")>
                        "Ny"
                    </a>
                </div>
                <nav class="filter" aria-label="Filter">
                    {filter}
                </nav>
                {tom}
                <ul class="verifikatlista">{rader}</ul>
            </div>
            <div class="verifikat-detalj">{notis} {fel} {detalj}</div>
        </div>
    }
    .into_any()
}

/// Den valda leverantören: uppgifterna, historiken och Ändra, Arkivera och Radera.
#[component]
fn Leverantörsdokument(
    bas: String,
    l: LeverantörVy,
    historik: Vec<HistorikVy>,
    arkivera: bool,
    radera: bool,
) -> impl IntoView {
    let här = format!("{bas}/leverantorer?l={}", l.id);
    let rad = |etikett: &'static str, värde: String| {
        (!värde.trim().is_empty()).then(|| {
            view! {
                <dt>{etikett}</dt>
                <dd>{värde}</dd>
            }
        })
    };
    let adress = [
        l.gatuadress.clone(),
        format!("{} {}", l.postnummer, l.postort).trim().to_string(),
    ]
    .into_iter()
    .filter(|t| !t.is_empty())
    .collect::<Vec<_>>()
    .join(", ");
    let standardkonto = if l.standardkonto.is_empty() {
        String::new()
    } else {
        format!("{} {}", l.standardkonto, l.standardkontots_namn)
    };
    let omvänd = OmvändSkattskyldighet::från_kod(&l.omvänd_skattskyldighet)
        .unwrap_or_default()
        .namn()
        .to_string();
    let status = if l.arkiverad { "Arkiverad" } else { "Aktiv" };
    let åtgärd = if arkivera && !l.arkiverad {
        view! {
            <section class="dialog" role="alertdialog" aria-labelledby="arkivera-rubrik">
                <h2 id="arkivera-rubrik">"Arkivera leverantören?"</h2>
                <p class="diskret">
                    "En arkiverad leverantör visas inte i väljarna. Uppgifterna och historiken finns kvar."
                </p>
                <form method="post" action=format!("{bas}/leverantorer/{}/arkivera", l.id)>
                    <a class="knapp" href=här.clone()>
                        "Avbryt"
                    </a>
                    " "
                    <button class="knapp knapp-primar" type="submit">
                        "Arkivera"
                    </button>
                </form>
            </section>
        }
        .into_any()
    } else if radera && l.kan_raderas {
        view! {
            <section class="dialog" role="alertdialog" aria-labelledby="radera-rubrik">
                <h2 id="radera-rubrik">"Radera leverantören?"</h2>
                <p class="diskret">
                    "Leverantören har inga fakturor. Raderingen loggas med uppgifterna."
                </p>
                <form method="post" action=format!("{bas}/leverantorer/{}/radera", l.id)>
                    <a class="knapp" href=här.clone()>
                        "Avbryt"
                    </a>
                    " "
                    <button class="knapp knapp-fara" type="submit">
                        "Radera leverantör"
                    </button>
                </form>
            </section>
        }
        .into_any()
    } else {
        let arkivera = (!l.arkiverad).then(|| {
            view! {
                <a class="knapp" href=format!("{här}&arkivera=1")>
                    "Arkivera"
                </a>
            }
        });
        let radera = if l.kan_raderas {
            view! {
                <a class="knapp knapp-fara-diskret" href=format!("{här}&radera=1")>
                    "Radera"
                </a>
            }
            .into_any()
        } else {
            view! {
                <span class="diskret">
                    "Leverantören har fakturor och kan bara arkiveras."
                </span>
            }
            .into_any()
        };
        view! {
            <p class="knappar">
                <a class="knapp" href=format!("{här}&andra=1")>
                    "Ändra"
                </a>
                " "
                {arkivera}
                " "
                {radera}
            </p>
        }
        .into_any()
    };
    view! {
        <article class="dokument">
            <header>
                <h2>{l.namn.clone()}</h2>
                <p>
                    <span class="markering">{status}</span>
                </p>
            </header>
            {åtgärd}
            <dl class="metadata">
                {rad("Orgnr", l.orgnr.clone())}
                {rad("Adress", adress)}
                {rad("Land", l.land.clone())}
                {rad("Momsreg-nr", l.momsregnr.clone())}
                {rad("Bankgiro", l.bankgiro.clone())}
                {rad("Plusgiro", l.plusgiro.clone())}
                {rad("IBAN", l.iban.clone())}
                {rad("BIC", l.bic.clone())}
                <dt>"Standardkonto"</dt>
                <dd>
                    {if standardkonto.is_empty() { "Inget".to_string() } else { standardkonto }}
                </dd>
                <dt>"Omvänd skattskyldighet"</dt>
                <dd>{omvänd}</dd>
            </dl>
            <section>
                <h3>"Historik"</h3>
                <ul class="historik">
                    {historik
                        .into_iter()
                        .map(|h| {
                            view! {
                                <li>
                                    <span class="diskret">{h.tid}</span>
                                    " "
                                    {h.text}
                                    " "
                                    <span class="diskret">"· " {h.namn}</span>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
            </section>
        </article>
    }
}

/// Felen vid ett fält, när de ska visas.
fn fel_vid(
    fel: Memo<Vec<Fältfel>>,
    visa: RwSignal<bool>,
    fält: &'static str,
) -> impl Fn() -> Option<AnyView> + Send + Sync + 'static {
    move || {
        if !visa.get() {
            return None;
        }
        fel.with(|fel| {
            fel.iter().find(|f| f.fält.namn() == fält).map(|f| {
                view! {
                    <p class="fel" role="alert" data-kod=f.kod>
                        {f.text.clone()}
                    </p>
                }
                .into_any()
            })
        })
    }
}

/// Formuläret för en ny eller ändrad leverantör. `visa` visar felen direkt, när formuläret kom
/// tillbaka från servern.
#[component]
fn LeverantörsFormulär(
    rubrik: &'static str,
    action: String,
    avbryt: String,
    start: NyLeverantör,
    konton: Vec<(Kontonummer, bool)>,
    visa: bool,
) -> impl IntoView {
    let leverantör = RwSignal::new(start.clone());
    let visa = RwSignal::new(visa);
    let fel =
        Memo::new(move |_| leverantör.with(|l| l.kontrollera(&konton).err().unwrap_or_default()));
    let skicka = move |ev: leptos::ev::SubmitEvent| {
        if !fel.with_untracked(Vec::is_empty) {
            ev.prevent_default();
            visa.set(true);
        }
    };
    let fält = move |fält: Fält,
                     etikett: &'static str,
                     värde: String,
                     sätt: fn(&mut NyLeverantör, String)| {
        let namn = fält.namn();
        view! {
            <label for=namn>{etikett}</label>
            <input
                id=namn
                name=namn
                type="text"
                value=värde
                on:input=move |ev| leverantör.update(|l| sätt(l, event_target_value(&ev)))
            />
            {fel_vid(fel, visa, namn)}
        }
    };
    let omvänd = start.omvänd_skattskyldighet.clone();
    view! {
        <form class="formular" method="post" action=action novalidate on:submit=skicka>
            <h2>{rubrik}</h2>
            {fält(Fält::Namn, "Namn", start.namn.clone(), |l, v| l.namn = v)}
            {fält(Fält::Orgnr, "Organisationsnummer", start.orgnr.clone(), |l, v| l.orgnr = v)}
            {fält(Fält::Gatuadress, "Gatuadress", start.gatuadress.clone(), |l, v| {
                l.gatuadress = v
            })}
            {fält(Fält::Postnummer, "Postnummer", start.postnummer.clone(), |l, v| {
                l.postnummer = v
            })}
            {fält(Fält::Postort, "Postort", start.postort.clone(), |l, v| l.postort = v)}
            {fält(Fält::Land, "Land (landskod)", start.land.clone(), |l, v| l.land = v)}
            {fält(Fält::Momsregnr, "Momsregistreringsnummer", start.momsregnr.clone(), |l, v| {
                l.momsregnr = v
            })}
            <h3>"Betaluppgifter"</h3>
            {fält(Fält::Bankgiro, "Bankgiro", start.bankgiro.clone(), |l, v| {
                l.bankgiro = v
            })}
            {fält(Fält::Plusgiro, "Plusgiro", start.plusgiro.clone(), |l, v| {
                l.plusgiro = v
            })}
            {fält(Fält::Iban, "IBAN", start.iban.clone(), |l, v| l.iban = v)}
            {fält(Fält::Bic, "BIC", start.bic.clone(), |l, v| l.bic = v)}
            <h3>"Förval på leverantörens fakturor"</h3>
            {fält(Fält::Standardkonto, "Standardkonto", start.standardkonto.clone(), |l, v| {
                l.standardkonto = v
            })}
            <p class="diskret">"Fylls i på tomma fakturarader. Varje ändring loggas."</p>
            <label for="omvand_skattskyldighet">"Omvänd skattskyldighet"</label>
            <select
                id="omvand_skattskyldighet"
                name="omvand_skattskyldighet"
                on:change=move |ev| {
                    leverantör.update(|l| l.omvänd_skattskyldighet = event_target_value(&ev))
                }
            >
                {OmvändSkattskyldighet::ALLA
                    .into_iter()
                    .map(|o| {
                        let vald = o.kod() == omvänd
                            || (omvänd.is_empty() && o == OmvändSkattskyldighet::Ingen);
                        view! {
                            <option value=o.kod() selected=vald>
                                {o.namn()}
                            </option>
                        }
                    })
                    .collect_view()}
            </select>
            {fel_vid(fel, visa, Fält::OmvändSkattskyldighet.namn())}
            <p class="knappar">
                <button class="knapp knapp-primar" type="submit">
                    "Spara"
                </button>
                " "
                <a class="knapp" href=avbryt>
                    "Avbryt"
                </a>
            </p>
        </form>
    }
}
