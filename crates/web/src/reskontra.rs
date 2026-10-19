//! Rapporter → Bokföring → Reskontran per datum (#151, R4, A2): de öppna posterna per faktura
//! och avstämningen mot 1510 och 2440, med export som PDF och CSV.
//!
//! Dagen står i adressen (`datum`, i dag när den saknas) och väljs med ett vanligt
//! GET-formulär. Sidan läser sin data med `sidodata::sida`.

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::bokforing::Bokföringsmeny;
use crate::foretag::{Företagsval, IngenÅtkomst};
use crate::sidodata::sida;
use crate::utskrift::Utskrift;

/// Sökvägen under företaget.
pub const SÖKVÄG: &str = "/rapporter/reskontra";

/// En öppen post. Beloppen är formaterade.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostVy {
    pub fakturanummer: String,
    /// Fakturans dokument.
    pub länk: String,
    pub motpart: String,
    pub fakturadatum: String,
    pub förfallodag: String,
    pub total: String,
    pub återstår: String,
    pub på_kontot: String,
    pub läge: String,
}

/// En reskontra med avstämningen mot sitt konto.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelVy {
    pub rubrik: String,
    /// T.ex. "1510".
    pub konto: String,
    pub poster: Vec<PostVy>,
    pub återstår: String,
    pub på_kontot: String,
    pub saldo: String,
    pub differens: String,
    pub stämmer: bool,
}

/// Det som sidan visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reskontradata {
    /// `ÅÅÅÅ-MM-DD`; tomt utan åtkomst.
    pub datum: String,
    pub delar: Vec<DelVy>,
    /// Filnamnet utan ändelse.
    pub filnamn: String,
}

