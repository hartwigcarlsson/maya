//! Sökningen: ⌘K och sökfältet i toppmenyn öppnar samma sökning efter sidor, handlingar,
//! företagsbyte och, i det valda företaget, verifikat, kund- och leverantörsfakturor, kunder och
//! leverantörer. Sidorna och handlingarna prövas med `maya_domain::sok` där sökningen visas,
//! resten av `core` på servern.
//!
//! Utan JavaScript skickar sökfältet frågan till `/foretag/<id>/sok`, som visar samma sökning
//! som paletten.

use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use maya_domain::sok::Fråga;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{FöretagVy, Företagsval, IngenÅtkomst};
use crate::sidodata::sida;

/// Något som sökningen leder till.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Träff {
    /// Rubriken som träffen visas under, t.ex. "Gå till" eller "Verifikat".
    pub grupp: String,
    pub titel: String,
    /// Datum och belopp för ett verifikat eller en faktura, organisationsnumret för ett
    /// företag.
    pub detalj: String,
    pub href: String,
    /// Ett märke vid titeln, t.ex. "Arkiverad" för en arkiverad kund; tomt utan märke.
    #[serde(default)]
    pub märke: String,
}

/// Handlingarna, sidorna och företagsbytet som frågan träffar, när `valt` är det valda
/// företaget. En tom fråga träffar alla.
pub fn mål(fråga: &str, valt: &FöretagVy, alla: &[FöretagVy]) -> Vec<Träff> {
    let bas = valt.href();
    // Grupp, titel, fler ord som träffar och vart den leder.
    let mut kandidater: Vec<(&str, String, &str, String, String)> = vec![
        (
            "Handlingar",
            "Nytt verifikat".into(),
            "bokför registrera",
            String::new(),
            format!("{bas}/verifikat/nytt"),
        ),
        (
            "Handlingar",
            "Ladda upp underlag".into(),
            "kvitto faktura fil okopplade",
            String::new(),
            format!("{bas}/underlag"),
        ),
        (
            "Handlingar",
            "Lägg upp företag".into(),
            "nytt",
            String::new(),
            "/foretag/nytt".into(),
        ),
        // #154
        (
            "Handlingar",
            "Ny kundfaktura".into(),
            "fakturera utfärda",
            String::new(),
            format!("{bas}/kundfakturor/ny"),
        ),
        (
            "Handlingar",
            "Registrera extern kundfaktura".into(),
            "ny faktura annat system",
            String::new(),
            format!("{bas}/kundfakturor/extern/ny"),
        ),
        (
            "Handlingar",
            "Registrera leverantörsfaktura".into(),
            "ny faktura att betala",
            String::new(),
            format!("{bas}/leverantorsfakturor/ny"),
        ),
        (
            "Handlingar",
            "Ny kund".into(),
            "kundregister lägg upp",
            String::new(),
            format!("{bas}/kunder/ny"),
        ),
        (
            "Handlingar",
            "Ny leverantör".into(),
            "leverantörsregister lägg upp",
            String::new(),
            format!("{bas}/leverantorer/ny"),
        ),
        (
            "Gå till",
            "Att göra".into(),
            "start",
            String::new(),
            bas.clone(),
        ),
        (
            "Gå till",
            "Verifikat".into(),
            "grundbok",
            String::new(),
            format!("{bas}/verifikat"),
        ),
        (
            "Gå till",
            "Utkast".into(),
            "verifikat",
            String::new(),
            format!("{bas}/verifikat?urval=utkast"),
        ),
        // #154
        (
            "Gå till",
            "Kundfakturor".into(),
            "faktura kundreskontra fordringar",
            String::new(),
            format!("{bas}/kundfakturor"),
        ),
        (
            "Gå till",
            "Leverantörsfakturor".into(),
            "faktura leverantörsreskontra skulder",
            String::new(),
            format!("{bas}/leverantorsfakturor"),
        ),
        (
            "Gå till",
            "Kunder".into(),
            "kundregister",
            String::new(),
            format!("{bas}/kunder"),
        ),
        (
            "Gå till",
            "Leverantörer".into(),
            "leverantörsregister",
            String::new(),
            format!("{bas}/leverantorer"),
        ),
        (
            "Gå till",
            "Rapporter".into(),
            "balansräkning resultaträkning sie4 export",
            String::new(),
            format!("{bas}/rapporter"),
        ),
        (
            "Gå till",
            "Grundbok".into(),
            "rapporter bokföring registreringsordning",
            String::new(),
            format!("{bas}/rapporter/grundbok"),
        ),
        (
            "Gå till",
            "Huvudbok".into(),
            "rapporter bokföring konton saldo",
            String::new(),
            format!("{bas}/rapporter/huvudbok"),
        ),
        (
            "Gå till",
            "Inställningar".into(),
            "företaget",
            String::new(),
            format!("{bas}/installningar"),
        ),
        (
            "Gå till",
            "Kontoplan".into(),
            "inställningar konton bas",
            String::new(),
            format!("{bas}/installningar/kontoplan"),
        ),
        (
            "Gå till",
            "Åtkomst".into(),
            "inställningar användare bjud in inbjudan dela",
            String::new(),
            format!("{bas}/installningar/atkomst"),
        ),
        (
            "Gå till",
            "Behandlingshistorik".into(),
            "inställningar logg ändringar revisor",
            String::new(),
            format!("{bas}/installningar/behandlingshistorik"),
        ),
        (
            "Gå till",
            "Systemdokumentation".into(),
            "inställningar revisor regler arkivplan kontoplan behandlingsregler",
            String::new(),
            format!("{bas}/installningar/systemdokumentation"),
        ),
    ];
    kandidater.extend(
        alla.iter()
            .filter(|f| f.id != valt.id)
            .map(|f| ("Byt företag", f.namn.clone(), "", f.orgnr.clone(), f.href())),
    );
    let fråga = Fråga::tolka(fråga);
    kandidater
        .into_iter()
        .filter(|(grupp, titel, ord, detalj, _)| {
            fråga
                .as_ref()
                .is_none_or(|f| f.träffar_text(&format!("{grupp} {titel} {ord} {detalj}")))
        })
        .map(|(grupp, titel, _, detalj, href)| Träff {
            grupp: grupp.into(),
            titel,
            detalj,
            href,
            märke: String::new(),
        })
        .collect()
}

