//! Inställningar → Företag: fakturauppgifterna (betalsätt, OCR, svarsadress och
//! standardmeddelande) och logotypen till fakturamallen.
//!
//! Formuläret prövas med `maya_domain::fakturauppgifter` i webbläsaren och av `core` på
//! servern, med samma koder. Logotypens format prövas av servern efter filens innehåll.

use leptos::prelude::*;
use maya_domain::fakturauppgifter::{Fakturauppgifter, Fält, Fältfel, logotypfel};
use serde::{Deserialize, Serialize};

use crate::installningar::fråga;

/// Fakturauppgifterna som sidan visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakturauppgifterVy {
    pub bankgiro: String,
    pub plusgiro: String,
    pub iban: String,
    pub bic: String,
    pub ocr: bool,
    pub svarsadress: String,
    pub standardmeddelande: String,
    pub logotyp: Option<LogotypVy>,
}

/// Logotypen som sidan visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogotypVy {
    pub filnamn: String,
    /// Början av filens SHA-256, så att webbläsaren hämtar en ny logotyp efter ett byte.
    pub version: String,
    /// Storleken som den visas, t.ex. "12 kB".
    pub storlek: String,
}

impl From<&FakturauppgifterVy> for Fakturauppgifter {
    fn from(f: &FakturauppgifterVy) -> Self {
        Fakturauppgifter {
            bankgiro: f.bankgiro.clone(),
            plusgiro: f.plusgiro.clone(),
            iban: f.iban.clone(),
            bic: f.bic.clone(),
            ocr: f.ocr,
            svarsadress: f.svarsadress.clone(),
            standardmeddelande: f.standardmeddelande.clone(),
        }
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{DefaultBodyLimit, Path, Request, State};
    use axum::http::{HeaderValue, StatusCode, header};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::{get, post};
    use axum::{Extension, Form, Router};
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::fakturauppgifter::{self, FakturauppgiftsFel};
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::{Databas, Klocka};
    use maya_domain::fakturauppgifter::{Fakturauppgifter, STÖRSTA_LOGOTYP};

    use super::{FakturauppgifterVy, LogotypVy};
    use crate::svar::{ingen_åtkomst, serverfel, text};
    use crate::underlag::server::läs_formulär;

    #[derive(Clone)]
    pub(crate) struct FakturauppgiftsState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: FakturauppgiftsState) -> Router {
        Router::new()
            .route(
                "/foretag/{id}/installningar/fakturauppgifter",
                post(uppgifter),
            )
            .route(
                "/foretag/{id}/installningar/logotyp",
                get(logotypen).post(byt_logotyp),
            )
            .route(
                "/foretag/{id}/installningar/logotyp/ta-bort",
                post(ta_bort_logotyp),
            )
            .layer(DefaultBodyLimit::max(STÖRSTA_LOGOTYP + 64 * 1024))
            .with_state(Arc::new(state))
    }

    /// Fakturauppgifterna och logotypen som sidan Företag visar.
    pub(crate) async fn vy(
        db: &Databas,
        inloggad: &Inloggad,
        id: FöretagsId,
    ) -> Result<FakturauppgifterVy, ÅtkomstFel> {
        let f = fakturauppgifter::fakturauppgifterna(db, &inloggad.aktör, id).await?;
        let u = f.uppgifter;
        Ok(FakturauppgifterVy {
            bankgiro: u.bankgiro,
            plusgiro: u.plusgiro,
            iban: u.iban,
            bic: u.bic,
            ocr: u.ocr,
            svarsadress: u.svarsadress,
            standardmeddelande: u.standardmeddelande,
            logotyp: f.logotyp.map(|l| LogotypVy {
                filnamn: l.filnamn,
                version: l.sha256.chars().take(12).collect(),
                storlek: maya_domain::underlag::storlek(l.storlek),
            }),
        })
    }

    type Fält = Vec<(String, String)>;

