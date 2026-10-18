//! Sidan Installation i användarmenyn (beslut 12 i #70): version, publik URL, senaste backup och
//! verifiering och arkivplanen. Bara läsning; konfigurationen ändras bara på servern.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::att_gora::DriftvarningVy;
use crate::sidodata::sida;

/// Det som sidan Installation visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installationsdata {
    pub version: String,
    pub publik_url: String,
    pub backupmål: String,
    pub backuptid: String,
    /// Den senaste backupen, t.ex. "2026-10-18 03:00, lyckades".
    pub senaste_backup: String,
    pub senaste_lyckade_backup: String,
    pub senaste_verifiering: String,
    /// Arkivplanens fritext: (rubrik, värde eller "ej angivet").
    pub arkivplan: Vec<(String, String)>,
    pub larm: String,
    pub driftvarningar: Vec<DriftvarningVy>,
}

/// Datan för `/installation`.
#[server(prefix = "/api", endpoint = "installation", input = server_fn::codec::GetUrl)]
pub async fn hämta_installation(adress: String) -> Result<Installationsdata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::installation(&begäran).await,
        None => Ok(Installationsdata::default()),
    }
}

#[cfg(feature = "ssr")]
mod server {
    use leptos::prelude::ServerFnError;
    use maya_core::backup::Körning;
    use maya_core::drift;

    use super::Installationsdata;
    use crate::att_gora::DriftvarningVy;
    use crate::sidodata::{Sidbegäran, serverfel};

    const EJ_ANGIVET: &str = "ej angivet";

    pub(crate) async fn installation(b: &Sidbegäran) -> Result<Installationsdata, ServerFnError> {
        let i = drift::installation(&b.db, &b.klocka, &b.config, &b.inloggad.aktör)
            .await
            .map_err(serverfel)?;
        let körning = |k: Option<Körning>, lyckad: &str| match k {
            None => "ingen än".to_string(),
            Some(k) => {
                let utfall = if k.lyckades { lyckad } else { "misslyckades" };
                let backup = k.backup.map(|b| format!(" ({b})")).unwrap_or_default();
                format!("{}, {utfall}{backup}", tid(k.tid))
            }
        };
        let fritext = |rubrik: &str, värde: &Option<String>| {
            (
                rubrik.to_string(),
                värde.clone().unwrap_or_else(|| EJ_ANGIVET.into()),
            )
        };
        Ok(Installationsdata {
            version: i.version.to_string(),
            publik_url: i.publik_url,
            backupmål: i.backupmål,
            backuptid: format!(
                "varje natt {} (Europe/Stockholm); 7 dagliga och 12 månatliga sparas",
                b.config.backup_time.strftime("%H:%M")
            ),
            senaste_backup: körning(i.backup.senaste_backup, "lyckades"),
            senaste_lyckade_backup: körning(i.backup.senaste_lyckade_backup, "lyckades"),
            senaste_verifiering: körning(i.backup.senaste_verifiering, "verifierad"),
            arkivplan: vec![
                fritext("Plats och land", &i.arkivplan.plats),
                fritext("Offsite", &i.arkivplan.offsite),
                fritext("Krypteringsnyckel", &i.arkivplan.nyckel),
                fritext("Ansvarig för driften", &i.arkivplan.ansvarig),
                fritext("Larmadress", &i.larmadress),
            ],
            larm: match (&i.larmadress, b.config.smtp.is_some()) {
                (Some(adress), true) => format!("med e-post till {adress}"),
                (Some(_), false) => "av (MAYA_SMTP_HOST saknas)".into(),
                (None, true) => "av (MAYA_DRIFT_EPOST saknas)".into(),
                (None, false) => "av (MAYA_SMTP_HOST och MAYA_DRIFT_EPOST saknas)".into(),
            },
            driftvarningar: i
                .driftvarningar
                .iter()
                .map(|v| DriftvarningVy {
                    rubrik: v.rubrik().into(),
                    text: v.text(),
                })
                .collect(),
        })
    }

    /// En tidpunkt i Stockholm, till minuten.
    fn tid(t: jiff::Timestamp) -> String {
        t.to_zoned(maya_domain::tid::stockholm())
            .strftime("%Y-%m-%d %H:%M")
            .to_string()
    }
}

#[component]
pub fn InstallationSida() -> impl IntoView {
    sida(hämta_installation, installation)
}

fn installation(data: Installationsdata) -> AnyView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let varningar = data
        .driftvarningar
        .into_iter()
        .map(|v| {
            view! {
                <div class="driftvarning" role="alert">
                    <span class="badge badge-drift">"drift"</span>
                    <div>
                        <b>{v.rubrik}</b>
                        <p>{v.text}</p>
                    </div>
                </div>
            }
        })
        .collect_view();
    let arkivplan = data
        .arkivplan
        .into_iter()
        .map(|(rubrik, värde)| {
            view! {
                <dt>{rubrik}</dt>
                <dd>{värde}</dd>
            }
        })
        .collect_view();
    view! {
        <section class="sida installation">
            <h1>"Installation"</h1>
            <p class="diskret">
                "Konfigurationen ändras bara på servern, i Mayas .env. Den här sidan visar vad som gäller."
            </p>
            <div class="driftvarningar">{varningar}</div>
            <h2>"Maya"</h2>
            <dl class="fakta">
                <dt>"Version"</dt>
                <dd>{data.version}</dd>
                <dt>"Publik URL"</dt>
                <dd>{data.publik_url}</dd>
                <dt>"Larm till driften"</dt>
                <dd>{data.larm}</dd>
            </dl>
            <h2>"Backup"</h2>
            <dl class="fakta">
                <dt>"Backupmål"</dt>
                <dd>{data.backupmål}</dd>
                <dt>"Schema"</dt>
                <dd>{data.backuptid}</dd>
                <dt>"Senaste backup"</dt>
                <dd>{data.senaste_backup}</dd>
                <dt>"Senaste lyckade backup"</dt>
                <dd>{data.senaste_lyckade_backup}</dd>
                <dt>"Senaste verifiering"</dt>
                <dd>{data.senaste_verifiering}</dd>
            </dl>
            <h2>"Arkivplan"</h2>
            <dl class="fakta">{arkivplan}</dl>
        </section>
    }
    .into_any()
}
