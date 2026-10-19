//! Kundregistret: listan med kunderna och den valda kunden, som verifikaten (dokumentmönstret
//! med lista och detalj), och formuläret för att lägga upp och ändra en kund. Formuläret prövas
//! med `maya_domain::kund` i webbläsaren och i `core`.
//!
//! Sidorna är `/foretag/<id>/kunder` (`?k=<kund>` väljer en, `&andra=1` öppnar formuläret,
//! `&arkivera=1` och `&radera=1` frågorna) och `/foretag/<id>/kunder/ny`. Kundfakturor länkar
//! hit med [`kundregistret`].

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use maya_domain::kund::{Fält, Fältfel, NyKund, OmvändSkattskyldighet};
use serde::{Deserialize, Serialize};

use crate::app::LoggaIn;
use crate::foretag::IngenÅtkomst;
use crate::sidodata::sida;
use crate::verifikat::{företagets_bas, inloggad};

/// Adressen till kundregistret för företaget med sidan `bas` (`/foretag/<id>`).
pub fn kundregistret(bas: &str) -> String {
    format!("{bas}/kunder")
}

/// En kund som sidan visar den.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KundVy {
    pub id: String,
    pub namn: String,
    pub identitet: String,
    pub adress: String,
    pub epost: String,
    pub momsregnr: String,
    pub betalningsvillkor: String,
    /// En kod ur [`OmvändSkattskyldighet::kod`].
    pub omvänd_skattskyldighet: String,
    pub arkiverad: bool,
    /// När och av vilken kanal kunden lades upp.
    pub skapad: String,
}

impl KundVy {
    fn formulär(&self) -> NyKund {
        NyKund {
            namn: self.namn.clone(),
            identitet: self.identitet.clone(),
            adress: self.adress.clone(),
            epost: self.epost.clone(),
            momsregnr: self.momsregnr.clone(),
            betalningsvillkor: self.betalningsvillkor.clone(),
            omvänd_skattskyldighet: self.omvänd_skattskyldighet.clone(),
        }
    }
}

/// Det som kundregistret visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kunddata {
    /// Aktiva kunder, och med `?visa=alla` också de arkiverade.
    pub lista: Vec<KundVy>,
    pub valt: Option<KundVy>,
    /// Den valda kunden saknar fakturor och kan raderas.
    pub kan_raderas: bool,
    /// Den valda kundens fakturor och vad som är obetalt (#139).
    pub fakturor: Vec<KundensFakturaVy>,
    pub obetalt: String,
    /// Det som kunden har tillgodo efter kreditfakturor, tomt utan (#140).
    pub tillgodo: String,
}

/// En faktura i kundens detalj (#139).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KundensFakturaVy {
    pub id: String,
    /// Tomt för ett utkast.
    pub nummer: String,
    pub fakturadatum: String,
    pub förfallodag: String,
    pub total: String,
    pub återstår: String,
    /// Märkets kod och namn; `utkast` för ett utkast.
    pub märke: (String, String),
}

/// Datan för `/foretag/<id>/kunder` och `/foretag/<id>/kunder/ny`.
#[server(prefix = "/api", endpoint = "kunddata", input = server_fn::codec::GetUrl)]
pub async fn hämta_kunddata(adress: String) -> Result<Kunddata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::kunddata(&begäran).await,
        None => Ok(Kunddata::default()),
    }
}

/// Felkoderna i adressen och deras texter.
fn feltext(kod: &str) -> Option<&'static str> {
    Some(match kod {
        "har_fakturor" => "Kunden har fakturor och kan inte raderas. Arkivera den i stället.",
        _ => return None,
    })
}

/// Formulärets fältnamn, i ordning.
const FÄLT: [Fält; 7] = [
    Fält::Namn,
    Fält::Identitet,
    Fält::Adress,
    Fält::Epost,
    Fält::Momsregnr,
    Fält::Betalningsvillkor,
    Fält::OmvändSkattskyldighet,
];