    fn värde(fält: &Fält, namn: &str) -> String {
        fält
            .iter()
            .find(|(k, _)| k == namn)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    fn sidan(företag: FöretagsId, query: &str) -> Response {
        Redirect::to(&format!("/foretag/{företag}/installningar?{query}")).into_response()
    }

    /// Svaret på ett användningsfall: sidan med `sparat=<form>` när det gick, och annars
    /// tillbaka med `tillbaka`.
    fn svara(
        företag: FöretagsId,
        form: &str,
        svar: Result<(), FakturauppgiftsFel>,
        tillbaka: impl FnOnce(&[maya_domain::fakturauppgifter::Fältfel]) -> Response,
    ) -> Response {
        match svar {
            Ok(()) => sidan(företag, &format!("sparat={form}")),
            Err(FakturauppgiftsFel::Ogiltigt(fel)) => tillbaka(&fel),
            Err(FakturauppgiftsFel::Åtkomst(
                ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte,
            )) => ingen_åtkomst(),
            Err(e) => serverfel(e),
        }
    }

    fn företaget(
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

    async fn uppgifter(
        State(state): State<Arc<FakturauppgiftsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag) = match företaget(&id, inloggad) {
            Ok(v) => v,
            Err(r) => return *r,
        };
        let nya = Fakturauppgifter {
            bankgiro: värde(&fält, "bankgiro"),
            plusgiro: värde(&fält, "plusgiro"),
            iban: värde(&fält, "iban"),
            bic: värde(&fält, "bic"),
            ocr: fält.iter().any(|(k, _)| k == "ocr"),
            svarsadress: värde(&fält, "svarsadress"),
            standardmeddelande: värde(&fält, "standardmeddelande"),
        };
        let svar =
            fakturauppgifter::ändra(&state.db, &state.klocka, &inloggad.aktör, företag, &nya).await;
        svara(företag, "fakturauppgifter", svar, |_| {
            // Tillbaka med det användaren skrev; sidan prövar det igen med samma regler.
            let mut query = url::form_urlencoded::Serializer::new(String::new());
            query.append_pair("form", "fakturauppgifter");
            for (k, v) in &fält {
                if !matches!(k.as_str(), "form" | "fel" | "sparat") {
                    query.append_pair(k, v);
                }
            }
            sidan(företag, &query.finish())
        })
    }

    fn logotypfel(företag: FöretagsId, kod: &str) -> Response {
        sidan(företag, &format!("form=logotyp&fel={kod}"))
    }

    async fn byt_logotyp(
        State(state): State<Arc<FakturauppgiftsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        req: Request,
    ) -> Response {
        let (inloggad, företag) = match företaget(&id, inloggad) {
            Ok(v) => v,
            Err(r) => return *r,
        };
        let formulär = match läs_formulär(req).await {
            Ok(f) => f,
            Err(r) => return *r,
        };
        if formulär.för_stort {
            return logotypfel(företag, "logotyp_for_stor");
        }
        let Some(fil) = formulär.fil else {
            return logotypfel(företag, "logotyp_saknas");
        };
        let svar = fakturauppgifter::byt_logotyp(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            &fil.filnamn,
            &fil.innehåll,
        )
        .await;
        svara(företag, "logotyp", svar, |fel| {
            logotypfel(företag, fel.first().map_or("logotyp_format", |f| f.kod))
        })
    }

    async fn ta_bort_logotyp(
        State(state): State<Arc<FakturauppgiftsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let (inloggad, företag) = match företaget(&id, inloggad) {
            Ok(v) => v,
            Err(r) => return *r,
        };
        let svar =
            fakturauppgifter::ta_bort_logotyp(&state.db, &state.klocka, &inloggad.aktör, företag)
                .await;
        svara(företag, "logotyp", svar, |_| {
            serverfel("logotypen togs inte bort")
        })
    }

    /// Logotypen som bild. En SVG kan innehålla skript, så svaret får en sandlåda och inga
    /// andra resurser; sidan visar den bara i en `img`.
    async fn logotypen(
        State(state): State<Arc<FakturauppgiftsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return (StatusCode::UNAUTHORIZED, "Logga in först.\n").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let fil = match fakturauppgifter::logotypen(&state.db, &inloggad.aktör, företag).await {
            Ok(Some(fil)) => fil,
            Ok(None) => return text(StatusCode::NOT_FOUND, "Företaget har ingen logotyp.\n"),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                return ingen_åtkomst();
            }
            Err(e) => return serverfel(e),
        };
        let mut response = fil.innehåll.into_response();
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&fil.mime)
                .unwrap_or(HeaderValue::from_static("application/octet-stream")),
        );
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; sandbox"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-cache"),
        );
        response
    }
}

/// Felen vid ett fält, när de ska visas.
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

