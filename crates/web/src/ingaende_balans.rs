//! Inställningar → Ingående balans: formuläret över balanskontona för företagets första
//! räkenskapsår, och den preliminära ingående balansen för senare år, märkt som preliminär.
//!
//! Formuläret prövar summan och kontona med `maya_domain::ingaende_balans` i webbläsaren, och
//! `core` prövar samma sak när det sparar. Utan JavaScript skickas formuläret till servern, som
//! skickar tillbaka hit med det användaren skrev; sidan prövar det igen och visar samma koder.
//!
//! Sidan läser sin data med `sidodata::sida`.

use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use maya_domain::belopp::{Belopp, Saldo};
use maya_domain::ingaende_balans::{Fält, NyIngåendeBalans, NySaldorad, Spärr};
use maya_domain::kontoplan::Kontonummer;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{Företagsval, IngenÅtkomst};
use crate::installningar::Inställningsmeny;
use crate::sidodata::sida;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KontoVy {
    pub nummer: u16,
    pub namn: String,
    pub aktiv: bool,
}

/// Det som sidan visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngåendeBalansdata {
    /// Räkenskapsåren i ordning, som start och slut `ÅÅÅÅ-MM-DD`.
    pub år: Vec<(String, String)>,
    /// Startdatum för det valda året.
    pub valt: String,
    pub preliminär: bool,
    /// Kontona i formuläret: aktiva balanskonton och de som har ett saldo.
    pub konton: Vec<KontoVy>,
    /// Den ingående balansen per konto i öre, debet positivt.
    pub saldon: Vec<(u16, i64)>,
}

