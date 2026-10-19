//! Panelen Rätta kontering på en utfärdad kundfaktura (#142), `?f=<faktura>&panel=ratta` i
//! Kundfakturor (prototypens panel *Rätta konteringen*): ett nytt intäktskonto per rad och en
//! valfri orsak. Kontona prövas med `maya_domain::kontering` i webbläsaren och i `core`.

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use maya_domain::kontering::{Intäktsrad, NyKontering};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kundfaktura::{Fält, Radfält};
use serde::{Deserialize, Serialize};

/// En rad i panelen: beskrivning, netto och kontot som raden är bokförd på nu.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KonteringsradVy {
    pub beskrivning: String,
    pub netto: i64,
    pub konto: u16,
}

/// Det som panelen visar och prövar mot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RättaKonteringVy {
    pub rader: Vec<KonteringsradVy>,
    /// Det gällande utfärdandeverifikatets nummer, t.ex. `V1`.
    pub verifikat: String,
    /// Numren som rättelsen och det nya verifikatet får om de bokförs nu, när dagens datum
    /// ligger i ett räkenskapsår.
    pub nästa: Option<(String, String)>,
    pub idag: String,
    /// Kontona och om de är aktiva.
    pub konton: Vec<(u16, bool)>,
    /// Aktiva konton i klass 3 med namn, för kontoväljaren.
    pub intäktskonton: Vec<(u16, String)>,
}

impl RättaKonteringVy {
    fn intäktsrader(&self) -> Vec<Intäktsrad> {
        self.rader
            .iter()
            .filter_map(|r| {
                Some(Intäktsrad {
                    beskrivning: r.beskrivning.clone(),
                    netto: r.netto,
                    konto: Kontonummer::new(r.konto)?,
                })
            })
            .collect()
    }

    fn kontoplanen(&self) -> Vec<(Kontonummer, bool)> {
        self.konton
            .iter()
            .filter_map(|(k, aktiv)| Some((Kontonummer::new(*k)?, *aktiv)))
            .collect()
    }
}

/// Formuläret ur fälten `konto_<rad>` och `orsak`, med `antal` rader.
pub(crate) fn ur_fält(antal: usize, värde: impl Fn(&str) -> Option<String>) -> NyKontering {
    NyKontering {
        konton: (0..antal)
            .map(|i| värde(&format!("konto_{i}")).unwrap_or_default())
            .collect(),
        orsak: värde("orsak").unwrap_or_default(),
    }
}

