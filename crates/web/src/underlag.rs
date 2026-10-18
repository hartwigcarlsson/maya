//! Underlag: sidan där underlag laddas upp, visas, laddas ned och tas bort, och delarna som
//! verifikatformuläret och dokumentet använder (underlaget bredvid, variant C i
//! `prototype/32-verifikat`).
//!
//! Sidan läser sin data med `sidodata::sida`. Filen hämtas från
//! `/foretag/<id>/underlag/<u>/fil`; PDF och bilder visas i sidan, andra format laddas ned.

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use serde::{Deserialize, Serialize};

use crate::app::LoggaIn;
use crate::foretag::IngenÅtkomst;
use crate::sidodata::sida;
use crate::verifikat::{företagets_bas, inloggad};

/// Ett underlag som sidorna visar det.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnderlagVy {
    pub id: String,
    pub filnamn: String,
    pub mime: String,
    /// `pdf`, `bild` eller `nedladdning`.
    pub visning: String,
    pub storlek: String,
    pub sha256: String,
    /// Tid och namn.
    pub uppladdad: (String, String),
    /// För ett verifikats underlag: när och av vem, om kopplingen är en komplettering (VF6).
    pub komplettering: Option<(String, String)>,
    /// Verifikaten underlaget är kopplat till: id och nummer, eller `None` för ett utkast.
    pub kopplingar: Vec<(String, Option<String>)>,
    pub kan_tas_bort: bool,
    /// Andra underlag i företaget med samma innehåll: filnamn och när de laddades upp.
    pub dubbletter: Vec<(String, String)>,
}

/// Det som sidan Underlag visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Underlagsdata {
    pub lista: Vec<UnderlagVy>,
    pub valt: Option<UnderlagVy>,
}

/// Datan för `/foretag/<id>/underlag`.
#[server(prefix = "/api", endpoint = "underlagsdata", input = server_fn::codec::GetUrl)]
pub async fn hämta_underlagsdata(adress: String) -> Result<Underlagsdata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::underlagsdata(&begäran).await,
        None => Ok(Underlagsdata::default()),
    }
}