/// Verifikaten, fakturorna, kunderna och leverantörerna som träffar frågan i företaget
/// `foretag`, för paletten.
#[server(prefix = "/api", endpoint = "sok", input = server_fn::codec::GetUrl)]
pub async fn sök_i_företaget(foretag: String, q: String) -> Result<Vec<Träff>, ServerFnError> {
    use maya_core::Databas;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;

    let parts = use_context::<axum::http::request::Parts>()
        .ok_or_else(|| ServerFnError::new("begäran saknas"))?;
    let inloggad = parts
        .extensions
        .get::<Inloggad>()
        .ok_or_else(|| ServerFnError::new("inte inloggad"))?;
    let db = use_context::<Databas>().ok_or_else(|| ServerFnError::new("databasen saknas"))?;
    let företag: FöretagsId = foretag
        .parse()
        .map_err(|_| ServerFnError::new("okänt företag"))?;
    server::träffar(&db, &inloggad.aktör, företag, &q)
        .await
        .map_err(|e| match e {
            maya_core::atkomst::ÅtkomstFel::IngenÅtkomst
            | maya_core::atkomst::ÅtkomstFel::KanalenFårInte => {
                ServerFnError::new("du har inte åtkomst till företaget")
            }
            e => {
                tracing::error!("sökningen: {e}");
                ServerFnError::new("något gick fel i Maya; försök igen")
            }
        })
}

/// Det i företaget som sidan `/foretag/<id>/sok` visar: verifikaten, fakturorna, kunderna och
/// leverantörerna.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sökdata {
    pub träffar: Vec<Träff>,
}