/// Panelen Rätta kontering för fakturan `id` i listan `lista`.
#[component]
pub fn RättaKonteringPanel(lista: String, id: String, p: RättaKonteringVy) -> impl IntoView {
    let här = format!("{lista}?f={id}");
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let tillbakaskickat = fråga("fel").as_deref() == Some("kontering");
    let start = if tillbakaskickat {
        ur_fält(p.rader.len(), fråga)
    } else {
        NyKontering {
            konton: p.rader.iter().map(|r| r.konto.to_string()).collect(),
            orsak: String::new(),
        }
    };
    let formulär = RwSignal::new(start);
    let (rader, konton) = (p.intäktsrader(), p.kontoplanen());
    let spärrar = Memo::new(move |_| {
        formulär.with(|f| f.kontrollera(&rader, &konton).err().unwrap_or_default())
    });
    let hydrerad = RwSignal::new(false);
    Effect::new(move |_| hydrerad.set(true));
    let spärr_vid = move |fält: Fält| {
        move || {
            spärrar.with(|s| {
                s.iter().find(|s| s.fält == fält).map(|s| {
                    view! { <p class="fel" role="alert" data-kod=s.kod>{s.text.clone()}</p> }
                })
            })
        }
    };
    // Att inget konto är ändrat visas först när servern har sagt det; i webbläsaren räcker det
    // att knappen är inaktiv.
    let spärr_för_raderna = move || {
        spärrar.with(|s| {
            s.iter()
                .find(|s| {
                    s.fält == Fält::Rader && (tillbakaskickat || s.kod != "kontering_oforandrad")
                })
                .map(|s| view! { <p class="fel" role="alert" data-kod=s.kod>{s.text.clone()}</p> })
        })
    };
    let tabell = p
        .rader
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let konto = formulär.with_untracked(|f| f.konton[i].clone());
            view! {
                <tr>
                    <td>{r.beskrivning.clone()}</td>
                    <td class="belopp">{maya_domain::belopp::Saldo::från_öre(r.netto).to_string()}</td>
                    <td class="diskret">{r.konto.to_string()}</td>
                    <td>
                        <input
                            name=format!("konto_{i}")
                            type="text"
                            inputmode="numeric"
                            list="rattade-intaktskonton"
                            aria-label="Nytt intäktskonto"
                            prop:value=konto
                            on:input=move |ev| formulär.update(|f| f.konton[i] = event_target_value(&ev))
                        />
                        {spärr_vid(Fält::Rad(i, Radfält::Konto))}
                    </td>
                </tr>
            }
        })
        .collect_view();
    let intäktskonton = p
        .intäktskonton
        .iter()
        .map(|(k, namn)| view! { <option value=k.to_string()>{format!("{k} {namn}")}</option> })
        .collect_view();
    let följd = match &p.nästa {
        Some((rättelse, ny)) => format!(
            "{rättelse} vänder {}, {ny} bokförs med de nya kontona. Båda dateras {}.",
            p.verifikat, p.idag
        ),
        None => format!(
            "En rättelse vänder {}, och ett nytt verifikat bokförs med de nya kontona. Båda \
             dateras {}.",
            p.verifikat, p.idag
        ),
    };
    let knapp = format!("Rätta {}", p.verifikat);
    let orsak = formulär.with_untracked(|f| f.orsak.clone());
    view! {
        <section class="dialog rattelse" role="dialog" aria-labelledby="ratta-kontering-rubrik">
            <h2 id="ratta-kontering-rubrik">"Rätta konteringen"</h2>
            <p class="diskret">
                "Välj ett nytt intäktskonto per rad. Fakturan, numret och PDF:en ändras inte."
            </p>
            <form method="post" action=format!("{lista}/{id}/ratta-kontering")>
                <datalist id="rattade-intaktskonton">{intäktskonton}</datalist>
                <table class="rader">
                    <thead>
                        <tr>
                            <th>"Rad"</th>
                            <th class="belopp">"Belopp"</th>
                            <th>"Bokförd på"</th>
                            <th>"Intäktskonto"</th>
                        </tr>
                    </thead>
                    <tbody>{tabell}</tbody>
                </table>
                <label>
                    "Orsak (valfri)"
                    <input
                        name="orsak"
                        type="text"
                        prop:value=orsak
                        on:input=move |ev| formulär.update(|f| f.orsak = event_target_value(&ev))
                    />
                </label>
                <p class="diskret">{följd}</p>
                {spärr_för_raderna}
                <a class="knapp" href=här>
                    "Avbryt"
                </a>
                " "
                <button
                    class="knapp knapp-primar"
                    type="submit"
                    disabled=move || hydrerad.get() && !spärrar.with(Vec::is_empty)
                >
                    {knapp}
                </button>
            </form>
        </section>
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::{Extension, Form};
    use maya_core::aktor::Aktör;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::kundfaktura::{KundfakturaFel, KundfakturaId};
    use maya_core::verifikat::{self, VerifikatFel};
    use maya_core::{Databas, kontering, kontoplan};
    use maya_domain::kontering::Intäktsrad;

    use super::*;
    use crate::kundfakturor::server::{KundfakturaState, fakturafel, finns_inte, tolka};

    /// Fakturans gällande kontering, eller `None` när den inte har något utfärdandeverifikat.
    pub(crate) async fn gällande(
        db: &Databas,
        aktör: &Aktör,
        företag: FöretagsId,
        f: KundfakturaId,
    ) -> Result<Option<kontering::Kontering>, KundfakturaFel> {
        match kontering::kontering(db, aktör, företag, f).await {
            Ok(k) => Ok(Some(k)),
            Err(KundfakturaFel::Verifikat(VerifikatFel::IngetVerifikat)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Det som panelen visar för konteringen `k`, med dagens datum `idag`.
    pub(crate) async fn panelen(
        db: &Databas,
        aktör: &Aktör,
        företag: FöretagsId,
        k: &kontering::Kontering,
        idag: jiff::civil::Date,
    ) -> Result<RättaKonteringVy, KundfakturaFel> {
        let konton = kontoplan::konton(db, aktör, företag).await?;
        let nästa = verifikat::nästa_nummer(db, aktör, företag)
            .await?
            .into_iter()
            .find(|(år, _)| år.innehåller(idag))
            .map(|(_, n)| (format!("V{}", n.get()), format!("V{}", n.get() + 1)));
        Ok(RättaKonteringVy {
            rader: k
                .rader
                .iter()
                .map(|r: &Intäktsrad| KonteringsradVy {
                    beskrivning: r.beskrivning.clone(),
                    netto: r.netto,
                    konto: r.konto.get(),
                })
                .collect(),
            verifikat: k
                .verifikat
                .nummer
                .map(|n| n.to_string())
                .unwrap_or_default(),
            nästa,
            idag: idag.to_string(),
            konton: konton.iter().map(|k| (k.nummer.get(), k.aktiv)).collect(),
            intäktskonton: konton
                .iter()
                .filter(|k| k.aktiv && k.nummer.klass() == 3)
                .map(|k| (k.nummer.get(), k.namn.clone()))
                .collect(),
        })
    }

    type Fält = Vec<(String, String)>;

    /// POST `/foretag/<id>/kundfakturor/<f>/ratta-kontering`.
    pub(crate) async fn rätta(
        State(state): State<Arc<KundfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(f) = f.parse::<KundfakturaId>() else {
            return finns_inte();
        };
        let värde = |namn: &str| fält.iter().find(|(k, _)| k == namn).map(|(_, v)| v.clone());
        let antal = (0..500)
            .take_while(|i| värde(&format!("konto_{i}")).is_some())
            .count();
        let formulär = ur_fält(antal, värde);
        let här = format!("/foretag/{företag}/kundfakturor?f={f}");
        match kontering::rätta(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            f,
            &formulär,
        )
        .await
        {
            Ok(_) => Redirect::to(&format!("{här}&rattad=1")).into_response(),
            Err(KundfakturaFel::Spärrat(_)) => {
                let mut query = url::form_urlencoded::Serializer::new(String::new());
                query.append_pair("panel", "ratta");
                query.append_pair("fel", "kontering");
                for (namn, v) in &fält {
                    query.append_pair(namn, v);
                }
                Redirect::to(&format!("{här}&{}", query.finish())).into_response()
            }
            Err(KundfakturaFel::Verifikat(
                VerifikatFel::Spärrat(_) | VerifikatFel::InteRättbart(_),
            )) => Redirect::to(&format!("{här}&panel=ratta&fel=rattelse")).into_response(),
            Err(KundfakturaFel::Verifikat(VerifikatFel::IngetVerifikat)) => {
                Redirect::to(&format!("{här}&fel=ingen_kontering")).into_response()
            }
            Err(e) => fakturafel(e),
        }
    }
}