/// Felkoderna i adressen och deras texter.
pub fn feltext(kod: &str) -> Option<&'static str> {
    Some(match kod {
        "for_stort" => "Ett underlag får vara högst 25 MB.",
        "ingen_fil" => "Välj en fil att ladda upp.",
        "hor_till_bokfort" => "Underlaget hör till ett bokfört verifikat och kan inte tas bort.",
        "okant_underlag" => "Underlaget finns inte i företaget.",
        "redan_kopplat" => "Underlaget är redan kopplat till verifikatet.",
        _ => return None,
    })
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use crate::svar::{ingen_åtkomst, serverfel};
    use std::sync::Arc;

    use axum::body::Bytes;
    use axum::extract::{DefaultBodyLimit, FromRequest, Multipart, Path, Request, State};
    use axum::http::{HeaderValue, StatusCode, header};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::{get, post};
    use axum::{Extension, Form, Router};
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::underlag::{
        self, Underlag, UnderlagFel, UnderlagId, Urval, VerifikatetsUnderlag,
    };
    use maya_core::{Databas, Klocka};
    use maya_domain::underlag::{STÖRSTA_STORLEK, Visning, storlek, visning};

    use super::{UnderlagVy, Underlagsdata};
    use crate::sidodata::{self, Sidbegäran};
    use crate::verifikat::server::tid;

    #[derive(Clone)]
    pub(crate) struct UnderlagState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    /// Hur stor en begäran med ett underlag får vara: underlaget och resten av formuläret.
    pub(crate) const STÖRSTA_BEGÄRAN: usize = STÖRSTA_STORLEK + 1024 * 1024;

    pub(crate) fn routes(state: UnderlagState) -> Router {
        Router::new()
            .route("/foretag/{id}/underlag", post(ladda_upp))
            .route("/foretag/{id}/underlag/{u}/fil", get(filen))
            .route("/foretag/{id}/underlag/{u}/ta-bort", post(ta_bort))
            .layer(DefaultBodyLimit::max(STÖRSTA_BEGÄRAN))
            .with_state(Arc::new(state))
    }

    /// En fil i formuläret.
    pub(crate) struct Fil {
        pub filnamn: String,
        pub mime: String,
        pub innehåll: Bytes,
    }

    /// Ett formulär som `application/x-www-form-urlencoded` eller `multipart/form-data`.
    pub(crate) struct Formulär {
        pub fält: Vec<(String, String)>,
        /// Filen i fältet `fil`, om en valdes.
        pub fil: Option<Fil>,
        /// Begäran var större än vad ett underlag får vara.
        pub för_stort: bool,
    }

    impl Formulär {
        pub fn värde(&self, namn: &str) -> Option<&str> {
            self.fält
                .iter()
                .find(|(k, _)| k == namn)
                .map(|(_, v)| v.as_str())
        }

        pub fn värden(&self, namn: &str) -> Vec<String> {
            self.fält
                .iter()
                .filter(|(k, _)| k == namn)
                .map(|(_, v)| v.clone())
                .collect()
        }
    }

    /// Läser formuläret i begäran, med eller utan fil.
    pub(crate) async fn läs_formulär(req: Request) -> Result<Formulär, Box<Response>> {
        let multipart = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("multipart/form-data"));
        if !multipart {
            let Form(fält) = Form::<Vec<(String, String)>>::from_request(req, &())
                .await
                .map_err(|e| Box::new(e.into_response()))?;
            return Ok(Formulär {
                fält,
                fil: None,
                för_stort: false,
            });
        }
        let mut delar = Multipart::from_request(req, &())
            .await
            .map_err(|e| Box::new(e.into_response()))?;
        let mut formulär = Formulär {
            fält: Vec::new(),
            fil: None,
            för_stort: false,
        };
        loop {
            let del = match delar.next_field().await {
                Ok(Some(del)) => del,
                Ok(None) => break,
                Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                    formulär.för_stort = true;
                    break;
                }
                Err(e) => return Err(Box::new(e.into_response())),
            };
            let namn = del.name().unwrap_or_default().to_string();
            if namn == "fil" {
                let filnamn = del.file_name().unwrap_or_default().to_string();
                let mime = del.content_type().unwrap_or_default().to_string();
                let innehåll = match del.bytes().await {
                    Ok(b) => b,
                    Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                        formulär.för_stort = true;
                        break;
                    }
                    Err(e) => return Err(Box::new(e.into_response())),
                };
                // Ett formulär utan vald fil skickar en tom del utan filnamn.
                if !filnamn.is_empty() || !innehåll.is_empty() {
                    formulär.fil = Some(Fil {
                        filnamn,
                        mime,
                        innehåll,
                    });
                }
            } else {
                match del.text().await {
                    Ok(text) => formulär.fält.push((namn, text)),
                    Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                        formulär.för_stort = true;
                        break;
                    }
                    Err(e) => return Err(Box::new(e.into_response())),
                }
            }
        }
        Ok(formulär)
    }

    /// Hämtar det som sidan Underlag visar, för `/foretag/<id>/underlag`. Utan åtkomst blir
    /// datan tom; sidan visar det ur företagsvalet.
    pub(crate) async fn underlagsdata(b: &Sidbegäran) -> Result<Underlagsdata, ServerFnError> {
        let delar = b.delar();
        let ["foretag", id, "underlag"] = delar.as_slice() else {
            return Ok(Underlagsdata::default());
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Ok(Underlagsdata::default());
        };
        let urval = if b.värde("visa").as_deref() == Some("alla") {
            Urval::Alla
        } else {
            Urval::Okopplade
        };
        let aktör = &b.inloggad.aktör;
        let resultat = async {
            let lista = underlag::lista(&b.db, aktör, företag, urval).await?;
            let valt = match b.värde("u").and_then(|u| u.parse::<UnderlagId>().ok()) {
                Some(u) => underlag::underlaget(&b.db, aktör, företag, u).await?,
                None => None,
            };
            Ok::<_, ÅtkomstFel>(Underlagsdata {
                lista: lista.iter().map(vy).collect(),
                valt: valt.as_ref().map(vy),
            })
        }
        .await;
        match resultat {
            Ok(data) => Ok(data),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                Ok(Underlagsdata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    fn visningens_namn(mime: &str) -> String {
        match visning(mime) {
            Visning::Pdf => "pdf",
            Visning::Bild => "bild",
            Visning::Nedladdning => "nedladdning",
        }
        .into()
    }

    pub(crate) fn vy(u: &Underlag) -> UnderlagVy {
        UnderlagVy {
            id: u.id.to_string(),
            filnamn: u.filnamn.clone(),
            mime: u.mime.clone(),
            visning: visningens_namn(&u.mime),
            storlek: storlek(u.storlek),
            sha256: u.sha256.clone(),
            uppladdad: (tid(u.uppladdad.tid), u.uppladdad.namn.clone()),
            komplettering: None,
            kopplingar: u
                .kopplingar
                .iter()
                .map(|k| (k.verifikat.to_string(), k.nummer.map(|n| n.to_string())))
                .collect(),
            kan_tas_bort: u.kan_tas_bort(),
            dubbletter: u
                .dubbletter
                .iter()
                .map(|d| (d.filnamn.clone(), tid(d.uppladdad)))
                .collect(),
        }
    }

    /// Ett verifikats underlag, med kompletteringens tid och vem.
    pub(crate) fn verifikatets_vy(u: &VerifikatetsUnderlag) -> UnderlagVy {
        UnderlagVy {
            id: u.id.to_string(),
            filnamn: u.filnamn.clone(),
            mime: u.mime.clone(),
            visning: visningens_namn(&u.mime),
            storlek: storlek(u.storlek),
            sha256: u.sha256.clone(),
            uppladdad: Default::default(),
            komplettering: u
                .komplettering
                .then(|| (tid(u.kopplad.tid), u.kopplad.namn.clone())),
            kopplingar: Vec::new(),
            kan_tas_bort: false,
            dubbletter: Vec::new(),
        }
    }

    /// Laddar upp filen i fältet `fil` som ett okopplat underlag.
    async fn ladda_upp(
        State(state): State<Arc<UnderlagState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        req: Request,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let sida = format!("/foretag/{företag}/underlag");
        let formulär = match läs_formulär(req).await {
            Ok(f) => f,
            Err(r) => return *r,
        };
        if formulär.för_stort {
            return Redirect::to(&format!("{sida}?fel=for_stort")).into_response();
        }
        let Some(fil) = formulär.fil else {
            return Redirect::to(&format!("{sida}?fel=ingen_fil")).into_response();
        };
        match underlag::ladda_upp(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            &fil.filnamn,
            &fil.mime,
            &fil.innehåll,
        )
        .await
        {
            Ok(u) => {
                let dubblett = if u.dubbletter.is_empty() {
                    ""
                } else {
                    "&dubblett=1"
                };
                Redirect::to(&format!("{sida}?u={}&uppladdat=1{dubblett}", u.id)).into_response()
            }
            Err(UnderlagFel::FörStort) => {
                Redirect::to(&format!("{sida}?fel=for_stort")).into_response()
            }
            Err(e) => underlagsfel(e),
        }
    }

    /// Originalfilen: PDF och bilder visas i sidan, andra format laddas ned.
    async fn filen(
        State(state): State<Arc<UnderlagState>>,
        Path((id, u)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        uri: axum::http::Uri,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return (StatusCode::UNAUTHORIZED, "Logga in först.\n").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let Ok(u) = u.parse::<UnderlagId>() else {
            return finns_inte();
        };
        let fil = match underlag::filen(&state.db, &inloggad.aktör, företag, u).await {
            Ok(Some(fil)) => fil,
            Ok(None) => return finns_inte(),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                return ingen_åtkomst();
            }
            Err(e) => return serverfel(e),
        };
        let ladda_ned = uri.query().is_some_and(|q| q.contains("ladda_ned=1"));
        let sätt = if ladda_ned || visning(&fil.mime) == Visning::Nedladdning {
            "attachment"
        } else {
            "inline"
        };
        let mime = if sätt == "inline" {
            fil.mime.as_str()
        } else {
            "application/octet-stream"
        };
        let mut response = fil.innehåll.into_response();
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(mime)
                .unwrap_or(HeaderValue::from_static("application/octet-stream")),
        );
        if let Ok(v) = HeaderValue::from_str(&innehållsdisposition(sätt, &fil.filnamn)) {
            headers.insert(header::CONTENT_DISPOSITION, v);
        }
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-cache"),
        );
        response
    }

    /// `Content-Disposition` med filnamnet både som ASCII och som UTF-8 (RFC 6266).
    fn innehållsdisposition(sätt: &str, filnamn: &str) -> String {
        let ascii: String = filnamn
            .chars()
            .map(|c| {
                if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let mut utf8 = String::new();
        for b in filnamn.bytes() {
            if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
                utf8.push(char::from(b));
            } else {
                utf8.push_str(&format!("%{b:02X}"));
            }
        }
        format!("{sätt}; filename=\"{ascii}\"; filename*=UTF-8''{utf8}")
    }

    /// Tar bort ett underlag som aldrig har hört till ett bokfört verifikat.
    async fn ta_bort(
        State(state): State<Arc<UnderlagState>>,
        Path((id, u)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let Ok(u) = u.parse::<UnderlagId>() else {
            return finns_inte();
        };
        let sida = format!("/foretag/{företag}/underlag");
        match underlag::ta_bort(&state.db, &state.klocka, &inloggad.aktör, företag, u).await {
            Ok(()) => Redirect::to(&format!("{sida}?borttaget=1")).into_response(),
            Err(UnderlagFel::HörTillBokfört) => {
                Redirect::to(&format!("{sida}?u={u}&fel=hor_till_bokfort")).into_response()
            }
            Err(e) => underlagsfel(e),
        }
    }

    pub(crate) fn underlagsfel(e: UnderlagFel) -> Response {
        match e {
            UnderlagFel::FinnsInte => finns_inte(),
            UnderlagFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            e => serverfel(e),
        }
    }

    fn finns_inte() -> Response {
        (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Underlaget finns inte.\n",
        )
            .into_response()
    }
}

/// Lucides gem (`paperclip`).
#[component]
pub fn Gem() -> impl IntoView {
    view! {
        <svg
            class="ikon"
            xmlns="http://www.w3.org/2000/svg"
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
        >
            <path d="m16 6-8.414 8.586a2 2 0 0 0 2.829 2.829l8.414-8.586a4 4 0 1 0-5.657-5.657l-8.379 8.551a6 6 0 1 0 8.485 8.485l8.379-8.551" />
        </svg>
    }
}

/// Underlaget som det visas i Maya: PDF i en ram, en bild, eller en länk för att ladda ned.
#[component]
pub fn Förhandsvisning(bas: String, u: UnderlagVy) -> impl IntoView {
    let fil = format!("{bas}/underlag/{}/fil", u.id);
    let ladda_ned = format!("{fil}?ladda_ned=1");
    let visning = match u.visning.as_str() {
        "pdf" => view! {
            <iframe class="forhandsvisning" src=fil title=u.filnamn.clone()></iframe>
        }
        .into_any(),
        "bild" => view! {
            <img class="forhandsvisning" src=fil alt=u.filnamn.clone() />
        }
        .into_any(),
        _ => view! {
            <div class="forhandsvisning tom-visning">
                <p>{u.filnamn.clone()}</p>
                <p class="diskret">"Formatet visas inte i Maya."</p>
            </div>
        }
        .into_any(),
    };
    view! {
        {visning}
        <p class="diskret">
            {u.storlek.clone()}
            " · "
            <a href=ladda_ned download=u.filnamn.clone()>
                "Ladda ned"
            </a>
        </p>
    }
}

/// Sidan Underlag: ladda upp, de okopplade underlagen och det valda underlaget.
#[component]
pub fn UnderlagSida() -> impl IntoView {
    sida(hämta_underlagsdata, underlagssida)
}

fn underlagssida(data: Underlagsdata) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let alla = fråga("visa").as_deref() == Some("alla");
    let notis = if fråga("borttaget").is_some() {
        Some("Underlaget togs bort och borttagningen loggades.")
    } else if fråga("uppladdat").is_some() {
        Some("Underlaget laddades upp och är arkiverat.")
    } else {
        None
    }
    .map(|text| view! { <p class="notis" role="status">{text}</p> });
    let fel = fråga("fel")
        .and_then(|k| feltext(&k))
        .map(|text| view! { <p class="fel" role="alert">{text}</p> });
    let filter = [("", "Okopplade", !alla), ("?visa=alla", "Alla", alla)]
        .into_iter()
        .map(|(q, namn, aktuellt)| {
            view! {
                <a href=format!("{bas}/underlag{q}") aria-current=aktuellt.then_some("page")>
                    {namn}
                </a>
            }
        })
        .collect_view();
    let valt_id = data.valt.as_ref().map(|u| u.id.clone());
    let visa = if alla { "&visa=alla" } else { "" };
    let rader = data
        .lista
        .iter()
        .map(|u| {
            let aktuellt = (Some(&u.id) == valt_id.as_ref()).then_some("page");
            view! {
                <li>
                    <a href=format!("{bas}/underlag?u={}{visa}", u.id) aria-current=aktuellt>
                        <span class="verifikatlista-topp">
                            <span>
                                <Gem />
                                " "
                                {u.filnamn.clone()}
                            </span>
                            <span class="diskret">{u.storlek.clone()}</span>
                        </span>
                        <span class="diskret">"uppladdat " {u.uppladdad.0.clone()}</span>
                    </a>
                </li>
            }
        })
        .collect_view();
    let tom = data.lista.is_empty().then(|| {
        view! {
            <p class="diskret">
                {if alla { "Inga underlag." } else { "Inga okopplade underlag." }}
            </p>
        }
    });
    let detalj = match data.valt.clone() {
        Some(u) => view! {
            <ValtUnderlag bas=bas.clone() u=u ta_bort=fråga("ta_bort").is_some() dubblett=fråga("dubblett").is_some() />
        }
        .into_any(),
        None => view! {
            <div class="tom">
                <p>"Välj ett underlag i listan, eller ladda upp ett nytt."</p>
            </div>
        }
        .into_any(),
    };
    view! {
        <div class="verifikat-delad">
            <div class="verifikat-spalt">
                <div class="verifikat-spalt-rubrik">
                    <h1>"Underlag"</h1>
                </div>
                <form
                    class="uppladdning"
                    method="post"
                    action=format!("{bas}/underlag")
                    enctype="multipart/form-data"
                >
                    <label class="slapp">
                        <span>"Släpp en fil här eller välj en fil"</span>
                        <span class="diskret">"Högst 25 MB, valfritt format"</span>
                        <input type="file" name="fil" required />
                    </label>
                    <button class="knapp knapp-primar" type="submit">
                        "Ladda upp underlag"
                    </button>
                </form>
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

/// Det valda underlaget: förhandsvisning, metadata, kopplingar och Ta bort.
#[component]
fn ValtUnderlag(bas: String, u: UnderlagVy, ta_bort: bool, dubblett: bool) -> impl IntoView {
    let dubblettvarning = (dubblett || !u.dubbletter.is_empty()).then(|| {
        let andra = u
            .dubbletter
            .iter()
            .map(|(namn, tid)| format!("{namn} (uppladdat {tid})"))
            .collect::<Vec<_>>()
            .join(", ");
        view! {
            <p class="notis varningsnotis" role="status">
                "Samma fil finns redan i företaget: " {andra} "."
            </p>
        }
    });
    let kopplingar = if u.kopplingar.is_empty() {
        view! { <dd>"Okopplat"</dd> }.into_any()
    } else {
        u.kopplingar
            .iter()
            .map(|(id, nummer)| {
                let namn = nummer.clone().unwrap_or_else(|| "Utkast".into());
                view! {
                    <dd>
                        <a href=format!("{bas}/verifikat?v={id}")>
                            {namn}
                        </a>
                    </dd>
                }
            })
            .collect_view()
            .into_any()
    };
    let åtgärd = if !u.kan_tas_bort {
        view! {
            <p class="diskret">
                "Underlaget hör till ett bokfört verifikat och kan inte tas bort."
            </p>
        }
        .into_any()
    } else if ta_bort {
        view! {
            <section class="dialog" role="alertdialog" aria-labelledby="ta-bort-rubrik">
                <h2 id="ta-bort-rubrik">"Ta bort underlaget?"</h2>
                <p>{u.filnamn.clone()} " · " {u.storlek.clone()}</p>
                <p class="diskret">
                    "Borttagningen loggas. Utkast som underlaget är kopplat till behåller sina rader."
                </p>
                <form method="post" action=format!("{bas}/underlag/{}/ta-bort", u.id)>
                    <a class="knapp" href=format!("{bas}/underlag?u={}", u.id)>
                        "Avbryt"
                    </a>
                    " "
                    <button class="knapp knapp-fara" type="submit">
                        "Ta bort underlag"
                    </button>
                </form>
            </section>
        }
        .into_any()
    } else {
        view! {
            <p>
                <a
                    class="knapp knapp-fara-diskret"
                    href=format!("{bas}/underlag?u={}&ta_bort=1", u.id)
                >
                    "Ta bort underlag"
                </a>
            </p>
        }
        .into_any()
    };
    view! {
        <article class="underlagsdokument">
            <h2>{u.filnamn.clone()}</h2>
            {dubblettvarning}
            <Förhandsvisning bas=bas.clone() u=u.clone() />
            <dl class="metadata">
                <dt>"Uppladdat"</dt>
                <dd>{u.uppladdad.0.clone()} " av " {u.uppladdad.1.clone()}</dd>
                <dt>"Typ"</dt>
                <dd>{u.mime.clone()}</dd>
                <dt>"SHA-256"</dt>
                <dd class="hash">{u.sha256.clone()}</dd>
                <dt>"Verifikat"</dt>
                {kopplingar}
            </dl>
            {åtgärd}
        </article>
    }
}
