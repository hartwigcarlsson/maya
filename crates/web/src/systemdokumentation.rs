//! Inställningar → Systemdokumentation (S1–S6): dokumentet som `core` genererar ur företagets
//! data och de inbyggda reglerna, avsnitt för avsnitt, och samma dokument som PDF.
//!
//! Sidan ritar bara blocken; all text kommer ur `maya_core::systemdokumentation`. Den läser sin
//! data med `sidodata::sida`.

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use maya_domain::systemdokumentation::{
    Avsnitt, Block, Stil, Systemdokumentation, Tabell, Tabellrad, Text,
};
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{Företagsval, IngenÅtkomst};
use crate::installningar::Inställningsmeny;
use crate::sidodata::sida;
use crate::utskrift::Utskrift;

/// Tabeller med fler rader än så visas hopfällda (kontoplanen och INK2-tabellen).
const FÄLL_IHOP_FRÅN: usize = 40;

/// Det som sidan visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Systemdokumentationsdata {
    pub dokument: Systemdokumentation,
    /// PDF:ens filnamn, t.ex. `systemdokumentation_5560360793_20261018.pdf`.
    pub filnamn: String,
}

/// Datan för `/foretag/<id>/installningar/systemdokumentation`.
#[server(prefix = "/api", endpoint = "systemdokumentation", input = server_fn::codec::GetUrl)]
pub async fn hämta_systemdokumentation(
    adress: String,
) -> Result<Systemdokumentationsdata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::systemdokumentation(&begäran).await,
        None => Ok(Systemdokumentationsdata::default()),
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;

    use super::Systemdokumentationsdata;
    use crate::sidodata::{self, Sidbegäran};

    /// Hämtar dokumentet. Utan åtkomst blir datan tom; sidan visar det ur företagsvalet.
    pub(crate) async fn systemdokumentation(
        b: &Sidbegäran,
    ) -> Result<Systemdokumentationsdata, ServerFnError> {
        let delar = b.delar();
        let ["foretag", id, "installningar", "systemdokumentation"] = delar.as_slice() else {
            return Ok(Systemdokumentationsdata::default());
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Ok(Systemdokumentationsdata::default());
        };
        match maya_core::systemdokumentation::systemdokumentation(
            &b.db,
            &b.config,
            &b.inloggad.aktör,
            företag,
        )
        .await
        {
            Ok(dokument) => Ok(Systemdokumentationsdata {
                filnamn: maya_core::utskrift::systemdokumentationens_filnamn(
                    &dokument.orgnr,
                    b.klocka.idag(),
                ),
                dokument,
            }),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                Ok(Systemdokumentationsdata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }
}

/// Inställningar → Systemdokumentation.
#[component]
pub fn SystemdokumentationSida() -> impl IntoView {
    sida(hämta_systemdokumentation, systemdokumentationssida)
}

fn systemdokumentationssida(data: Systemdokumentationsdata) -> AnyView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let val = use_context::<Företagsval>().unwrap_or_default();
    let Some(företaget) = val.0.into_iter().find(|f| f.id == id) else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let bas = företaget.href();
    let pdf = format!("{bas}/installningar/systemdokumentation/pdf");
    let d = data.dokument;
    let innehåll = d
        .avsnitt
        .iter()
        .map(|a| {
            view! {
                <li>
                    <a href=format!("#{}", a.id)>{a.rubrik.clone()}</a>
                </li>
            }
        })
        .collect_view();
    let avsnitt = d.avsnitt.into_iter().map(avsnittet).collect_view();
    view! {
        <section class="sida systemdokumentation">
            <Inställningsmeny bas=bas.clone() aktiv="/systemdokumentation" />
            <p class="diskret">{företaget.namn} " · " {företaget.orgnr}</p>
            <h1>"Systemdokumentation"</h1>
            <p class="diskret">
                "Genereras ur företagets data och Mayas inbyggda regler varje gång sidan visas. Maya "
                {d.version} " · INK2-tabell " {d.ink2} " · " {d.bas}
            </p>
            <Utskrift href=pdf filnamn=data.filnamn />
            <nav class="sd-innehall" aria-label="Innehåll">
                <ol>{innehåll}</ol>
            </nav>
            {avsnitt}
        </section>
    }
    .into_any()
}