fn sätt(k: &mut NyKund, fält: Fält, värde: String) {
    match fält {
        Fält::Namn => k.namn = värde,
        Fält::Identitet => k.identitet = värde,
        Fält::Adress => k.adress = värde,
        Fält::Epost => k.epost = värde,
        Fält::Momsregnr => k.momsregnr = värde,
        Fält::Betalningsvillkor => k.betalningsvillkor = värde,
        Fält::OmvändSkattskyldighet => k.omvänd_skattskyldighet = värde,
    }
}

/// Formuläret ur fälten i ett inskickat formulär eller i adressen.
fn ur_fält(värde: impl Fn(&str) -> Option<String>) -> NyKund {
    let mut k = NyKund {
        betalningsvillkor: String::new(),
        omvänd_skattskyldighet: String::new(),
        ..NyKund::default()
    };
    for fält in FÄLT {
        sätt(&mut k, fält, värde(fält.namn()).unwrap_or_default());
    }
    k
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::kund::{self, Kund, KundFel, KundId, Kundstatus};
    use maya_core::kundfaktura::{self, Status};
    use maya_core::{Databas, Klocka};

    use super::{FÄLT, KundVy, Kunddata, KundensFakturaVy, ur_fält};
    use crate::sidodata::{self, Sidbegäran};
    use crate::svar::{ingen_åtkomst, serverfel, text};
    use crate::verifikat::server::tid;

    #[derive(Clone)]
    pub(crate) struct KundState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: KundState) -> Router {
        Router::new()
            .route("/foretag/{id}/kunder", post(skapa))
            .route("/foretag/{id}/kunder/{k}", post(ändra))
            .route("/foretag/{id}/kunder/{k}/arkivera", post(arkivera))
            .route("/foretag/{id}/kunder/{k}/radera", post(radera))
            .with_state(Arc::new(state))
    }

    /// Hämtar det som kundregistret visar. Utan åtkomst blir datan tom; sidan visar det ur
    /// företagsvalet.
    pub(crate) async fn kunddata(b: &Sidbegäran) -> Result<Kunddata, ServerFnError> {
        let (företag, ny) = match b.delar().as_slice() {
            ["foretag", id, "kunder"] => (id.parse::<FöretagsId>().ok(), false),
            ["foretag", id, "kunder", "ny"] => (id.parse::<FöretagsId>().ok(), true),
            _ => (None, false),
        };
        let Some(företag) = företag else {
            return Ok(Kunddata::default());
        };
        let alla = b.värde("visa").as_deref() == Some("alla");
        let valt = if ny {
            None
        } else {
            b.värde("k").and_then(|k| k.parse::<KundId>().ok())
        };
        let valt_id = valt;
        let aktör = &b.inloggad.aktör;
        let resultat = async {
            let lista = kund::kunder(&b.db, aktör, företag).await?;
            let (valt, kan_raderas) = match valt {
                Some(k) => match kund::kunden(&b.db, aktör, företag, k).await {
                    Ok(kunden) => {
                        let har = kund::har_fakturor(&b.db, aktör, företag, k).await?;
                        (Some(vy(&kunden)), !har)
                    }
                    Err(KundFel::FinnsInte) => (None, false),
                    Err(e) => return Err(e),
                },
                None => (None, false),
            };
            // #139: kundens fakturor och vad som är obetalt.
            let fakturor = match (&valt, valt_id) {
                (Some(_), Some(k)) => {
                    kundfaktura::kundens_fakturor(&b.db, aktör, företag, k).await?
                }
                _ => Vec::new(),
            };
            let idag = b.klocka.idag();
            let kr = |öre: i64| maya_domain::belopp::Saldo::från_öre(öre).to_string();
            let obetalt: i64 = fakturor
                .iter()
                .filter(|f| f.status.i_reskontran())
                .map(|f| f.återstår.max(0))
                .sum();
            let tillgodo: i64 = fakturor
                .iter()
                .filter(|f| f.status.i_reskontran())
                .map(|f| (-f.återstår).max(0))
                .sum();
            let fakturor = fakturor
                .iter()
                .map(|f| KundensFakturaVy {
                    id: f.id.to_string(),
                    // #143: en extern faktura har det andra systemets nummer.
                    nummer: f.fakturanummer.clone(),
                    fakturadatum: f.fakturadatum.map(|d| d.to_string()).unwrap_or_default(),
                    förfallodag: f.förfallodag.map(|d| d.to_string()).unwrap_or_default(),
                    total: kr(f.total),
                    återstår: kr(f.återstår),
                    märke: match (f.status, f.förfallodag) {
                        (Status::Utfärdad | Status::Registrerad, Some(d)) => {
                            let m = crate::kundfakturor::server::märke_för(f, d, idag);
                            (
                                m.kod().into(),
                                m.namn_i(maya_domain::betalning::Reskontra::Kund).into(),
                            )
                        }
                        (Status::Upplåst, _) => ("upplast".into(), "Upplåst".into()),
                        _ => ("utkast".into(), "Utkast".into()),
                    },
                })
                .collect();
            Ok::<_, KundFel>(Kunddata {
                lista: lista
                    .iter()
                    .filter(|k| alla || k.status == Kundstatus::Aktiv)
                    .map(vy)
                    .collect(),
                valt,
                kan_raderas,
                fakturor,
                obetalt: kr(obetalt),
                tillgodo: if tillgodo > 0 {
                    kr(tillgodo)
                } else {
                    String::new()
                },
            })
        }
        .await;
        match resultat {
            Ok(data) => Ok(data),
            Err(KundFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                Ok(Kunddata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    fn vy(k: &Kund) -> KundVy {
        KundVy {
            id: k.id.to_string(),
            namn: k.namn.clone(),
            identitet: k.identitet.clone(),
            adress: k.adress.clone(),
            epost: k.epost.clone(),
            momsregnr: k.momsregnr.clone(),
            betalningsvillkor: k.betalningsvillkor.to_string(),
            omvänd_skattskyldighet: k.omvänd_skattskyldighet.kod().into(),
            arkiverad: k.status == Kundstatus::Arkiverad,
            skapad: tid(k.skapad),
        }
    }

    type Fält = Vec<(String, String)>;

    fn värde(fält: &Fält, namn: &str) -> Option<String> {
        fält.iter().find(|(k, _)| k == namn).map(|(_, v)| v.clone())
    }

    /// Tillbaka till formuläret på `sida` (som redan har en fråga eller inte) med det som
    /// användaren skrev; sidan prövar det igen med samma regler.
    fn tillbaka(sida: &str, fält: &Fält) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("form", "kund");
        for namn in FÄLT.map(|f| f.namn()) {
            query.append_pair(namn, &värde(fält, namn).unwrap_or_default());
        }
        let skiljetecken = if sida.contains('?') { '&' } else { '?' };
        Redirect::to(&format!("{sida}{skiljetecken}{}", query.finish())).into_response()
    }

    /// Den inloggade och företaget i sökvägen, eller svaret om något saknas.
    fn tolka(
        id: &str,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Result<(Inloggad, FöretagsId), Box<Response>> {
        let Some(Extension(inloggad)) = inloggad else {
            return Err(Box::new(Redirect::to("/").into_response()));
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Err(Box::new(ingen_åtkomst()));
        };
        Ok((inloggad, företag))
    }

    async fn skapa(
        State(state): State<Arc<KundState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let formulär = ur_fält(|namn| värde(&fält, namn));
        let sida = format!("/foretag/{företag}/kunder");
        match kund::skapa(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            &formulär,
        )
        .await
        {
            Ok(k) => Redirect::to(&format!("{sida}?k={k}&upplagd=1")).into_response(),
            Err(KundFel::Ogiltigt(_)) => tillbaka(&format!("{sida}/ny"), &fält),
            Err(e) => kundfel(e),
        }
    }

    async fn ändra(
        State(state): State<Arc<KundState>>,
        Path((id, k)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(k) = k.parse::<KundId>() else {
            return finns_inte();
        };
        let formulär = ur_fält(|namn| värde(&fält, namn));
        let sida = format!("/foretag/{företag}/kunder?k={k}");
        match kund::ändra(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            k,
            &formulär,
        )
        .await
        {
            Ok(()) => Redirect::to(&format!("{sida}&sparat=1")).into_response(),
            Err(KundFel::Ogiltigt(_)) => tillbaka(&format!("{sida}&andra=1"), &fält),
            Err(e) => kundfel(e),
        }
    }

    async fn arkivera(
        State(state): State<Arc<KundState>>,
        Path((id, k)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(k) = k.parse::<KundId>() else {
            return finns_inte();
        };
        match kund::arkivera(&state.db, &state.klocka, &inloggad.aktör, företag, k).await {
            Ok(()) => Redirect::to(&format!("/foretag/{företag}/kunder?k={k}&arkiverad=1"))
                .into_response(),
            Err(e) => kundfel(e),
        }
    }

    async fn radera(
        State(state): State<Arc<KundState>>,
        Path((id, k)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(k) = k.parse::<KundId>() else {
            return finns_inte();
        };
        let sida = format!("/foretag/{företag}/kunder");
        match kund::radera(&state.db, &state.klocka, &inloggad.aktör, företag, k).await {
            Ok(()) => Redirect::to(&format!("{sida}?raderad=1")).into_response(),
            Err(KundFel::KanInteRaderas(spärr)) => {
                Redirect::to(&format!("{sida}?k={k}&fel={}", spärr.kod())).into_response()
            }
            Err(e) => kundfel(e),
        }
    }

    fn kundfel(e: KundFel) -> Response {
        match e {
            KundFel::FinnsInte => finns_inte(),
            KundFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            e => serverfel(e),
        }
    }

    fn finns_inte() -> Response {
        text(StatusCode::NOT_FOUND, "Kunden finns inte.\n")
    }
}

/// Kundregistret: listan och den valda kunden.
#[component]
pub fn KundregisterSida() -> impl IntoView {
    sida(hämta_kunddata, |data| kundregistret_sida(data, false))
}

/// Kundregistret med formuläret för en ny kund.
#[component]
pub fn NyKundSida() -> impl IntoView {
    sida(hämta_kunddata, |data| kundregistret_sida(data, true))
}

fn kundregistret_sida(data: Kunddata, ny: bool) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let alla = fråga("visa").as_deref() == Some("alla");
    let notis = [
        ("upplagd", "Kunden lades upp och loggades."),
        ("sparat", "Sparat."),
        (
            "arkiverad",
            "Kunden arkiverades och visas inte längre i väljarna.",
        ),
        ("raderad", "Kunden raderades och raderingen loggades."),
    ]
    .into_iter()
    .find(|(namn, _)| fråga(namn).is_some())
    .map(|(_, text)| view! { <p class="notis" role="status">{text}</p> });
    let fel = fråga("fel")
        .and_then(|k| feltext(&k))
        .map(|text| view! { <p class="fel" role="alert">{text}</p> });
    let register = kundregistret(&bas);
    let filter = [("", "Aktiva", !alla), ("?visa=alla", "Alla", alla)]
        .into_iter()
        .map(|(q, namn, aktuellt)| {
            view! {
                <a href=format!("{register}{q}") aria-current=aktuellt.then_some("page")>
                    {namn}
                </a>
            }
        })
        .collect_view();
    let valt_id = data.valt.as_ref().map(|k| k.id.clone());
    let visa = if alla { "&visa=alla" } else { "" };
    let rader = data
        .lista
        .iter()
        .map(|k| {
            let aktuellt = (Some(&k.id) == valt_id.as_ref()).then_some("page");
            let arkiverad = k
                .arkiverad
                .then(|| view! { <span class="badge badge-okopplat">"Arkiverad"</span> });
            view! {
                <li>
                    <a href=format!("{register}?k={}{visa}", k.id) aria-current=aktuellt>
                        <span class="verifikatlista-topp">
                            <span>{k.namn.clone()}</span>
                            {arkiverad}
                        </span>
                        <span class="diskret">
                            {[k.identitet.clone(), k.epost.clone()]
                                .into_iter()
                                .filter(|d| !d.is_empty())
                                .collect::<Vec<_>>()
                                .join(" · ")}
                        </span>
                    </a>
                </li>
            }
        })
        .collect_view();
    let tom = data.lista.is_empty().then(|| {
        view! {
            <p class="diskret">
                {if alla { "Inga kunder." } else { "Inga aktiva kunder." }}
            </p>
        }
    });
    let tillbaka = fråga("form").as_deref() == Some("kund");
    let från_adressen = || ur_fält(|namn| fråga(namn));
    let detalj = match data.valt.clone() {
        _ if ny => {
            let start = if tillbaka {
                från_adressen()
            } else {
                NyKund::default()
            };
            view! {
                <KundFormulär
                    rubrik="Ny kund"
                    action=register.clone()
                    start=start
                    visa_fel=tillbaka
                    knapp="Lägg upp kunden"
                    avbryt=register.clone()
                />
            }
            .into_any()
        }
        Some(k) if fråga("andra").is_some() => {
            let start = if tillbaka {
                från_adressen()
            } else {
                k.formulär()
            };
            view! {
                <KundFormulär
                    rubrik="Ändra kunden"
                    action=format!("{register}/{}", k.id)
                    start=start
                    visa_fel=tillbaka
                    knapp="Spara kunden"
                    avbryt=format!("{register}?k={}", k.id)
                />
            }
            .into_any()
        }
        Some(k) => view! {
            <ValdKund
                register=register.clone()
                k=k
                kan_raderas=data.kan_raderas
                arkivera=fråga("arkivera").is_some()
                radera=fråga("radera").is_some()
            />
            <KundensFakturor
                bas=bas.clone()
                fakturor=data.fakturor.clone()
                obetalt=data.obetalt.clone()
                tillgodo=data.tillgodo.clone()
            />
        }
        .into_any(),
        None => view! {
            <div class="tom">
                <p>"Välj en kund i listan, eller lägg upp en ny."</p>
            </div>
        }
        .into_any(),
    };
    view! {
        <div class="verifikat-delad kundregister">
            <div class="verifikat-spalt">
                <div class="verifikat-spalt-rubrik">
                    <h1>"Kundregistret"</h1>
                    <a class="knapp" href=format!("{register}/ny")>
                        "Ny kund"
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

fn omvänd_namn(kod: &str) -> &'static str {
    OmvändSkattskyldighet::från_kod(kod).map_or("", OmvändSkattskyldighet::namn)
}

/// Den valda kunden: uppgifterna, Ändra, Arkivera och Radera.
#[component]
fn ValdKund(
    register: String,
    k: KundVy,
    kan_raderas: bool,
    arkivera: bool,
    radera: bool,
) -> impl IntoView {
    let här = format!("{register}?k={}", k.id);
    let uppgift = |v: &str| {
        if v.is_empty() {
            view! { <dd class="diskret">"Saknas"</dd> }.into_any()
        } else {
            view! { <dd>{v.to_string()}</dd> }.into_any()
        }
    };
    let adress = if k.adress.is_empty() {
        uppgift("")
    } else {
        let rader = k
            .adress
            .lines()
            .map(|r| view! { <span class="adressrad">{r.to_string()}</span> })
            .collect_view();
        view! { <dd>{rader}</dd> }.into_any()
    };
    let status = k
        .arkiverad
        .then(|| view! { <span class="badge badge-okopplat">"Arkiverad"</span> });
    let åtgärd = if arkivera && !k.arkiverad {
        view! {
            <section class="dialog" role="alertdialog" aria-labelledby="arkivera-rubrik">
                <h2 id="arkivera-rubrik">"Arkivera kunden?"</h2>
                <p>{k.namn.clone()}</p>
                <p class="diskret">
                    "Kunden finns kvar i registret men visas inte i väljarna. Arkiveringen loggas."
                </p>
                <form method="post" action=format!("{register}/{}/arkivera", k.id)>
                    <a class="knapp" href=här.clone()>
                        "Avbryt"
                    </a>
                    " "
                    <button class="knapp knapp-primar" type="submit">
                        "Arkivera kunden"
                    </button>
                </form>
            </section>
        }
        .into_any()
    } else if radera && kan_raderas {
        view! {
            <section class="dialog" role="alertdialog" aria-labelledby="radera-rubrik">
                <h2 id="radera-rubrik">"Radera kunden?"</h2>
                <p>{k.namn.clone()}</p>
                <p class="diskret">
                    "Kunden har inga fakturor. Raderingen loggas med kundens uppgifter."
                </p>
                <form method="post" action=format!("{register}/{}/radera", k.id)>
                    <a class="knapp" href=här.clone()>
                        "Avbryt"
                    </a>
                    " "
                    <button class="knapp knapp-fara" type="submit">
                        "Radera kunden"
                    </button>
                </form>
            </section>
        }
        .into_any()
    } else {
        let arkivera = (!k.arkiverad).then(|| {
            view! {
                <a class="knapp" href=format!("{här}&arkivera=1")>
                    "Arkivera…"
                </a>
            }
        });
        let radera = if kan_raderas {
            view! {
                <a class="knapp knapp-fara-diskret" href=format!("{här}&radera=1")>
                    "Radera…"
                </a>
            }
            .into_any()
        } else {
            view! {
                <span class="diskret">
                    "Kunden har fakturor och kan inte raderas, bara arkiveras."
                </span>
            }
            .into_any()
        };
        view! {
            <p class="knappar">
                <a class="knapp" href=format!("{här}&andra=1")>
                    "Ändra…"
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
                <p class="dokument-nummer">"Kund"</p>
                <h2>{k.namn.clone()}</h2>
                {status}
            </header>
            <dl class="metadata">
                <dt>"Orgnr eller personnr"</dt>
                {uppgift(&k.identitet)}
                <dt>"Adress"</dt>
                {adress}
                <dt>"E-post"</dt>
                {uppgift(&k.epost)}
                <dt>"Momsreg-nr"</dt>
                {uppgift(&k.momsregnr)}
                <dt>"Betalningsvillkor"</dt>
                <dd>{format!("{} dagar", k.betalningsvillkor)}</dd>
                <dt>"Omvänd skattskyldighet"</dt>
                <dd>{omvänd_namn(&k.omvänd_skattskyldighet)}</dd>
                <dt>"Upplagd"</dt>
                <dd>{k.skapad.clone()}</dd>
            </dl>
            {åtgärd}
        </article>
    }
}

/// Felet vid ett fält, när felen ska visas.
fn fel_vid(
    fel: Memo<Vec<Fältfel>>,
    visa: RwSignal<bool>,
    fält: Fält,
) -> impl Fn() -> Option<AnyView> + Send + Sync + 'static {
    move || {
        if !visa.get() {
            return None;
        }
        fel.with(|fel| {
            fel.iter().find(|f| f.fält == fält).map(|f| {
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

/// Formuläret för en ny eller ändrad kund. Felen visas vid fälten efter ett försök att spara.
#[component]
fn KundFormulär(
    rubrik: &'static str,
    action: String,
    start: NyKund,
    visa_fel: bool,
    knapp: &'static str,
    avbryt: String,
) -> impl IntoView {
    let kund = RwSignal::new(start.clone());
    let visa = RwSignal::new(visa_fel);
    let fel = Memo::new(move |_| kund.with(|k| k.kontrollera().err().unwrap_or_default()));
    let skicka = move |ev: leptos::ev::SubmitEvent| {
        if !fel.with_untracked(Vec::is_empty) {
            ev.prevent_default();
            visa.set(true);
        }
    };
    let fält = move |fält: Fält, etikett: &'static str, värde: String| {
        let namn = fält.namn();
        view! {
            <label for=namn>{etikett}</label>
            <input
                id=namn
                name=namn
                type="text"
                value=värde
                on:input=move |ev| kund.update(|k| sätt(k, fält, event_target_value(&ev)))
            />
            {fel_vid(fel, visa, fält)}
        }
    };
    let omvänd = start.omvänd_skattskyldighet.clone();
    let val = OmvändSkattskyldighet::ALLA
        .into_iter()
        .map(|o| {
            view! {
                <option value=o.kod() selected=o.kod() == omvänd>
                    {o.namn()}
                </option>
            }
        })
        .collect_view();
    view! {
        <form class="formular" method="post" action=action novalidate on:submit=skicka>
            <h2>{rubrik}</h2>
            {fält(Fält::Namn, "Namn", start.namn.clone())}
            {fält(Fält::Identitet, "Orgnr eller personnr", start.identitet.clone())}
            <label for="adress">"Adress (gata, postnummer och ort)"</label>
            <textarea
                id="adress"
                name="adress"
                rows="3"
                on:input=move |ev| kund.update(|k| k.adress = event_target_value(&ev))
            >
                {start.adress.clone()}
            </textarea>
            {fält(Fält::Epost, "E-post", start.epost.clone())}
            {fält(Fält::Momsregnr, "Momsreg-nr", start.momsregnr.clone())}
            {fält(Fält::Betalningsvillkor, "Betalningsvillkor, dagar", start.betalningsvillkor.clone())}
            <label for="omvand_skattskyldighet">"Omvänd skattskyldighet"</label>
            <select
                id="omvand_skattskyldighet"
                name="omvand_skattskyldighet"
                on:change=move |ev| {
                    kund.update(|k| k.omvänd_skattskyldighet = event_target_value(&ev))
                }
            >
                {val}
            </select>
            {fel_vid(fel, visa, Fält::OmvändSkattskyldighet)}
            <p class="knappar">
                <a class="knapp knapp-diskret" href=avbryt>
                    "Avbryt"
                </a>
                " "
                <button class="knapp knapp-primar" type="submit">
                    {knapp}
                </button>
            </p>
        </form>
    }
}

// #139 Betalningar
/// Kundens fakturor med status och det som återstår, och summan som är obetald (story 11).
#[component]
fn KundensFakturor(
    bas: String,
    fakturor: Vec<KundensFakturaVy>,
    obetalt: String,
    tillgodo: String,
) -> impl IntoView {
    let lista = crate::kundfakturor::kundfakturorna(&bas);
    let rader = fakturor
        .iter()
        .map(|f| {
            let nummer = if f.nummer.is_empty() {
                "Utkast".to_string()
            } else {
                f.nummer.clone()
            };
            view! {
                <tr>
                    <td>
                        <a href=format!("{lista}?f={}", f.id)>{nummer}</a>
                    </td>
                    <td>{f.fakturadatum.clone()}</td>
                    <td>{f.förfallodag.clone()}</td>
                    <td>
                        <crate::betalning::Märke kod=f.märke.0.clone() namn=f.märke.1.clone() />
                    </td>
                    <td class="belopp">{f.total.clone()}</td>
                    <td class="belopp">{f.återstår.clone()}</td>
                </tr>
            }
        })
        .collect_view();
    let innehåll = if fakturor.is_empty() {
        view! { <p class="diskret">"Kunden har inga fakturor än."</p> }.into_any()
    } else {
        view! {
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Faktura"</th>
                        <th>"Fakturadatum"</th>
                        <th>"Förfallodag"</th>
                        <th>"Status"</th>
                        <th class="belopp">"Totalt"</th>
                        <th class="belopp">"Återstår"</th>
                    </tr>
                </thead>
                <tbody>{rader}</tbody>
            </table>
        }
        .into_any()
    };
    view! {
        <section class="dokument kundens-fakturor">
            <h3>"Fakturor"</h3>
            <p>{format!("Obetalt {obetalt} kr")}</p>
            {(!tillgodo.is_empty()).then(|| view! { <p>{format!("Tillgodo {tillgodo} kr")}</p> })}
            {innehåll}
        </section>
    }
}