/// Datan för `/foretag/<id>/installningar/ingaende-balans?ar=<start>`.
#[server(prefix = "/api", endpoint = "ingaende_balans", input = server_fn::codec::GetUrl)]
pub async fn hämta_ingående_balans(adress: String) -> Result<IngåendeBalansdata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::ingående_balans(&begäran).await,
        None => Ok(IngåendeBalansdata::default()),
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
    use maya_core::foretag::{self, FöretagsId};
    use maya_core::inloggning::Inloggad;
    use maya_core::saldo::{self, IngåendeBalansFel, SaldoFel, Status};
    use maya_core::{Databas, Klocka, kontoplan};
    use maya_domain::ingaende_balans::{NyIngåendeBalans, NySaldorad};
    use maya_domain::kontoplan::Kontonummer;

    use super::{IngåendeBalansdata, KontoVy};
    use crate::sidodata::{self, Sidbegäran};

    #[derive(Clone)]
    pub(crate) struct IngåendeBalansState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: IngåendeBalansState) -> Router {
        Router::new()
            .route("/foretag/{id}/ingaende-balans", post(spara))
            .with_state(Arc::new(state))
    }

    /// Hämtar det som sidan visar, för `/foretag/<id>/installningar/ingaende-balans?ar=<start>`.
    /// Utan åtkomst blir datan tom; sidan visar det ur företagsvalet.
    pub(crate) async fn ingående_balans(
        b: &Sidbegäran,
    ) -> Result<IngåendeBalansdata, ServerFnError> {
        let delar = b.delar();
        let ["foretag", id, "installningar", "ingaende-balans"] = delar.as_slice() else {
            return Ok(IngåendeBalansdata::default());
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Ok(IngåendeBalansdata::default());
        };
        let valt = b.värde("ar").and_then(|v| v.parse::<Date>().ok());
        match hämta(&b.db, &b.inloggad, företag, valt).await {
            Ok(data) => Ok(data),
            Err(SaldoFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                Ok(IngåendeBalansdata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    async fn hämta(
        db: &Databas,
        inloggad: &Inloggad,
        företag: FöretagsId,
        valt: Option<Date>,
    ) -> Result<IngåendeBalansdata, SaldoFel> {
        let aktör = &inloggad.aktör;
        let år = foretag::företaget(db, aktör, företag).await?.räkenskapsår;
        let valt = valt
            .filter(|v| år.iter().any(|å| å.start == *v))
            .or_else(|| år.first().map(|å| å.start))
            .ok_or(ÅtkomstFel::IngenÅtkomst)?;
        let ib = saldo::ingående_balans(db, aktör, företag, valt).await?;
        let balanskonton = saldo::balanskonton(db, aktör, företag).await?;
        let alla = kontoplan::konton(db, aktör, företag).await?;
        let namn = |nr: Kontonummer| {
            alla.iter()
                .find(|k| k.nummer == nr)
                .map(|k| k.namn.clone())
                .unwrap_or_default()
        };
        let konton = balanskonton
            .iter()
            .filter(|(nr, aktiv)| *aktiv || ib.saldon.iter().any(|(k, _)| k == nr))
            .map(|&(nr, aktiv)| KontoVy {
                nummer: nr.get(),
                namn: namn(nr),
                aktiv,
            })
            .collect();
        Ok(IngåendeBalansdata {
            år: år
                .iter()
                .map(|å| (å.start.to_string(), å.slut.to_string()))
                .collect(),
            valt: valt.to_string(),
            preliminär: ib.status == Status::Preliminär,
            konton,
            saldon: ib.saldon.iter().map(|(k, s)| (k.get(), s.öre())).collect(),
        })
    }

    /// Formuläret, med raderna som `konto_<i>`, `debet_<i>` och `kredit_<i>`.
    fn tolka(fält: &[(String, String)]) -> (String, NyIngåendeBalans) {
        let värde = |namn: &str| {
            fält
                .iter()
                .find(|(k, _)| k == namn)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let antal = fält
            .iter()
            .filter_map(|(k, _)| k.rsplit_once('_')?.1.parse::<usize>().ok())
            .map(|i| i + 1)
            .max()
            .unwrap_or(0)
            .min(10_000);
        let rader = (0..antal)
            .map(|i| NySaldorad {
                konto: värde(&format!("konto_{i}")),
                debet: värde(&format!("debet_{i}")),
                kredit: värde(&format!("kredit_{i}")),
            })
            .collect();
        (värde("ar"), NyIngåendeBalans { rader })
    }

    async fn spara(
        State(state): State<Arc<IngåendeBalansState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Vec<(String, String)>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let (år, formulär) = tolka(&fält);
        let sida = format!("/foretag/{företag}/installningar/ingaende-balans");
        let Ok(start) = år.parse::<Date>() else {
            return Redirect::to(&sida).into_response();
        };
        match saldo::ändra_ingående_balans(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            start,
            &formulär,
        )
        .await
        {
            Ok(()) => Redirect::to(&format!("{sida}?ar={start}")).into_response(),
            Err(IngåendeBalansFel::Spärrat(_)) => tillbaka(&sida, start, &formulär),
            Err(IngåendeBalansFel::InteFörstaÅret | IngåendeBalansFel::OkäntRäkenskapsår) => {
                Redirect::to(&format!("{sida}?ar={start}")).into_response()
            }
            Err(IngåendeBalansFel::Åtkomst(
                ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte,
            )) => ingen_åtkomst(),
            Err(e) => serverfel(e),
        }
    }

    /// Tillbaka till formuläret med raderna som användaren fyllde i. Sidan prövar dem igen och
    /// visar felen vid fälten.
    fn tillbaka(sida: &str, år: Date, f: &NyIngåendeBalans) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("ar", &år.to_string());
        for (i, rad) in f.rader.iter().enumerate() {
            if rad.debet.trim().is_empty() && rad.kredit.trim().is_empty() {
                continue;
            }
            query
                .append_pair(&format!("konto_{i}"), &rad.konto)
                .append_pair(&format!("debet_{i}"), &rad.debet)
                .append_pair(&format!("kredit_{i}"), &rad.kredit);
        }
        Redirect::to(&format!("{sida}?{}", query.finish())).into_response()
    }
}

fn saldo_i(saldo: i64, sida: fn(Saldo) -> Belopp) -> String {
    sida(Saldo::från_öre(saldo)).eller_tomt()
}

/// Inställningar → Ingående balans.
#[component]
pub fn IngåendeBalansSida() -> impl IntoView {
    sida(hämta_ingående_balans, ingående_balans)
}

fn ingående_balans(data: IngåendeBalansdata) -> AnyView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let val = use_context::<Företagsval>().unwrap_or_default();
    let Some(företaget) = val.0.into_iter().find(|f| f.id == id) else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let bas = företaget.href();
    let sida = format!("{bas}/installningar/ingaende-balans");
    let årsval = data
        .år
        .iter()
        .map(|(start, slut)| {
            let aktuellt = (*start == data.valt).then_some("page");
            view! {
                <a href=format!("{sida}?ar={start}") aria-current=aktuellt>
                    {format!("{start} – {slut}")}
                </a>
            }
        })
        .collect_view();
    let innehåll = if data.preliminär {
        view! { <Preliminär data=data.clone() sida=sida.clone() /> }.into_any()
    } else {
        view! { <Formulär bas=bas.clone() data=data.clone() /> }.into_any()
    };
    view! {
        <section class="sida">
            <Inställningsmeny bas=bas.clone() aktiv="/ingaende-balans" />
            <p class="diskret">{företaget.namn} " · " {företaget.orgnr}</p>
            <h1>"Ingående balans"</h1>
            <nav class="filter" aria-label="Räkenskapsår">
                {årsval}
            </nav>
            {innehåll}
        </section>
    }
    .into_any()
}

/// Den preliminära ingående balansen: bara att läsa, märkt som preliminär.
#[component]
fn Preliminär(data: IngåendeBalansdata, sida: String) -> impl IntoView {
    let föregående = data
        .år
        .iter()
        .take_while(|(start, _)| *start != data.valt)
        .last()
        .cloned()
        .unwrap_or_default();
    let namn = |nr: u16| {
        data.konton
            .iter()
            .find(|k| k.nummer == nr)
            .map(|k| k.namn.clone())
            .unwrap_or_default()
    };
    let rader = data
        .saldon
        .iter()
        .map(|&(konto, saldo)| {
            view! {
                <tr>
                    <td>{konto}</td>
                    <td>{namn(konto)}</td>
                    <td class="belopp">{saldo_i(saldo, Saldo::debet)}</td>
                    <td class="belopp">{saldo_i(saldo, Saldo::kredit)}</td>
                </tr>
            }
        })
        .collect_view();
    view! {
        <div class="banderoll" role="status">
            <p>
                <span class="markering">"Preliminär"</span>
                " Den ingående balansen räknas fram ur föregående års utgående balans, eftersom "
                <a href=format!("{sida}?ar={}", föregående.0)>
                    {format!("{} – {}", föregående.0, föregående.1)}
                </a>
                " inte är stängt. Resultatet som inte är ombokat står på 2099. Den sparas inte och ändras när föregående år ändras."
            </p>
        </div>
        <table class="rader">
            <thead>
                <tr>
                    <th>"Konto"</th>
                    <th>"Kontonamn"</th>
                    <th class="belopp">"Debet"</th>
                    <th class="belopp">"Kredit"</th>
                </tr>
            </thead>
            <tbody>{rader}</tbody>
        </table>
    }
}

/// Formuläret över balanskontona för det första året. Felen visas vid sina fält medan
/// användaren skriver, och Spara är inaktiv med antalet problem kvar.
#[component]
fn Formulär(bas: String, data: IngåendeBalansdata) -> impl IntoView {
    // Tillbaka från servern: raderna som användaren skrev, efter kontonummer.
    let ifyllt: Option<Vec<(String, String, String)>> = use_query_map().with_untracked(|q| {
        let rader: Vec<_> = (0..data.konton.len())
            .filter_map(|i| {
                Some((
                    q.get(&format!("konto_{i}"))?,
                    q.get(&format!("debet_{i}")).unwrap_or_default(),
                    q.get(&format!("kredit_{i}")).unwrap_or_default(),
                ))
            })
            .collect();
        (!rader.is_empty()).then_some(rader)
    });
    let start = NyIngåendeBalans {
        rader: data
            .konton
            .iter()
            .map(|k| {
                let konto = k.nummer.to_string();
                let (debet, kredit) = match &ifyllt {
                    Some(rader) => rader
                        .iter()
                        .find(|(nr, _, _)| *nr == konto)
                        .map(|(_, d, k)| (d.clone(), k.clone()))
                        .unwrap_or_default(),
                    None => {
                        let saldo = data
                            .saldon
                            .iter()
                            .find(|(nr, _)| *nr == k.nummer)
                            .map_or(0, |(_, s)| *s);
                        (saldo_i(saldo, Saldo::debet), saldo_i(saldo, Saldo::kredit))
                    }
                };
                NySaldorad {
                    konto,
                    debet,
                    kredit,
                }
            })
            .collect(),
    };
    let balans = RwSignal::new(start);
    let konton: Vec<(Kontonummer, bool)> = data
        .konton
        .iter()
        .filter_map(|k| Some((Kontonummer::new(k.nummer)?, k.aktiv)))
        .collect();
    let spärrar =
        Memo::new(move |_| balans.with(|b| b.kontrollera(&konton).err().unwrap_or_default()));
    let hydrerad = RwSignal::new(false);
    Effect::new(move |_| hydrerad.set(true));
    let skicka = move |ev: leptos::ev::SubmitEvent| {
        if !spärrar.with_untracked(Vec::is_empty) {
            ev.prevent_default();
        }
    };
    let spärr_vid = move |fält: Fält| {
        move || {
            spärrar.with(|s| {
                s.iter().find(|s| s.fält == fält).map(|s: &Spärr| {
                    view! {
                        <p class="fel" role="alert" data-kod=s.kod>
                            {s.text.clone()}
                        </p>
                    }
                })
            })
        }
    };
    let summa = move |sida: fn(&NySaldorad) -> &String| {
        move || {
            let öre: i64 = balans.with(|b| {
                b.rader
                    .iter()
                    .filter_map(|r| Belopp::tolka(sida(r)).map(Belopp::öre))
                    .sum()
            });
            Belopp::från_öre(öre).to_string()
        }
    };
    let spara_text = move || match spärrar.with(Vec::len) {
        0 => "Spara".to_string(),
        1 => "Spara · 1 problem kvar".to_string(),
        n => format!("Spara · {n} problem kvar"),
    };
    let rader = data
        .konton
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let fält = move |f: fn(&NySaldorad) -> &String| {
                balans.with_untracked(|b| f(&b.rader[i]).clone())
            };
            view! {
                <tr>
                    <td>
                        {k.nummer}
                        <input type="hidden" name=format!("konto_{i}") value=k.nummer.to_string() />
                        {spärr_vid(Fält::Konto(i))}
                    </td>
                    <td>{k.namn.clone()}</td>
                    <td>
                        <input
                            name=format!("debet_{i}")
                            type="text"
                            inputmode="decimal"
                            class="belopp"
                            aria-label=format!("Debet {}", k.nummer)
                            value=fält(|r| &r.debet)
                            on:input=move |ev| {
                                balans.update(|b| b.rader[i].debet = event_target_value(&ev))
                            }
                        />
                        {spärr_vid(Fält::Belopp(i))}
                    </td>
                    <td>
                        <input
                            name=format!("kredit_{i}")
                            type="text"
                            inputmode="decimal"
                            class="belopp"
                            aria-label=format!("Kredit {}", k.nummer)
                            value=fält(|r| &r.kredit)
                            on:input=move |ev| {
                                balans.update(|b| b.rader[i].kredit = event_target_value(&ev))
                            }
                        />
                    </td>
                </tr>
            }
        })
        .collect_view();
    view! {
        <form
            class="verifikatformular"
            method="post"
            action=format!("{bas}/ingaende-balans")
            novalidate
            on:submit=skicka
        >
            <input type="hidden" name="ar" value=data.valt.clone() />
            <p class="diskret">
                "Saldot på varje balanskonto när företagets första räkenskapsår i Maya börjar. Summan ska vara noll. Varje ändring loggas i behandlingshistoriken."
            </p>
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Konto"</th>
                        <th>"Kontonamn"</th>
                        <th class="belopp">"Debet"</th>
                        <th class="belopp">"Kredit"</th>
                    </tr>
                </thead>
                <tbody>{rader}</tbody>
                <tfoot>
                    <tr>
                        <td colspan="2">"Summa"</td>
                        <td class="belopp">{summa(|r| &r.debet)}</td>
                        <td class="belopp">{summa(|r| &r.kredit)}</td>
                    </tr>
                </tfoot>
            </table>
            {spärr_vid(Fält::Summa)}
            <p>
                <button
                    class="knapp knapp-primar"
                    type="submit"
                    disabled=move || hydrerad.get() && !spärrar.with(Vec::is_empty)
                >
                    {spara_text}
                </button>
            </p>
        </form>
    }
}