/// Datan för `/foretag/<id>/sok?q=`.
#[server(prefix = "/api", endpoint = "sokdata", input = server_fn::codec::GetUrl)]
pub async fn hämta_sökdata(adress: String) -> Result<Sökdata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::sökdata(&begäran).await,
        None => Ok(Sökdata::default()),
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use leptos::prelude::ServerFnError;
    use maya_core::Databas;
    use maya_core::aktor::Aktör;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::kund::Kundstatus;
    use maya_core::kundfaktura::{self, Kundfakturarad};
    use maya_core::leverantor;
    use maya_core::leverantorsfaktura::{self, Fakturarad};
    use maya_core::sok;
    use maya_domain::belopp::Saldo;

    use super::{Sökdata, Träff};
    use crate::sidodata::{self, Sidbegäran};

    /// Så många träffar av varje slag visar sökningen.
    const HÖGST: usize = 20;

    /// Märket på en arkiverad kund eller leverantör.
    const ARKIVERAD: &str = "Arkiverad";

    /// Träffarna i företaget grupperade per slag: verifikat, kundfakturor, leverantörsfakturor,
    /// kunder och leverantörer. Varje träff leder till sin flik.
    pub(crate) async fn träffar(
        db: &Databas,
        aktör: &Aktör,
        företag: FöretagsId,
        fråga: &str,
    ) -> Result<Vec<Träff>, ÅtkomstFel> {
        let t = sok::sök(db, aktör, företag, fråga).await?;
        let bas = format!("/foretag/{företag}");
        let träff = |grupp: &str, titel: String, detalj: String, href: String| Träff {
            grupp: grupp.into(),
            titel,
            detalj,
            href,
            märke: String::new(),
        };
        let mut träffar: Vec<Träff> = Vec::new();
        träffar.extend(t.verifikat.into_iter().take(HÖGST).map(|v| {
            träff(
                "Verifikat",
                format!(
                    "{} · {}",
                    v.nummer
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "Utkast".into()),
                    v.beskrivning
                ),
                match v.datum {
                    Some(d) => format!("{d} · {}", v.summa),
                    None => v.summa.to_string(),
                },
                format!("{bas}/verifikat?v={}", v.id),
            )
        }));
        träffar.extend(t.kundfakturor.iter().take(HÖGST).map(|f| {
            träff(
                "Kundfakturor",
                kundfakturans_titel(f),
                fakturans_detalj(f.fakturadatum, f.total),
                format!("{bas}/kundfakturor?f={}", f.id),
            )
        }));
        träffar.extend(t.leverantörsfakturor.iter().take(HÖGST).map(|f| {
            träff(
                "Leverantörsfakturor",
                leverantörsfakturans_titel(f),
                fakturans_detalj(f.fakturadatum, f.total.unwrap_or_default()),
                format!("{bas}/leverantorsfakturor?f={}", f.id),
            )
        }));
        träffar.extend(t.kunder.iter().take(HÖGST).map(|k| {
            let arkiverad = k.status == Kundstatus::Arkiverad;
            Träff {
                märke: if arkiverad {
                    ARKIVERAD.into()
                } else {
                    String::new()
                },
                ..träff(
                    "Kunder",
                    k.namn.clone(),
                    k.identitet.clone(),
                    // Registret visar de arkiverade med `visa=alla`.
                    format!(
                        "{bas}/kunder?k={}{}",
                        k.id,
                        if arkiverad { "&visa=alla" } else { "" }
                    ),
                )
            }
        }));
        träffar.extend(t.leverantörer.iter().take(HÖGST).map(|l| {
            let arkiverad = l.status == leverantor::Status::Arkiverad;
            Träff {
                märke: if arkiverad {
                    ARKIVERAD.into()
                } else {
                    String::new()
                },
                ..träff(
                    "Leverantörer",
                    l.uppgifter.namn.clone(),
                    l.uppgifter.orgnr.clone(),
                    format!(
                        "{bas}/leverantorer?l={}{}",
                        l.id,
                        if arkiverad { "&visa=arkiverade" } else { "" }
                    ),
                )
            }
        }));
        Ok(träffar)
    }

    /// "Faktura 12 · Kund AB", "Kreditfaktura 13 · Kund AB" eller "Utkast · Kund AB".
    fn kundfakturans_titel(f: &Kundfakturarad) -> String {
        let vad = match (f.status, f.krediterar.is_some()) {
            (kundfaktura::Status::Utkast, _) if f.fakturanummer.is_empty() => "Utkast".into(),
            (kundfaktura::Status::Utkast, _) => format!("Utkast {}", f.fakturanummer),
            (_, true) => format!("Kreditfaktura {}", f.fakturanummer),
            (_, false) => format!("Faktura {}", f.fakturanummer),
        };
        med_motpart(vad, &f.kund)
    }

    fn leverantörsfakturans_titel(f: &Fakturarad) -> String {
        let vad = match (f.status, f.kreditfaktura) {
            (leverantorsfaktura::Status::Utkast, _) if f.fakturanummer.is_empty() => {
                "Utkast".into()
            }
            (leverantorsfaktura::Status::Utkast, _) => format!("Utkast {}", f.fakturanummer),
            (_, true) => format!("Kreditfaktura {}", f.fakturanummer),
            (_, false) => format!("Faktura {}", f.fakturanummer),
        };
        med_motpart(vad, &f.leverantörsnamn)
    }

    fn med_motpart(vad: String, motpart: &str) -> String {
        if motpart.is_empty() {
            vad
        } else {
            format!("{vad} · {motpart}")
        }
    }

    /// Fakturadatumet och totalen, negativ för en kreditfaktura.
    fn fakturans_detalj(fakturadatum: Option<jiff::civil::Date>, total: i64) -> String {
        let total = Saldo::från_öre(total);
        match fakturadatum {
            Some(d) => format!("{d} · {total}"),
            None => total.to_string(),
        }
    }

    /// Hämtar träffarna som sidan `/foretag/<id>/sok?q=` visar. Utan åtkomst blir datan tom;
    /// sidan visar det ur företagsvalet.
    pub(crate) async fn sökdata(b: &Sidbegäran) -> Result<Sökdata, ServerFnError> {
        let delar = b.delar();
        let ["foretag", id, "sok"] = delar.as_slice() else {
            return Ok(Sökdata::default());
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Ok(Sökdata::default());
        };
        let fråga = b.värde("q").unwrap_or_default();
        match träffar(&b.db, &b.inloggad.aktör, företag, &fråga).await {
            Ok(träffar) => Ok(Sökdata { träffar }),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                Ok(Sökdata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }
}

/// Sökfältet i toppmenyn. Med JavaScript öppnar det paletten, liksom ⌘K (Ctrl+K) var som helst
/// i företaget; utan skickar det frågan till sidan `/foretag/<id>/sok`.
#[component]
pub fn Sökfält(valt: FöretagVy, alla: Vec<FöretagVy>) -> impl IntoView {
    let öppen = RwSignal::new(false);
    #[cfg(feature = "hydrate")]
    {
        let lyssnare = window_event_listener(leptos::ev::keydown, move |ev| {
            if (ev.meta_key() || ev.ctrl_key()) && ev.key().eq_ignore_ascii_case("k") {
                ev.prevent_default();
                öppen.update(|ö| *ö = !*ö);
            }
        });
        on_cleanup(move || lyssnare.remove());
    }
    let action = format!("{}/sok", valt.href());
    let palett = move || {
        öppen.get().then(|| {
            let stäng = Callback::new(move |()| öppen.set(false));
            view! {
                <div
                    class="palett-bakgrund"
                    on:click=move |ev| {
                        if ev.target() == ev.current_target() {
                            öppen.set(false);
                        }
                    }
                >
                    <div class="palett" role="dialog" aria-modal="true" aria-label="Sök">
                        <Sökning
                            valt=valt.clone()
                            alla=alla.clone()
                            fråga=String::new()
                            träffar=Vec::new()
                            stäng=stäng
                        />
                    </div>
                </div>
            }
        })
    };
    view! {
        <form
            class="sokfalt"
            action=action
            role="search"
            on:submit=move |ev| {
                ev.prevent_default();
                öppen.set(true);
            }
        >
            <input
                name="q"
                type="search"
                placeholder="Sök eller gå till…"
                aria-label="Sök"
                autocomplete="off"
                on:focus=move |_| öppen.set(true)
            />
            <kbd>"⌘K"</kbd>
        </form>
        {palett}
    }
}

/// Sökningen: fältet och träffarna. Paletten och sidan `/foretag/<id>/sok` visar samma.
#[component]
fn Sökning(
    valt: FöretagVy,
    alla: Vec<FöretagVy>,
    fråga: String,
    /// Träffarna från servern: verifikat, fakturor, kunder och leverantörer.
    träffar: Vec<Träff>,
    #[prop(optional)] stäng: Option<Callback<()>>,
) -> impl IntoView {
    let action = format!("{}/sok", valt.href());
    let företag = valt.id.clone();
    let start = fråga.clone();
    let fråga = RwSignal::new(fråga);
    let från_servern = RwSignal::new(träffar);
    let vald = RwSignal::new(0usize);
    let träffar = Memo::new(move |_| {
        let mut t = mål(&fråga.get(), &valt, &alla);
        t.extend(från_servern.get());
        t
    });
    let fält = NodeRef::<leptos::html::Input>::new();
    if stäng.is_some() {
        Effect::new(move |_| {
            if let Some(f) = fält.get() {
                let _ = f.focus();
            }
        });
    }
    let navigate = use_navigate();
    let skriv = move |ev: leptos::ev::Event| {
        let ny = event_target_value(&ev);
        fråga.set(ny.clone());
        vald.set(0);
        let företag = företag.clone();
        leptos::task::spawn_local(async move {
            let nya = if ny.trim().is_empty() {
                Ok(Vec::new())
            } else {
                sök_i_företaget(företag, ny.clone()).await
            };
            if let Ok(nya) = nya
                && fråga.get_untracked() == ny
            {
                från_servern.set(nya);
            }
        });
    };
    let tangent = move |ev: leptos::ev::KeyboardEvent| {
        let antal = träffar.with_untracked(Vec::len);
        match ev.key().as_str() {
            "ArrowDown" => {
                ev.prevent_default();
                vald.update(|v| *v = (*v + 1).min(antal.saturating_sub(1)));
            }
            "ArrowUp" => {
                ev.prevent_default();
                vald.update(|v| *v = v.saturating_sub(1));
            }
            "Enter" => {
                if let Some(t) = träffar.with_untracked(|t| t.get(vald.get_untracked()).cloned()) {
                    ev.prevent_default();
                    if let Some(stäng) = stäng {
                        stäng.run(());
                    }
                    navigate(&t.href, Default::default());
                }
            }
            "Escape" => {
                if let Some(stäng) = stäng {
                    ev.prevent_default();
                    stäng.run(());
                }
            }
            _ => {}
        }
    };
    let lista = move || {
        let träffar = träffar.get();
        if träffar.is_empty() {
            return view! { <p class="sok-tom">"Inga träffar."</p> }.into_any();
        }
        let mut grupp = String::new();
        let rader = träffar
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let rubrik = (t.grupp != grupp).then(|| {
                    grupp = t.grupp.clone();
                    view! { <li class="sok-grupp">{t.grupp.clone()}</li> }
                });
                let märke = (!t.märke.is_empty())
                    .then(|| view! { <span class="badge badge-okopplat">{t.märke.clone()}</span> });
                let detalj = (!t.detalj.is_empty())
                    .then(|| view! { <span class="diskret">{t.detalj.clone()}</span> });
                view! {
                    {rubrik}
                    <li>
                        <a
                            href=t.href
                            class:vald=move || vald.get() == i
                            on:click=move |_| {
                                if let Some(stäng) = stäng {
                                    stäng.run(());
                                }
                            }
                        >
                            <span>{t.titel}</span>
                            {märke}
                            {detalj}
                        </a>
                    </li>
                }
            })
            .collect_view();
        view! { <ul class="sok-traffar">{rader}</ul> }.into_any()
    };
    view! {
        <div class="sokning">
            <form action=action role="search">
                <input
                    name="q"
                    type="search"
                    placeholder="Sök verifikat, faktura, kund, leverantör, belopp, sida…"
                    aria-label="Sök"
                    autocomplete="off"
                    value=start
                    prop:value=fråga
                    node_ref=fält
                    on:input=skriv
                    on:keydown=tangent
                />
            </form>
            {lista}
            <p class="sok-hjalp diskret">
                <kbd>"↑"</kbd> <kbd>"↓"</kbd> " välj · " <kbd>"↵"</kbd> " öppna · " <kbd>"Esc"</kbd>
                " stäng"
            </p>
        </div>
    }
}

/// Sidan `/foretag/<id>/sok`, dit sökfältet skickar frågan utan JavaScript.
#[component]
pub fn SökSida() -> impl IntoView {
    sida(hämta_sökdata, söksida)
}

fn söksida(data: Sökdata) -> AnyView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let alla = use_context::<Företagsval>().unwrap_or_default().0;
    let Some(valt) = alla.iter().find(|f| f.id == id).cloned() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let fråga = use_query_map().with_untracked(|q| q.get("q").unwrap_or_default());
    view! {
        <section class="sida">
            <h1>"Sök"</h1>
            <Sökning valt=valt alla=alla fråga=fråga träffar=data.träffar />
        </section>
    }
    .into_any()
}