/// Datan för `/foretag/<id>/rapporter/reskontra`.
#[server(prefix = "/api", endpoint = "reskontradata", input = server_fn::codec::GetUrl)]
pub async fn hämta_reskontradata(adress: String) -> Result<Reskontradata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::reskontradata(&begäran).await,
        None => Ok(Reskontradata::default()),
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use crate::svar::{ingen_åtkomst, serverfel};
    use std::sync::Arc;

    use axum::extract::{Extension, Path, RawQuery, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Redirect, Response};
    use jiff::civil::Date;
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::betalning::Faktura;
    use maya_core::foretag::{self, FöretagsId};
    use maya_core::inloggning::Inloggad;
    use maya_core::reskontra::{self, Reskontradel};
    use maya_core::utskrift::UtskriftFel;
    use maya_core::{Databas, Klocka};
    use maya_domain::belopp::Saldo;
    use maya_domain::betalning::Reskontra;

    use super::{DelVy, PostVy, Reskontradata};
    use crate::bokforing::server::BokföringsState;
    use crate::sidodata::{self, Sidbegäran};

    /// Dagen i adressen (`datum`), eller i dag.
    fn dagen(query: Option<&str>, klocka: &Klocka) -> Date {
        url::form_urlencoded::parse(query.unwrap_or_default().as_bytes())
            .find(|(k, _)| k == "datum")
            .and_then(|(_, v)| v.trim().parse().ok())
            .unwrap_or_else(|| klocka.idag())
    }

    fn kr(öre: i64) -> String {
        Saldo::från_öre(öre).to_string()
    }

    fn del(företag: FöretagsId, d: &Reskontradel) -> DelVy {
        let (rubrik, flik) = match d.reskontra {
            Reskontra::Kund => ("Kundreskontra", "kundfakturor"),
            Reskontra::Leverantör => ("Leverantörsreskontra", "leverantorsfakturor"),
        };
        DelVy {
            rubrik: rubrik.into(),
            konto: d.reskontra.konto().to_string(),
            poster: d
                .poster
                .iter()
                .map(|p| PostVy {
                    fakturanummer: p.fakturanummer.clone(),
                    länk: match p.faktura {
                        Faktura::Kund(f) => format!("/foretag/{företag}/{flik}?f={f}"),
                        Faktura::Leverantör(f) => format!("/foretag/{företag}/{flik}?f={f}"),
                    },
                    motpart: p.motpart.clone(),
                    fakturadatum: p.fakturadatum.to_string(),
                    förfallodag: p.förfallodag.map(|d| d.to_string()).unwrap_or_default(),
                    total: kr(p.total),
                    återstår: kr(p.återstår),
                    på_kontot: kr(p.bokfört),
                    läge: p.läge.namn().into(),
                })
                .collect(),
            återstår: kr(d.återstår),
            på_kontot: kr(d.bokfört),
            saldo: kr(d.saldo),
            differens: kr(d.differens),
            stämmer: d.differens == 0,
        }
    }

    pub(crate) async fn reskontradata(b: &Sidbegäran) -> Result<Reskontradata, ServerFnError> {
        let delar = b.delar();
        let ["foretag", id, "rapporter", "reskontra"] = delar.as_slice() else {
            return Ok(Reskontradata::default());
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Ok(Reskontradata::default());
        };
        let dag = dagen(Some(b.fråga()), &b.klocka);
        match hämta(&b.db, &b.inloggad, företag, dag).await {
            Ok(data) => Ok(data),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                Ok(Reskontradata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    async fn hämta(
        db: &Databas,
        inloggad: &Inloggad,
        företag: FöretagsId,
        dag: Date,
    ) -> Result<Reskontradata, ÅtkomstFel> {
        let r = reskontra::reskontran(db, &inloggad.aktör, företag, dag).await?;
        let orgnr = foretag::företaget(db, &inloggad.aktör, företag)
            .await?
            .orgnr;
        Ok(Reskontradata {
            datum: dag.to_string(),
            delar: vec![del(företag, &r.kund), del(företag, &r.leverantör)],
            filnamn: reskontra::filnamnsstam(&orgnr.to_string(), dag),
        })
    }

    /// GET `/foretag/<id>/rapporter/reskontra/{pdf|csv}?datum=`. PDF:en är `inline` så att
    /// Skriv ut visar den; CSV:n laddas ned.
    pub(crate) async fn exportera(
        State(state): State<Arc<BokföringsState>>,
        Path((id, format)): Path<(String, String)>,
        RawQuery(query): RawQuery,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let dag = dagen(query.as_deref(), &state.klocka);
        let (db, klocka, aktör) = (&state.db, &state.klocka, &inloggad.aktör);
        let svar = match format.as_str() {
            "pdf" => reskontra::pdf(db, klocka, aktör, företag, dag)
                .await
                .map(crate::svar::pdf),
            "csv" => reskontra::csv(db, aktör, företag, dag)
                .await
                .map(|f| crate::svar::csv(&f.filnamn, f.innehåll))
                .map_err(UtskriftFel::Åtkomst),
            _ => return (StatusCode::NOT_FOUND, "Sidan finns inte.\n").into_response(),
        };
        match svar {
            Ok(svar) => svar,
            Err(UtskriftFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }
}

/// Rapporter → Bokföring → Reskontran per datum.
#[component]
pub fn ReskontraSida() -> impl IntoView {
    sida(hämta_reskontradata, reskontrasida)
}

fn reskontrasida(data: Reskontradata) -> AnyView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let Some(företaget) = use_context::<Företagsval>()
        .unwrap_or_default()
        .0
        .into_iter()
        .find(|f| f.id == id)
    else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    if data.datum.is_empty() {
        return view! { <IngenÅtkomst /> }.into_any();
    }
    let bas = företaget.href();
    let sökväg = format!("{bas}{SÖKVÄG}");
    let export = |format: &str| format!("{sökväg}/{format}?datum={}", data.datum);
    let delar = data
        .delar
        .iter()
        .map(|d| view! { <Reskontradelen d=d.clone() /> })
        .collect_view();
    view! {
        <section class="sida">
            <p class="diskret">{företaget.namn} " · " {företaget.orgnr}</p>
            <Bokföringsmeny bas=bas.clone() aktiv=SÖKVÄG />
            <h1>"Reskontran per " {data.datum.clone()}</h1>
            <form class="verktygsrad" method="get" action=sökväg.clone()>
                <label>
                    "Per datum " <input name="datum" type="date" value=data.datum.clone() />
                </label>
                <button class="knapp" type="submit">
                    "Visa"
                </button>
            </form>
            <div class="exporter">
                <Utskrift href=export("pdf") filnamn=format!("{}.pdf", data.filnamn) />
                <a
                    class="exportera-csv"
                    href=export("csv")
                    download=format!("{}.csv", data.filnamn)
                    rel="external"
                >
                    "Exportera CSV"
                </a>
            </div>
            <article class="dokument bokforingsdokument reskontran">
                <p class="diskret">
                    "Öppna poster vid dagens utgång. Det som står på kontot jämförs med saldot i huvudboken."
                </p>
                {delar}
            </article>
        </section>
    }
    .into_any()
}

/// En reskontras öppna poster och avstämningen mot kontot.
#[component]
fn Reskontradelen(d: DelVy) -> impl IntoView {
    let poster = if d.poster.is_empty() {
        view! { <p class="diskret">"Inga öppna poster."</p> }.into_any()
    } else {
        view! {
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Faktura"</th>
                        <th>"Motpart"</th>
                        <th>"Fakturadatum"</th>
                        <th>"Förfaller"</th>
                        <th class="belopp">"Total"</th>
                        <th class="belopp">"Återstår"</th>
                        <th class="belopp">"På kontot"</th>
                        <th>"Läge"</th>
                    </tr>
                </thead>
                <tbody>
                    {d
                        .poster
                        .iter()
                        .map(|p| {
                            view! {
                                <tr>
                                    <td>
                                        <a href=p.länk.clone()>{p.fakturanummer.clone()}</a>
                                    </td>
                                    <td>{p.motpart.clone()}</td>
                                    <td>{p.fakturadatum.clone()}</td>
                                    <td>{p.förfallodag.clone()}</td>
                                    <td class="belopp">{p.total.clone()}</td>
                                    <td class="belopp">{p.återstår.clone()}</td>
                                    <td class="belopp">{p.på_kontot.clone()}</td>
                                    <td>{p.läge.clone()}</td>
                                </tr>
                            }
                        })
                        .collect_view()}
                </tbody>
            </table>
        }
        .into_any()
    };
    let utfall = if d.stämmer {
        view! { <p class="avstamning stammer">"Reskontran stämmer med huvudboken."</p> }.into_any()
    } else {
        view! {
            <p class="avstamning avviker" role="alert">
                {format!("Reskontran avviker från {} med {} kr.", d.konto, d.differens)}
            </p>
        }
        .into_any()
    };
    view! {
        <section class="reskontradel">
            <h3>{d.rubrik.clone()}</h3>
            {poster}
            <dl class="fakta avstamningen">
                <dt>"Summa återstår"</dt>
                <dd>{d.återstår.clone()} " kr"</dd>
                <dt>{format!("Reskontrans belopp på {}", d.konto)}</dt>
                <dd>{d.på_kontot.clone()} " kr"</dd>
                <dt>{format!("Saldo på {} i huvudboken", d.konto)}</dt>
                <dd>{d.saldo.clone()} " kr"</dd>
                <dt>"Differens"</dt>
                <dd class="differens">{d.differens.clone()} " kr"</dd>
            </dl>
            {utfall}
        </section>
    }
}