/// Betalsätt, OCR, svarsadress och standardmeddelande.
#[component]
pub fn FakturaFormulär(bas: String, data: FakturauppgifterVy) -> impl IntoView {
    let (form, q) = fråga();
    let tillbaka = form.as_deref() == Some("fakturauppgifter");
    let start = if tillbaka {
        let v = |n: Fält| q(n.namn()).unwrap_or_default();
        Fakturauppgifter {
            bankgiro: v(Fält::Bankgiro),
            plusgiro: v(Fält::Plusgiro),
            iban: v(Fält::Iban),
            bic: v(Fält::Bic),
            ocr: q(Fält::Ocr.namn()).is_some(),
            svarsadress: v(Fält::Svarsadress),
            standardmeddelande: v(Fält::Standardmeddelande),
        }
    } else {
        Fakturauppgifter::from(&data)
    };
    let uppgifter = RwSignal::new(start.clone());
    let visa = RwSignal::new(tillbaka);
    let fel = Memo::new(move |_| uppgifter.with(|u| u.kontrollera().err().unwrap_or_default()));
    let skicka = move |ev: leptos::ev::SubmitEvent| {
        if !fel.with_untracked(Vec::is_empty) {
            ev.prevent_default();
            visa.set(true);
        }
    };
    let fält = move |fält: Fält,
                     etikett: &'static str,
                     värde: String,
                     platshållare: &'static str,
                     sätt: fn(&mut Fakturauppgifter, String)| {
        let namn = fält.namn();
        view! {
            <label for=namn>{etikett}</label>
            <input
                id=namn
                name=namn
                type="text"
                value=värde
                placeholder=platshållare
                on:input=move |ev| uppgifter.update(|u| sätt(u, event_target_value(&ev)))
            />
            {fel_vid(fel, visa, fält)}
        }
    };
    view! {
        <form
            class="formular"
            method="post"
            action=format!("{bas}/installningar/fakturauppgifter")
            novalidate
            on:submit=skicka
        >
            <h2>"Fakturor"</h2>
            <p class="diskret">
                "Minst ett betalsätt krävs för att utfärda en faktura: bankgiro, plusgiro eller IBAN med BIC."
            </p>
            {fält(Fält::Bankgiro, "Bankgiro", start.bankgiro.clone(), "5050-1055", |u, v| {
                u.bankgiro = v
            })}
            {fält(Fält::Plusgiro, "Plusgiro", start.plusgiro.clone(), "12345-5", |u, v| {
                u.plusgiro = v
            })}
            {fält(Fält::Iban, "IBAN", start.iban.clone(), "SE45 5000 0000 0583 9825 7466", |u, v| {
                u.iban = v
            })}
            {fält(Fält::Bic, "BIC", start.bic.clone(), "ESSESESS", |u, v| u.bic = v)}
            <label class="kryss">
                <input
                    name="ocr"
                    type="checkbox"
                    value="ja"
                    checked=start.ocr
                    on:change=move |ev| uppgifter.update(|u| u.ocr = event_target_checked(&ev))
                />
                "OCR-referens på fakturorna"
            </label>
            <p class="diskret">
                "Med OCR bildas referensen av fakturanumret, en längdsiffra och en kontrollsiffra. Utan OCR är fakturanumret referensen."
            </p>
            {fält(
                Fält::Svarsadress,
                "Svarsadress för fakturamejl",
                start.svarsadress.clone(),
                "faktura@exempel.se",
                |u, v| u.svarsadress = v,
            )}
            <label for="standardmeddelande">"Standardmeddelande för fakturamejl"</label>
            <textarea
                id="standardmeddelande"
                name="standardmeddelande"
                rows="4"
                on:input=move |ev| {
                    uppgifter.update(|u| u.standardmeddelande = event_target_value(&ev))
                }
            >
                {start.standardmeddelande.clone()}
            </textarea>
            {fel_vid(fel, visa, Fält::Standardmeddelande)}
            <button class="knapp" type="submit">
                "Spara fakturauppgifterna"
            </button>
        </form>
    }
}

/// Logotypen till fakturamallen: visa, byt och ta bort.
#[component]
pub fn LogotypFormulär(bas: String, data: FakturauppgifterVy) -> impl IntoView {
    let (form, q) = fråga();
    let fel = (form.as_deref() == Some("logotyp"))
        .then(|| q("fel"))
        .flatten()
        .and_then(|kod| logotypfel(&kod))
        .map(|f| {
            view! {
                <p class="fel" role="alert" data-kod=f.kod>
                    {f.text}
                </p>
            }
        });
    let nuvarande = match data.logotyp {
        Some(l) => view! {
            <img
                class="logotyp-forhandsvisning"
                src=format!("{bas}/installningar/logotyp?v={}", l.version)
                alt="Logotypen"
            />
            <p class="diskret">{l.filnamn} " · " {l.storlek}</p>
            <form
                class="formular"
                method="post"
                action=format!("{bas}/installningar/logotyp/ta-bort")
            >
                <button class="knapp-diskret" type="submit">
                    "Ta bort logotypen"
                </button>
            </form>
        }
        .into_any(),
        None => view! { <p class="diskret">"Ingen logotyp."</p> }.into_any(),
    };
    view! {
        <section class="logotyp">
            <h2>"Logotyp"</h2>
            <p class="diskret">"Logotypen står på fakturorna. PNG, JPG eller SVG, högst 1 MB."</p>
            {nuvarande}
            <form
                class="formular"
                method="post"
                action=format!("{bas}/installningar/logotyp")
                enctype="multipart/form-data"
            >
                <label for="logotyp">"Ny logotyp"</label>
                <input
                    id="logotyp"
                    name="fil"
                    type="file"
                    accept="image/png,image/jpeg,image/svg+xml"
                />
                {fel}
                <button class="knapp" type="submit">
                    "Ladda upp logotypen"
                </button>
            </form>
        </section>
    }
}