fn avsnittet(a: Avsnitt) -> impl IntoView {
    let krav = format!("Krav: {}", a.krav.join(", "));
    let block = a.block.into_iter().map(blocket).collect_view();
    view! {
        <section class="sd-avsnitt" id=a.id>
            <h2>{a.rubrik}</h2>
            <p class="diskret sd-krav">{krav}</p>
            {block}
        </section>
    }
}

fn texten(t: Text) -> impl IntoView {
    t.0.into_iter()
        .map(|d| match d.stil {
            Stil::Fet => view! { <strong>{d.text}</strong> }.into_any(),
            Stil::Kursiv => view! { <em>{d.text}</em> }.into_any(),
            Stil::Vanlig => d.text.into_any(),
        })
        .collect_view()
}

fn tabellen(t: Tabell) -> AnyView {
    let n = t.kolumner.len();
    let antal = t
        .rader
        .iter()
        .filter(|r| matches!(r, Tabellrad::Rad(_)))
        .count();
    let huvud = t
        .kolumner
        .into_iter()
        .map(|k| view! { <th>{k}</th> })
        .collect_view();
    let rader = t
        .rader
        .into_iter()
        .map(|r| match r {
            Tabellrad::Rubrik(r) => view! {
                <tr class="sd-rubrik">
                    <th colspan=n>{r}</th>
                </tr>
            }
            .into_any(),
            Tabellrad::Rad(celler) => {
                let celler = celler
                    .into_iter()
                    .map(|c| view! { <td>{texten(c)}</td> })
                    .collect_view();
                view! { <tr>{celler}</tr> }.into_any()
            }
        })
        .collect_view();
    let tabell = view! {
        <table class="installningstabell sd-tabell">
            <thead>
                <tr>{huvud}</tr>
            </thead>
            <tbody>{rader}</tbody>
        </table>
    };
    if antal > FÄLL_IHOP_FRÅN {
        view! {
            <details class="sd-hopfalld">
                <summary>{format!("Visa tabellen ({antal} rader)")}</summary>
                {tabell}
            </details>
        }
        .into_any()
    } else {
        tabell.into_any()
    }
}

fn blocket(b: Block) -> AnyView {
    match b {
        Block::Underrubrik(t) => view! { <h3>{t}</h3> }.into_any(),
        Block::Stycke(t) => view! { <p>{texten(t)}</p> }.into_any(),
        Block::Lista(punkter) => {
            let punkter = punkter
                .into_iter()
                .map(|p| view! { <li>{texten(p)}</li> })
                .collect_view();
            view! { <ul>{punkter}</ul> }.into_any()
        }
        Block::Fakta(rader) => {
            let rader = rader
                .into_iter()
                .map(|(rubrik, värde)| {
                    view! {
                        <dt>{rubrik}</dt>
                        <dd>{texten(värde)}</dd>
                    }
                })
                .collect_view();
            view! { <dl class="fakta">{rader}</dl> }.into_any()
        }
        Block::Tabell(t) => tabellen(t),
        Block::Varning(t) => view! {
            <div class="driftvarning" role="alert">
                <span class="badge badge-drift">"varning"</span>
                <p class="varning">{texten(t)}</p>
            </div>
        }
        .into_any(),
        Block::Länk { text, href } => {
            // PDF:er är filer, inte sidor i appen.
            let extern_fil = href.ends_with("/pdf").then_some("external");
            view! {
                <p class="sd-lank">
                    <a href=href rel=extern_fil>
                        {text}
                    </a>
                </p>
            }
            .into_any()
        }
    }
}
