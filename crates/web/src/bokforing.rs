//! Rapporter → Bokföring: grundboken och huvudboken som dokument (L4, A2), med panelen till
//! verifikatet och export som PDF och CSV. Kontoplanen hör också till Bokföring men ligger
//! under Inställningar.
//!
//! Urvalet står i adressen (`ar`, `fran`, `till`, `konton`, `v` för panelen) och väljs med ett
//! vanligt GET-formulär. Datan kommer från middleware vid serverrenderingen, så länkarna hit har
//! `rel="external"`: klientens router skulle annars rita sidan utan data.

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{FöretagVy, Företagsval, IngenÅtkomst};
use crate::utskrift::Utskrift;

/// Sidorna under Bokföring: sökväg under företaget och namn.
const SIDOR: [(&str, &str); 3] = [
    ("/rapporter/grundbok", "Grundbok"),
    ("/rapporter/huvudbok", "Huvudbok"),
    ("/installningar/kontoplan", "Kontoplan"),
];

/// Bokföringens sidor som en meny, på Rapporter och på sidorna själva.
#[component]
pub fn Bokföringsmeny(bas: String, aktiv: &'static str) -> impl IntoView {
    view! {
        <nav class="installningsmeny" aria-label="Bokföring">
            {SIDOR
                .into_iter()
                .map(|(sökväg, namn)| {
                    let aktuell = (sökväg == aktiv).then_some("page");
                    view! {
                        <a rel="external" href=format!("{bas}{sökväg}") aria-current=aktuell>
                            {namn}
                        </a>
                    }
                })
                .collect_view()}
        </nav>
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadVy {
    pub konto: String,
    pub kontonamn: String,
    pub text: String,
    pub debet: String,
    pub kredit: String,
}

/// Ett verifikat i grundboken eller panelen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostVy {
    pub id: String,
    pub nummer: String,
    pub datum: String,
    pub beskrivning: String,
    /// "2026-03-25 10:00 av Anna".
    pub bokförd: String,
    pub summa: String,
    pub rader: Vec<RadVy>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodVy {
    pub namn: String,
    pub verifikat: Vec<PostVy>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransaktionVy {
    pub id: String,
    pub nummer: String,
    pub datum: String,
    pub text: String,
    pub debet: String,
    pub kredit: String,
    pub saldo: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KontoVy {
    pub rubrik: String,
    pub ingående: String,
    pub transaktioner: Vec<TransaktionVy>,
    pub utgående: String,
}

/// Det som sidan visar. Middleware hämtar det ur `core` och lägger det på begäran, och det
/// följer med till webbläsaren vid hydreringen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bokföringsdata {
    /// Räkenskapsåren som start och slut `ÅÅÅÅ-MM-DD`.
    pub år: Vec<(String, String)>,
    pub valt_år: String,
    /// Det valda årets perioder som `ÅÅÅÅ-MM` och namn.
    pub perioder: Vec<(String, String)>,
    pub från: String,
    pub till: String,
    /// Kontourvalet som det skrevs.
    pub konton: String,
    /// Kontourvalet gick inte att tolka; huvudboken visar då alla konton.
    pub konton_fel: bool,
    pub preliminär: bool,
    pub grundbok: Vec<PeriodVy>,
    pub huvudbok: Vec<KontoVy>,
    pub panel: Option<PostVy>,
    /// Exportens adresser (utan ändelse) och filnamn utan ändelse.
    pub export: (String, String),
    /// Urvalet som query, för länkar som ska behålla det.
    pub query: String,
}

fn data() -> Bokföringsdata {
    SharedValue::new(|| {
        #[cfg(feature = "ssr")]
        if let Some(parts) = use_context::<axum::http::request::Parts>()
            && let Some(data) = parts.extensions.get::<Bokföringsdata>()
        {
            return data.clone();
        }
        Bokföringsdata::default()
    })
    .into_inner()
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::Router;
    use axum::extract::{Extension, Path, RawQuery, Request, State};
    use axum::http::{StatusCode, header};
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::get;
    use jiff::Timestamp;
    use jiff::civil::Date;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::bokforing::{self, Bokföringsrad, Fil, Periodurval, RapportFel};
    use maya_core::foretag::{self, FöretagsId};
    use maya_core::inloggning::Inloggad;
    use maya_core::saldo::Status;
    use maya_core::utskrift::Pdf;
    use maya_core::verifikat::{self, VerifikatId};
    use maya_core::{Databas, Klocka};
    use maya_domain::belopp::Belopp;
    use maya_domain::kontourval::Kontourval;
    use maya_domain::rakenskapsar::periodnamn;

    use super::{Bokföringsdata, KontoVy, PeriodVy, PostVy, RadVy, TransaktionVy};

    #[derive(Clone)]
    pub(crate) struct BokföringsState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: BokföringsState) -> Router {
        Router::new()
            .route("/foretag/{id}/rapporter/{rapport}/{format}", get(exportera))
            .with_state(Arc::new(state))
    }

    /// Urvalet i adressen.
    #[derive(Default)]
    struct Fråga {
        år: Option<Date>,
        från: Option<Date>,
        till: Option<Date>,
        konton: String,
        v: Option<String>,
    }

    impl Fråga {
        fn tolka(query: Option<&str>) -> Self {
            let mut f = Fråga::default();
            let månad = |v: &str| format!("{v}-01").parse::<Date>().ok();
            for (k, v) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
                match &*k {
                    "ar" => f.år = v.parse().ok(),
                    "fran" => f.från = månad(&v),
                    "till" => f.till = månad(&v),
                    "konton" => f.konton = v.into_owned(),
                    "v" => f.v = Some(v.into_owned()),
                    _ => {}
                }
            }
            f
        }
    }

    /// Räkenskapsåret i frågan, eller det som innehåller dagens datum, eller det senaste.
    /// Perioder utanför året släpps, så att ett byte av år inte ger ett fel.
    async fn urval(
        db: &Databas,
        klocka: &Klocka,
        inloggad: &Inloggad,
        företag: FöretagsId,
        fråga: &Fråga,
    ) -> Result<(Vec<foretag::Räkenskapsår>, Periodurval), ÅtkomstFel> {
        let åren = foretag::företaget(db, &inloggad.aktör, företag)
            .await?
            .räkenskapsår;
        let idag = klocka.idag();
        let år = fråga
            .år
            .and_then(|a| åren.iter().find(|å| å.start == a))
            .or_else(|| åren.iter().find(|å| å.start <= idag && idag <= å.slut))
            .or(åren.last())
            .ok_or(ÅtkomstFel::IngenÅtkomst)?;
        let i_året = |d: Option<Date>| d.filter(|d| år.start <= *d && *d <= år.slut);
        let (mut från, mut till) = (i_året(fråga.från), i_året(fråga.till));
        if let (Some(f), Some(t)) = (från, till)
            && f > t
        {
            (från, till) = (Some(t), Some(f));
        }
        Ok((
            åren.clone(),
            Periodurval {
                år: år.start,
                från,
                till,
            },
        ))
    }

    /// Hämtar det som sidan visar, för `/foretag/<id>/rapporter/grundbok|huvudbok`. Körs efter
    /// `inloggning::identifiera`.
    pub(crate) async fn lägg_på_bokföringsdata(
        State(state): State<BokföringsState>,
        mut req: Request,
        next: Next,
    ) -> Response {
        let Some(inloggad) = req.extensions().get::<Inloggad>().cloned() else {
            return next.run(req).await;
        };
        let delar: Vec<&str> = req.uri().path().trim_matches('/').split('/').collect();
        let [
            "foretag",
            id,
            "rapporter",
            rapport @ ("grundbok" | "huvudbok"),
        ] = delar.as_slice()
        else {
            return next.run(req).await;
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return next.run(req).await;
        };
        let fråga = Fråga::tolka(req.uri().query());
        let huvudbok = *rapport == "huvudbok";
        match hämta(&state, &inloggad, företag, &fråga, huvudbok).await {
            Ok(data) => {
                req.extensions_mut().insert(data);
            }
            // Sidan visar att användaren saknar åtkomst, ur företagsvalet.
            Err(RapportFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
            }
            Err(e) => return serverfel(e),
        }
        next.run(req).await
    }

    async fn hämta(
        state: &BokföringsState,
        inloggad: &Inloggad,
        företag: FöretagsId,
        fråga: &Fråga,
        huvudbok: bool,
    ) -> Result<Bokföringsdata, RapportFel> {
        let (db, aktör) = (&state.db, &inloggad.aktör);
        let (åren, urval) = urval(db, &state.klocka, inloggad, företag, fråga).await?;
        let kontourval = Kontourval::tolka(&fråga.konton);
        let år = åren
            .iter()
            .find(|å| å.start == urval.år)
            .ok_or(RapportFel::OkäntRäkenskapsår)?;
        let månad = |d: Date| d.strftime("%Y-%m").to_string();
        let mut data = Bokföringsdata {
            år: åren
                .iter()
                .map(|å| (å.start.to_string(), å.slut.to_string()))
                .collect(),
            valt_år: urval.år.to_string(),
            perioder: år
                .perioder
                .iter()
                .map(|p| (månad(p.start), periodnamn(p.start)))
                .collect(),
            från: urval.från.map(månad).unwrap_or_default(),
            till: urval.till.map(månad).unwrap_or_default(),
            konton: fråga.konton.clone(),
            konton_fel: kontourval.is_none(),
            ..Default::default()
        };
        let perioder: Vec<_> = if huvudbok {
            let h = bokforing::huvudbok(
                db,
                aktör,
                företag,
                &urval,
                &kontourval.clone().unwrap_or_default(),
            )
            .await?;
            data.preliminär = h.status == Status::Preliminär;
            data.huvudbok = h
                .konton
                .iter()
                .map(|k| KontoVy {
                    rubrik: format!("{} {}", k.konto, k.namn),
                    ingående: k.ingående.to_string(),
                    utgående: k.utgående.to_string(),
                    transaktioner: k
                        .transaktioner
                        .iter()
                        .map(|t| TransaktionVy {
                            id: t.id.to_string(),
                            nummer: t.nummer.to_string(),
                            datum: t.datum.to_string(),
                            text: t.text.clone(),
                            debet: belopp(t.debet),
                            kredit: belopp(t.kredit),
                            saldo: t.saldo.to_string(),
                        })
                        .collect(),
                })
                .collect();
            h.perioder
        } else {
            let g = bokforing::grundbok(db, aktör, företag, &urval).await?;
            data.grundbok = g
                .perioder
                .iter()
                .map(|p| PeriodVy {
                    namn: periodnamn(p.period.start),
                    verifikat: p
                        .verifikat
                        .iter()
                        .map(|v| PostVy {
                            id: v.id.to_string(),
                            nummer: v.nummer.to_string(),
                            datum: v.datum.to_string(),
                            beskrivning: v.beskrivning.clone(),
                            bokförd: format!("{} av {}", tid(v.bokförd.tid), v.bokförd.namn),
                            summa: v.summa.to_string(),
                            rader: v.rader.iter().map(rad).collect(),
                        })
                        .collect(),
                })
                .collect();
            g.perioder.iter().map(|p| p.period).collect()
        };
        if let Some(id) = fråga
            .v
            .as_deref()
            .and_then(|v| v.parse::<VerifikatId>().ok())
        {
            data.panel = panel(db, inloggad, företag, id).await?;
        }
        data.query = {
            let mut query = url::form_urlencoded::Serializer::new(String::new());
            query.append_pair("ar", &data.valt_år);
            if !data.från.is_empty() {
                query.append_pair("fran", &data.från);
            }
            if !data.till.is_empty() {
                query.append_pair("till", &data.till);
            }
            if huvudbok && !fråga.konton.is_empty() {
                query.append_pair("konton", &fråga.konton);
            }
            query.finish()
        };
        let rapport = if huvudbok { "huvudbok" } else { "grundbok" };
        let sista = perioder.last().map_or(år.slut, |p| p.start);
        let orgnr = foretag::företaget(db, aktör, företag).await?.orgnr;
        data.export = (
            format!("/foretag/{företag}/rapporter/{rapport}"),
            format!(
                "{rapport}_{}_{}_{}",
                orgnr.to_string().replace('-', ""),
                år.start.strftime("%Y"),
                sista.strftime("%Y%m")
            ),
        );
        Ok(data)
    }

    /// Ett bokfört verifikat i panelen. Utkast och andra företags verifikat visas inte.
    async fn panel(
        db: &Databas,
        inloggad: &Inloggad,
        företag: FöretagsId,
        id: VerifikatId,
    ) -> Result<Option<PostVy>, ÅtkomstFel> {
        let Some(v) = verifikat::verifikatet(db, &inloggad.aktör, företag, id).await? else {
            return Ok(None);
        };
        let (Some(nummer), Some(bokförd)) = (v.nummer, v.bokförd) else {
            return Ok(None);
        };
        let konton = maya_core::kontoplan::konton(db, &inloggad.aktör, företag).await?;
        Ok(Some(PostVy {
            id: v.id.to_string(),
            nummer: nummer.to_string(),
            datum: v.datum.map(|d| d.to_string()).unwrap_or_default(),
            beskrivning: v.beskrivning,
            bokförd: format!("{} av {}", tid(bokförd.tid), bokförd.namn),
            summa: v.summa.to_string(),
            rader: v
                .rader
                .iter()
                .filter_map(|r| {
                    let konto = r.konto?;
                    Some(rad(&Bokföringsrad {
                        konto,
                        kontonamn: konton
                            .iter()
                            .find(|k| k.nummer == konto)
                            .map(|k| k.namn.clone())
                            .unwrap_or_default(),
                        text: r.text.clone(),
                        debet: r.debet,
                        kredit: r.kredit,
                    }))
                })
                .collect(),
        }))
    }

    fn rad(r: &Bokföringsrad) -> RadVy {
        RadVy {
            konto: r.konto.to_string(),
            kontonamn: r.kontonamn.clone(),
            text: r.text.clone().unwrap_or_default(),
            debet: belopp(r.debet),
            kredit: belopp(r.kredit),
        }
    }

    fn belopp(b: Belopp) -> String {
        if b == Belopp::NOLL {
            String::new()
        } else {
            b.to_string()
        }
    }

    fn tid(t: Timestamp) -> String {
        t.to_zoned(maya_domain::tid::stockholm())
            .strftime("%Y-%m-%d %H:%M")
            .to_string()
    }

    /// GET `/foretag/<id>/rapporter/{grundbok|huvudbok}/{pdf|csv}?<urvalet>`. PDF:en är
    /// `inline` så att Skriv ut visar den; CSV:n laddas ned.
    async fn exportera(
        State(state): State<Arc<BokföringsState>>,
        Path((id, rapport, format)): Path<(String, String, String)>,
        RawQuery(query): RawQuery,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let fråga = Fråga::tolka(query.as_deref());
        let urval = match urval(&state.db, &state.klocka, &inloggad, företag, &fråga).await {
            Ok((_, urval)) => urval,
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                return ingen_åtkomst();
            }
            Err(e) => return serverfel(e),
        };
        let konton = Kontourval::tolka(&fråga.konton).unwrap_or_default();
        let (db, klocka, aktör) = (&state.db, &state.klocka, &inloggad.aktör);
        let svar = match (rapport.as_str(), format.as_str()) {
            ("grundbok", "pdf") => bokforing::grundbok_pdf(db, klocka, aktör, företag, &urval)
                .await
                .map(pdf),
            ("huvudbok", "pdf") => {
                bokforing::huvudbok_pdf(db, klocka, aktör, företag, &urval, &konton)
                    .await
                    .map(pdf)
            }
            ("grundbok", "csv") => bokforing::grundbok_csv(db, aktör, företag, &urval)
                .await
                .map(csv),
            ("huvudbok", "csv") => bokforing::huvudbok_csv(db, aktör, företag, &urval, &konton)
                .await
                .map(csv),
            _ => return (StatusCode::NOT_FOUND, "Sidan finns inte.\n").into_response(),
        };
        match svar {
            Ok(svar) => svar,
            Err(RapportFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    fn pdf(pdf: Pdf) -> Response {
        fil(
            "application/pdf",
            format!(r#"inline; filename="{}""#, pdf.filnamn),
            pdf.innehåll,
        )
    }

    fn csv(csv: Fil) -> Response {
        fil(
            "text/csv; charset=utf-8",
            format!(r#"attachment; filename="{}""#, csv.filnamn),
            csv.innehåll,
        )
    }

    fn fil(typ: &'static str, disposition: String, innehåll: Vec<u8>) -> Response {
        (
            [
                (header::CONTENT_TYPE, typ.to_string()),
                (header::CONTENT_DISPOSITION, disposition),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            innehåll,
        )
            .into_response()
    }

    fn ingen_åtkomst() -> Response {
        (
            StatusCode::FORBIDDEN,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Du har inte åtkomst till företaget.\n",
        )
            .into_response()
    }

    fn serverfel(e: impl std::fmt::Display) -> Response {
        tracing::error!("bokföringen: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Något gick fel i Maya; försök igen.\n",
        )
            .into_response()
    }
}

/// Företaget i sökvägen, om användaren har åtkomst till det.
fn företaget() -> Option<FöretagVy> {
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    use_context::<Företagsval>()
        .unwrap_or_default()
        .0
        .into_iter()
        .find(|f| f.id == id)
}

/// Bokföringens sidor som ett avsnitt på Rapporter.
#[component]
pub fn BokföringPåRapporter(bas: String) -> impl IntoView {
    view! {
        <h2>"Bokföring"</h2>
        <p>"Grundboken och huvudboken ur de bokförda verifikaten, och kontoplanen."</p>
        <Bokföringsmeny bas=bas aktiv="" />
    }
}

/// Rapporter → Bokföring → Grundbok.
#[component]
pub fn GrundbokSida() -> impl IntoView {
    view! { <Bokföringssida huvudbok=false /> }
}

/// Rapporter → Bokföring → Huvudbok.
#[component]
pub fn HuvudbokSida() -> impl IntoView {
    view! { <Bokföringssida huvudbok=true /> }
}

#[component]
fn Bokföringssida(huvudbok: bool) -> impl IntoView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(företaget) = företaget() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let bas = företaget.href();
    let data = data();
    let (rubrik, aktiv, sökväg) = if huvudbok {
        ("Huvudbok", SIDOR[1].0, format!("{bas}{}", SIDOR[1].0))
    } else {
        ("Grundbok", SIDOR[0].0, format!("{bas}{}", SIDOR[0].0))
    };
    let (export, filnamn) = data.export.clone();
    let pdf = format!("{export}/pdf?{}", data.query);
    let csv = format!("{export}/csv?{}", data.query);
    let dokument = if huvudbok {
        view! { <Huvudboken data=data.clone() sida=sökväg.clone() /> }.into_any()
    } else {
        view! { <Grundboken data=data.clone() sida=sökväg.clone() /> }.into_any()
    };
    let panel = data
        .panel
        .clone()
        .map(|v| view! { <Panel v=v bas=bas.clone() stäng=format!("{sökväg}?{}", data.query) /> });
    view! {
        <section class="sida">
            <p class="diskret">{företaget.namn} " · " {företaget.orgnr}</p>
            <Bokföringsmeny bas=bas.clone() aktiv=aktiv />
            <h1>{rubrik}</h1>
            <Verktygsrad data=data.clone() sida=sökväg huvudbok=huvudbok />
            <div class="exporter">
                <Utskrift href=pdf filnamn=format!("{filnamn}.pdf") />
                <a class="exportera-csv" href=csv download=format!("{filnamn}.csv") rel="external">
                    "Exportera CSV"
                </a>
            </div>
            {dokument}
            {panel}
        </section>
    }
    .into_any()
}

/// Räkenskapsår, perioder och (i huvudboken) konton, som ett GET-formulär.
#[component]
fn Verktygsrad(data: Bokföringsdata, sida: String, huvudbok: bool) -> impl IntoView {
    let sista = data.perioder.len().saturating_sub(1);
    let perioder = move |valt: String, standard: usize| {
        let valt = if valt.is_empty() {
            data.perioder
                .get(standard)
                .map(|p| p.0.clone())
                .unwrap_or_default()
        } else {
            valt
        };
        data.perioder
            .iter()
            .map(|(m, namn)| {
                view! {
                    <option value=m.clone() selected=*m == valt>
                        {namn.clone()}
                    </option>
                }
            })
            .collect_view()
    };
    let konton = huvudbok.then(|| {
        let fel = data.konton_fel.then(|| {
            view! {
                <p class="fel" role="alert">
                    "Skriv kontonummer och intervall med komma emellan, t.ex. 1930, 3000–3999. Alla konton visas."
                </p>
            }
        });
        view! {
            <label>
                "Konton "
                <input
                    name="konton"
                    type="text"
                    value=data.konton.clone()
                    placeholder="Alla, eller t.ex. 1930, 3000–3999"
                />
            </label>
            {fel}
        }
    });
    view! {
        <form class="verktygsrad" method="get" action=sida>
            <label>
                "Räkenskapsår "
                <select name="ar">
                    {data
                        .år
                        .iter()
                        .map(|(start, slut)| {
                            view! {
                                <option value=start.clone() selected=*start == data.valt_år>
                                    {format!("{start} – {slut}")}
                                </option>
                            }
                        })
                        .collect_view()}
                </select>
            </label>
            <label>"Från " <select name="fran">{perioder(data.från.clone(), 0)}</select></label>
            <label>"T.o.m. " <select name="till">{perioder(data.till.clone(), sista)}</select></label>
            {konton}
            <button class="knapp" type="submit">
                "Visa"
            </button>
        </form>
    }
}

/// Länken som öppnar verifikatet i panelen och behåller urvalet.
fn panellänk(sida: &str, query: &str, id: &str, nummer: String) -> impl IntoView {
    view! {
        <a rel="external" href=format!("{sida}?{query}&v={id}")>
            {nummer}
        </a>
    }
}

#[component]
fn Grundboken(data: Bokföringsdata, sida: String) -> impl IntoView {
    let perioder = data
        .grundbok
        .iter()
        .map(|p| {
            let verifikat = if p.verifikat.is_empty() {
                view! { <p class="diskret">"Inga bokförda verifikat."</p> }.into_any()
            } else {
                let rader = p
                    .verifikat
                    .iter()
                    .map(|v| {
                        view! {
                            <tbody>
                                <tr class="verifikathuvud">
                                    <td>{panellänk(&sida, &data.query, &v.id, v.nummer.clone())}</td>
                                    <td colspan="2">
                                        {v.datum.clone()} " · " {v.beskrivning.clone()}
                                        <br />
                                        <span class="diskret">"Bokfört " {v.bokförd.clone()}</span>
                                    </td>
                                    <td></td>
                                    <td class="belopp">{v.summa.clone()}</td>
                                </tr>
                                {v
                                    .rader
                                    .iter()
                                    .map(|r| {
                                        view! {
                                            <tr>
                                                <td>{r.konto.clone()}</td>
                                                <td>{r.kontonamn.clone()}</td>
                                                <td>{r.text.clone()}</td>
                                                <td class="belopp">{r.debet.clone()}</td>
                                                <td class="belopp">{r.kredit.clone()}</td>
                                            </tr>
                                        }
                                    })
                                    .collect_view()}
                            </tbody>
                        }
                    })
                    .collect_view();
                view! {
                    <table class="rader">
                        <thead>
                            <tr>
                                <th>"Konto"</th>
                                <th>"Kontonamn"</th>
                                <th>"Text"</th>
                                <th class="belopp">"Debet"</th>
                                <th class="belopp">"Kredit"</th>
                            </tr>
                        </thead>
                        {rader}
                    </table>
                }
                .into_any()
            };
            view! {
                <section>
                    <h3>{p.namn.clone()}</h3>
                    {verifikat}
                </section>
            }
        })
        .collect_view();
    view! {
        <article class="dokument bokforingsdokument">
            <p class="diskret">"Bokförda verifikat per period i registreringsordning."</p>
            {perioder}
        </article>
    }
}

#[component]
fn Huvudboken(data: Bokföringsdata, sida: String) -> impl IntoView {
    let ingående = if data.preliminär {
        "Ingående balans (preliminär)"
    } else {
        "Ingående balans"
    };
    let banderoll = data.preliminär.then(|| {
        view! {
            <div class="notis" role="status">
                <span class="markering">"Preliminär"</span>
                " Den ingående balansen räknas fram ur föregående års utgående balans, eftersom det året inte är stängt."
            </div>
        }
    });
    let konton = if data.huvudbok.is_empty() {
        view! { <p class="diskret">"Inga konton med saldo eller transaktioner i urvalet."</p> }
            .into_any()
    } else {
        data.huvudbok
            .iter()
            .map(|k| {
                view! {
                    <section>
                        <h3>{k.rubrik.clone()}</h3>
                        <table class="rader">
                            <thead>
                                <tr>
                                    <th>"Datum"</th>
                                    <th>"Ver"</th>
                                    <th>"Text"</th>
                                    <th class="belopp">"Debet"</th>
                                    <th class="belopp">"Kredit"</th>
                                    <th class="belopp">"Saldo"</th>
                                </tr>
                            </thead>
                            <tbody>
                                <tr>
                                    <td></td>
                                    <td></td>
                                    <td>{ingående}</td>
                                    <td></td>
                                    <td></td>
                                    <td class="belopp">{k.ingående.clone()}</td>
                                </tr>
                                {k
                                    .transaktioner
                                    .iter()
                                    .map(|t| {
                                        view! {
                                            <tr>
                                                <td>{t.datum.clone()}</td>
                                                <td>{panellänk(&sida, &data.query, &t.id, t.nummer.clone())}</td>
                                                <td>{t.text.clone()}</td>
                                                <td class="belopp">{t.debet.clone()}</td>
                                                <td class="belopp">{t.kredit.clone()}</td>
                                                <td class="belopp">{t.saldo.clone()}</td>
                                            </tr>
                                        }
                                    })
                                    .collect_view()}
                            </tbody>
                            <tfoot>
                                <tr>
                                    <td colspan="5">"Utgående balans"</td>
                                    <td class="belopp">{k.utgående.clone()}</td>
                                </tr>
                            </tfoot>
                        </table>
                    </section>
                }
            })
            .collect_view()
            .into_any()
    };
    view! {
        {banderoll}
        <article class="dokument bokforingsdokument">{konton}</article>
    }
}

/// Verifikatet i en panel bredvid dokumentet, med länk till det i fliken Verifikat.
#[component]
fn Panel(v: PostVy, bas: String, stäng: String) -> impl IntoView {
    view! {
        <aside class="sidopanel" aria-label="Verifikat">
            <div class="panelhuvud">
                <h2>{v.nummer.clone()} " · " {v.beskrivning.clone()}</h2>
                <a rel="external" class="knapp knapp-diskret" href=stäng aria-label="Stäng">
                    "Stäng"
                </a>
            </div>
            <dl class="fakta">
                <dt>"Verifikatdatum"</dt>
                <dd>{v.datum.clone()}</dd>
                <dt>"Bokfört"</dt>
                <dd>{v.bokförd.clone()}</dd>
                <dt>"Summa"</dt>
                <dd>{v.summa.clone()} " kr"</dd>
            </dl>
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Konto"</th>
                        <th>"Text"</th>
                        <th class="belopp">"Debet"</th>
                        <th class="belopp">"Kredit"</th>
                    </tr>
                </thead>
                <tbody>
                    {v
                        .rader
                        .into_iter()
                        .map(|r| {
                            view! {
                                <tr>
                                    <td>{r.konto} " " <span class="diskret">{r.kontonamn}</span></td>
                                    <td>{r.text}</td>
                                    <td class="belopp">{r.debet}</td>
                                    <td class="belopp">{r.kredit}</td>
                                </tr>
                            }
                        })
                        .collect_view()}
                </tbody>
            </table>
            <p>
                <a
                    rel="external"
                    class="till-verifikatet"
                    href=format!("{bas}/verifikat?v={}", v.id)
                >
                    "Öppna verifikatet"
                </a>
            </p>
        </aside>
    }
}
