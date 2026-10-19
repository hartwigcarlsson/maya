//! Betalningar i fakturans dokument (#139): panelen Registrera betalning med "Bokförs som",
//! listan över betalningar med det som återstår, och panelen Rätta betalning. Panelen prövar
//! betalningen med `maya_domain::betalning` i webbläsaren och i `core`, så att spärrarna visas
//! vid fälten. Kund- och leverantörsfakturor (#147) använder samma komponenter.

use jiff::civil::Date;
use leptos::prelude::*;
use maya_domain::belopp::Saldo;
use maya_domain::betalning::{
    Fält, Förutsättningar, NyBetalning, Reskontra, STÖRSTA_ÖRESUTJÄMNING, Spärr,
};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::Räkenskapsår;
use serde::{Deserialize, Serialize};

/// Ett belopp i öre som det visas.
fn kr(öre: i64) -> String {
    Saldo::från_öre(öre).to_string()
}

/// En rad i "Bokförs som": konto, kontonamn, debet och kredit.
pub type KonteringVy = (String, String, String, String);

/// Det som panelen Registrera betalning prövas mot och visar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BetalningsVy {
    pub reskontra: Reskontra,
    /// Det som återstår i öre.
    pub återstår: i64,
    pub förfallodag: String,
    /// Kontona och om de är aktiva.
    pub konton: Vec<(u16, bool)>,
    /// Räkenskapsårens start och slut.
    pub räkenskapsår: Vec<(String, String)>,
    /// Aktiva konton i klass 1 med namn, för väljaren.
    pub betalkonton: Vec<(u16, String)>,
    /// Kontonamnen som "Bokförs som" kan behöva.
    pub kontonamn: Vec<(u16, String)>,
    /// Det som panelen visar när den öppnas, eller det som användaren skickade.
    pub start: NyBetalning,
    /// Felet från servern, om betalningen avvisades.
    pub fel: Option<String>,
}

impl BetalningsVy {
    fn domän(&self) -> Förutsättningar {
        let datum = |d: &str| d.parse::<Date>().ok();
        Förutsättningar {
            reskontra: self.reskontra,
            återstår: self.återstår,
            konton: self
                .konton
                .iter()
                .filter_map(|(k, aktiv)| Some((Kontonummer::new(*k)?, *aktiv)))
                .collect(),
            räkenskapsår: self
                .räkenskapsår
                .iter()
                .filter_map(|(s, e)| Some(Räkenskapsår::befintligt(datum(s)?, datum(e)?)))
                .collect(),
        }
    }
}

/// En betalning i listan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BetalningsradVy {
    pub id: String,
    pub betaldag: String,
    pub belopp: String,
    /// Tom text utan öresutjämning.
    pub öresutjämning: String,
    pub konto: String,
    /// Verifikatets id och nummer.
    pub verifikat: (String, String),
    /// Tid och namn.
    pub registrerad: (String, String),
    /// För en rättad betalning.
    pub rättad: Option<BetalningsrättelseVy>,
}

/// Rättelsen av en betalning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BetalningsrättelseVy {
    /// Rättelsens id och nummer.
    pub verifikat: (String, String),
    pub orsak: String,
    /// Tid och namn.
    pub rättad: (String, String),
}

/// Panelen Rätta betalning.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RättaBetalningVy {
    pub id: String,
    pub betaldag: String,
    pub belopp: String,
    pub verifikat: String,
    /// Rättelsens rader: betalningens verifikat vänt.
    pub rader: Vec<KonteringVy>,
    pub datum: String,
    pub orsak: String,
    pub fel: Option<String>,
}

/// Fakturan i reskontran som dokumentet visar den.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReskontraVy {
    /// Märkets kod och namn.
    pub märke: (String, String),
    pub återstår: String,
    pub återstår_öre: i64,
    pub betalningar: Vec<BetalningsradVy>,
    /// När panelen Registrera betalning är öppen.
    pub betalning: Option<BetalningsVy>,
    /// När panelen Rätta betalning är öppen.
    pub rätta: Option<RättaBetalningVy>,
}

/// Felkoderna i adressen för betalningar och deras texter.
pub fn feltext(kod: &str) -> Option<&'static str> {
    Some(match kod {
        "betalning" => "Betalningen kunde inte registreras. Rätta det som är markerat.",
        "ratta" => "Betalningen kunde inte rättas. Rätta det som är markerat.",
        "redan_rattad" => "Betalningen är redan rättad.",
        _ => return None,
    })
}

/// Märket för en faktura: `badge badge-<kod>`.
#[component]
pub fn Märke(kod: String, namn: String) -> impl IntoView {
    view! { <span class=format!("badge badge-{kod}")>{namn}</span> }
}

/// Betalningarna på fakturan, det som återstår och länken Rätta betalning… på varje betalning
/// som inte är rättad. `här` är dokumentets adress.
#[component]
pub fn Betalningar(bas: String, här: String, r: ReskontraVy) -> impl IntoView {
    let rader = r
        .betalningar
        .iter()
        .map(|b| {
            let (vid, vnr) = b.verifikat.clone();
            let läge = match &b.rättad {
                Some(BetalningsrättelseVy {
                    verifikat: (rid, rnr),
                    orsak,
                    rättad: (tid, namn),
                }) => {
                    let orsak = (!orsak.is_empty()).then(|| format!(", {orsak}"));
                    view! {
                        <span class="badge badge-rattad">"Rättad"</span>
                        " med "
                        <a href=format!("{bas}/verifikat?v={rid}")>{rnr.clone()}</a>
                        <span class="diskret">{orsak} " · " {tid.clone()} " · " {namn.clone()}</span>
                    }
                    .into_any()
                }
                None => view! {
                    <a href=format!("{här}&panel=ratta-betalning&b={}", b.id)>"Rätta betalning…"</a>
                }
                .into_any(),
            };
            let öresutjämning = (!b.öresutjämning.is_empty()).then(|| {
                view! {
                    <span class="diskret">{format!(" + {} öresutjämning", b.öresutjämning)}</span>
                }
            });
            view! {
                <tr class:rattad=b.rättad.is_some()>
                    <td>{b.betaldag.clone()}</td>
                    <td>{b.konto.clone()}</td>
                    <td>
                        <a href=format!("{bas}/verifikat?v={vid}")>{vnr}</a>
                    </td>
                    <td class="belopp">{b.belopp.clone()} {öresutjämning}</td>
                    <td>{läge}</td>
                </tr>
            }
        })
        .collect_view();
    let tom = r
        .betalningar
        .is_empty()
        .then(|| view! { <p class="diskret">"Inga betalningar än."</p> });
    view! {
        <section class="betalningar">
            <h3>"Betalningar"</h3>
            {tom}
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Betaldag"</th>
                        <th>"Konto"</th>
                        <th>"Verifikat"</th>
                        <th class="belopp">"Belopp"</th>
                        <th></th>
                    </tr>
                </thead>
                <tbody>{rader}</tbody>
                <tfoot>
                    <tr class="summa">
                        <td colspan="3">"Återstår"</td>
                        <td class="belopp">{r.återstår.clone()}</td>
                        <td></td>
                    </tr>
                </tfoot>
            </table>
        </section>
    }
}

/// "Bokförs som" för konteringen.
fn kontering(rader: &[KonteringVy]) -> impl IntoView + use<> {
    let rader = rader
        .iter()
        .map(|(konto, namn, debet, kredit)| {
            view! {
                <tr>
                    <td>{konto.clone()}</td>
                    <td>{namn.clone()}</td>
                    <td class="belopp">{debet.clone()}</td>
                    <td class="belopp">{kredit.clone()}</td>
                </tr>
            }
        })
        .collect_view();
    view! {
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

/// Panelen Registrera betalning: betaldag, belopp (förvalt det som återstår) och betalkonto,
/// delbetalningen och öresutjämningen, spärrarna vid fälten och verifikatet innan det bokförs.
#[component]
pub fn Betalningspanel(action: String, stäng: String, v: BetalningsVy) -> impl IntoView {
    let förutsättningar = v.domän();
    let betalning = RwSignal::new(v.start.clone());
    let spärrar = {
        let f = förutsättningar.clone();
        Memo::new(move |_| betalning.with(|b| b.kontrollera(&f).err().unwrap_or_default()))
    };
    let giltig = {
        let f = förutsättningar.clone();
        Memo::new(move |_| betalning.with(|b| b.kontrollera(&f).ok()))
    };
    let återstår = v.återstår;
    let rest = Memo::new(move |_| betalning.with(|b| b.rest(återstår)));
    let hydrerad = RwSignal::new(false);
    Effect::new(move |_| hydrerad.set(true));
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
    let start = v.start.clone();
    let konton = v
        .betalkonton
        .iter()
        .map(|(nr, namn)| {
            let nr = nr.to_string();
            view! {
                <option value=nr.clone() selected=nr == start.konto>
                    {format!("{nr} {namn}")}
                </option>
            }
        })
        .collect_view();
    let delbetalning = move || {
        rest.get()
            .filter(|r| *r > 0)
            .map(|r| view! { <p class="diskret delbetalning">{format!("Delbetalning, {} kr återstår.", kr(r))}</p> })
    };
    // #140: med ett tillgodohavande är panelen en återbetalning åt andra hållet.
    let återbetalning = v.återstår < 0;
    let öresutjämning = move || {
        let r = rest.get().unwrap_or_default();
        let vald = betalning.with(|b| b.öresutjämning);
        (!återbetalning && ((r > 0 && r <= STÖRSTA_ÖRESUTJÄMNING) || vald)).then(|| {
            view! {
                <label class="kryss">
                    <input
                        type="checkbox"
                        name="oresutjamning"
                        value="1"
                        checked=vald
                        on:change=move |ev| {
                            betalning.update(|b| b.öresutjämning = event_target_checked(&ev))
                        }
                    />
                    {format!(" Stäng resten {} kr mot öresutjämning (3740)", kr(r.max(0)))}
                </label>
            }
        })
    };
    let kontonamn = v.kontonamn.clone();
    let bokförs_som = move || {
        giltig.get().map(|g| {
            let namn = |nr: u16| {
                kontonamn
                    .iter()
                    .find(|(k, _)| *k == nr)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_default()
            };
            let rader: Vec<KonteringVy> = g
                .kontering()
                .iter()
                .map(|r| {
                    (
                        r.konto.to_string(),
                        namn(r.konto.get()),
                        r.debet.eller_tomt(),
                        r.kredit.eller_tomt(),
                    )
                })
                .collect();
            view! {
                <div class="kont">
                    <h4>{format!("Bokförs som verifikat · {}", g.betaldag)}</h4>
                    {kontering(&rader)}
                </div>
            }
        })
    };
    let fel = v
        .fel
        .clone()
        .map(|t| view! { <p class="fel" role="alert">{t}</p> });
    let registrera = move |ev: leptos::ev::MouseEvent| {
        if !spärrar.with_untracked(Vec::is_empty) {
            ev.prevent_default();
        }
    };
    let (rubrik, läge, till, knapp) = if återbetalning {
        (
            "Återbetala tillgodohavande",
            format!("Kunden har tillgodo {} kr", kr(-v.återstår)),
            "Från konto",
            "Återbetala",
        )
    } else {
        (
            "Registrera betalning",
            format!(
                "Återstår {} kr · förfallodag {}",
                kr(v.återstår),
                v.förfallodag
            ),
            "Till konto",
            "Registrera betalning",
        )
    };
    view! {
        <section class="panel betalningspanel" aria-labelledby="betalning-rubrik">
            <header class="panelhuvud">
                <h3 id="betalning-rubrik">{rubrik}</h3>
                <a class="knapp knapp-diskret" href=stäng>
                    "Avbryt"
                </a>
            </header>
            <p class="diskret">{läge}</p>
            {fel}
            <form method="post" action=action novalidate>
                {återbetalning
                    .then(|| view! { <input type="hidden" name="aterbetalning" value="1" /> })}
                <div class="falt-grupp">
                    <div>
                        <label for="betaldag">"Betaldag"</label>
                        <input
                            id="betaldag"
                            name="betaldag"
                            type="text"
                            inputmode="numeric"
                            placeholder="ÅÅÅÅ-MM-DD"
                            value=v.start.betaldag.clone()
                            on:input=move |ev| betalning.update(|b| b.betaldag = event_target_value(&ev))
                        />
                        {spärr_vid(Fält::Betaldag)}
                    </div>
                    <div>
                        <label for="belopp">"Belopp"</label>
                        <input
                            id="belopp"
                            name="belopp"
                            type="text"
                            inputmode="decimal"
                            value=v.start.belopp.clone()
                            on:input=move |ev| betalning.update(|b| b.belopp = event_target_value(&ev))
                        />
                        {spärr_vid(Fält::Belopp)}
                    </div>
                    <div>
                        <label for="konto">{till}</label>
                        <select
                            id="konto"
                            name="konto"
                            on:change=move |ev| betalning.update(|b| b.konto = event_target_value(&ev))
                        >
                            {konton}
                        </select>
                        {spärr_vid(Fält::Konto)}
                    </div>
                </div>
                {delbetalning}
                {öresutjämning}
                {spärr_vid(Fält::Öresutjämning)}
                {bokförs_som}
                <p class="knappar">
                    <button
                        class="knapp knapp-primar"
                        type="submit"
                        on:click=registrera
                        disabled=move || hydrerad.get() && !spärrar.with(Vec::is_empty)
                    >
                        {knapp}
                    </button>
                </p>
            </form>
        </section>
    }
}

/// Panelen Rätta betalning: rättelsen som vänder betalningens verifikat, datum och orsak.
#[component]
pub fn RättaBetalningspanel(
    action: String, stäng: String, r: RättaBetalningVy
) -> impl IntoView {
    let fel = r
        .fel
        .clone()
        .map(|t| view! { <p class="fel" role="alert">{t}</p> });
    view! {
        <section class="panel rattelse" aria-labelledby="ratta-betalning-rubrik">
            <header class="panelhuvud">
                <h3 id="ratta-betalning-rubrik">"Rätta betalning"</h3>
                <a class="knapp knapp-diskret" href=stäng>
                    "Avbryt"
                </a>
            </header>
            <p>
                {format!(
                    "Betalningen {} kr den {} ({}) vänds med en rättelse och tas bort ur \
                     reskontran, så att det som återstår blir som före betalningen. Betalningen \
                     kan sedan registreras om.",
                    r.belopp,
                    r.betaldag,
                    r.verifikat,
                )}
            </p>
            <div class="kont">
                <h4>"Rättelsen bokförs som"</h4>
                {kontering(&r.rader)}
            </div>
            {fel}
            <form method="post" action=action>
                <div class="falt-grupp">
                    <div>
                        <label for="datum">"Datum"</label>
                        <input
                            id="datum"
                            name="datum"
                            type="text"
                            inputmode="numeric"
                            placeholder="ÅÅÅÅ-MM-DD"
                            value=r.datum.clone()
                        />
                    </div>
                    <div>
                        <label for="orsak">"Orsak (valfri)"</label>
                        <input
                            id="orsak"
                            name="orsak"
                            type="text"
                            placeholder="t.ex. fel belopp"
                            value=r.orsak.clone()
                        />
                    </div>
                </div>
                <p class="knappar">
                    <button class="knapp knapp-primar" type="submit">
                        {format!("Rätta {}", r.verifikat)}
                    </button>
                </p>
            </form>
        </section>
    }
}

/// Betalningen ur fälten i ett inskickat formulär eller i adressen.
pub fn ur_fält(värde: impl Fn(&str) -> Option<String>) -> NyBetalning {
    NyBetalning {
        betaldag: värde("betaldag").unwrap_or_default(),
        belopp: värde("belopp").unwrap_or_default(),
        konto: värde("konto").unwrap_or_default(),
        öresutjämning: värde("oresutjamning").is_some_and(|v| !v.is_empty()),
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::{Extension, Form};
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::betalning::{self, BetalningFel, BetalningsId, Faktura};
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::kundfaktura::KundfakturaId;
    use maya_core::{kontoplan, verifikat};
    use maya_domain::betalning::NyBetalning;
    use maya_domain::reskontra::märke_med_kredit;

    use super::*;
    use crate::kundfakturor::server::KundfakturaState;
    use crate::sidodata::Sidbegäran;
    use crate::svar::{ingen_åtkomst, serverfel};
    use crate::verifikat::server::tid;

    /// Fakturan i reskontran för dokumentet, med panelen som `?panel=` öppnar.
    pub(crate) async fn reskontra(
        b: &Sidbegäran,
        företag: FöretagsId,
        faktura: Faktura,
        panel: Option<&str>,
    ) -> Result<ReskontraVy, BetalningFel> {
        let aktör = &b.inloggad.aktör;
        let idag = b.klocka.idag();
        let post = betalning::reskontrapost(&b.db, aktör, företag, faktura).await?;
        let m = märke_med_kredit(
            post.total,
            post.krediterat,
            post.återstår,
            post.förfallodag,
            idag,
        );
        let mut vy = ReskontraVy {
            märke: (m.kod().into(), m.namn().into()),
            återstår: kr(post.återstår),
            återstår_öre: post.återstår,
            betalningar: post
                .betalningar
                .iter()
                .map(|p| BetalningsradVy {
                    id: p.id.to_string(),
                    betaldag: p.betaldag.to_string(),
                    belopp: kr(p.belopp),
                    öresutjämning: if p.öresutjämning == 0 {
                        String::new()
                    } else {
                        kr(p.öresutjämning)
                    },
                    konto: p.konto.to_string(),
                    verifikat: (
                        p.verifikat.id.to_string(),
                        p.verifikat
                            .nummer
                            .map(|n| n.to_string())
                            .unwrap_or_default(),
                    ),
                    registrerad: (tid(p.registrerad.tid), p.registrerad.namn.clone()),
                    rättad: p.rättad.as_ref().map(|r| BetalningsrättelseVy {
                        verifikat: (
                            r.verifikat.id.to_string(),
                            r.verifikat
                                .nummer
                                .map(|n| n.to_string())
                                .unwrap_or_default(),
                        ),
                        orsak: r.orsak.clone(),
                        rättad: (tid(r.rättad.tid), r.rättad.namn.clone()),
                    }),
                })
                .collect(),
            betalning: None,
            rätta: None,
        };
        let fel = b.värde("fel");
        match panel {
            // #140: Återbetala… är samma panel med ett tillgodohavande.
            Some(p @ ("betalning" | "aterbetala"))
                if (p == "betalning" && post.återstår > 0)
                    || (p == "aterbetala" && post.återstår < 0) =>
            {
                let f = betalning::förutsättningar(&b.db, aktör, företag, faktura).await?;
                let konton = kontoplan::konton(&b.db, aktör, företag).await?;
                let tillbaka = fel.as_deref() == Some("betalning");
                vy.betalning = Some(BetalningsVy {
                    reskontra: f.reskontra,
                    återstår: f.återstår,
                    förfallodag: post.förfallodag.to_string(),
                    konton: f.konton.iter().map(|(k, a)| (k.get(), *a)).collect(),
                    räkenskapsår: f
                        .räkenskapsår
                        .iter()
                        .map(|å| (å.start().to_string(), å.slut().to_string()))
                        .collect(),
                    betalkonton: konton
                        .iter()
                        .filter(|k| k.aktiv && k.nummer.klass() == 1)
                        .map(|k| (k.nummer.get(), k.namn.clone()))
                        .collect(),
                    kontonamn: konton
                        .iter()
                        .filter(|k| k.nummer.klass() == 1 || [2440, 3740].contains(&k.nummer.get()))
                        .map(|k| (k.nummer.get(), k.namn.clone()))
                        .collect(),
                    start: if tillbaka {
                        ur_fält(|namn| b.värde(namn))
                    } else {
                        NyBetalning::förvald(f.återstår, idag)
                    },
                    fel: tillbaka.then(|| feltext("betalning").unwrap_or_default().into()),
                });
            }
            Some("ratta-betalning") => {
                let vald = b.värde("b");
                if let Some(p) = post
                    .betalningar
                    .iter()
                    .find(|p| Some(p.id.to_string()) == vald && p.rättad.is_none())
                {
                    let konton = kontoplan::konton(&b.db, aktör, företag).await?;
                    let namn = |nr: u16| {
                        konton
                            .iter()
                            .find(|k| k.nummer.get() == nr)
                            .map(|k| k.namn.clone())
                            .unwrap_or_default()
                    };
                    let rader = verifikat::verifikatet(&b.db, aktör, företag, p.verifikat.id)
                        .await?
                        .map(|v| v.rader)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|r| {
                            let konto = r.konto?;
                            Some((
                                konto.to_string(),
                                namn(konto.get()),
                                r.kredit.eller_tomt(),
                                r.debet.eller_tomt(),
                            ))
                        })
                        .collect();
                    let tillbaka = fel.as_deref() == Some("ratta");
                    vy.rätta = Some(RättaBetalningVy {
                        id: p.id.to_string(),
                        betaldag: p.betaldag.to_string(),
                        belopp: kr(p.belopp + p.öresutjämning),
                        verifikat: p
                            .verifikat
                            .nummer
                            .map(|n| n.to_string())
                            .unwrap_or_default(),
                        rader,
                        datum: if tillbaka {
                            b.värde("datum").unwrap_or_default()
                        } else {
                            idag.to_string()
                        },
                        orsak: if tillbaka {
                            b.värde("orsak").unwrap_or_default()
                        } else {
                            String::new()
                        },
                        fel: tillbaka.then(|| {
                            b.värde("text")
                                .unwrap_or_else(|| feltext("ratta").unwrap_or_default().into())
                        }),
                    });
                }
            }
            _ => {}
        }
        Ok(vy)
    }

    type Fält = Vec<(String, String)>;

    fn värde(fält: &Fält, namn: &str) -> Option<String> {
        fält.iter().find(|(k, _)| k == namn).map(|(_, v)| v.clone())
    }

    fn tillbaka(sida: &str, fält: &Fält, extra: &[(&str, &str)]) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (namn, v) in extra {
            query.append_pair(namn, v);
        }
        for (namn, v) in fält {
            query.append_pair(namn, v);
        }
        Redirect::to(&format!("{sida}&{}", query.finish())).into_response()
    }

    fn tolka(
        id: &str,
        f: &str,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Result<(Inloggad, FöretagsId, KundfakturaId), Box<Response>> {
        let Some(Extension(inloggad)) = inloggad else {
            return Err(Box::new(Redirect::to("/").into_response()));
        };
        match (id.parse::<FöretagsId>(), f.parse::<KundfakturaId>()) {
            (Ok(företag), Ok(f)) => Ok((inloggad, företag, f)),
            _ => Err(Box::new(ingen_åtkomst())),
        }
    }

    fn betalningsfel(e: BetalningFel) -> Response {
        match e {
            BetalningFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            e => serverfel(e),
        }
    }

    /// `POST /foretag/{id}/kundfakturor/{f}/betalning`: registrerar betalningen och visar
    /// fakturan, eller panelen igen med spärrarna vid fälten.
    pub(crate) async fn registrera_kundbetalning(
        State(state): State<Arc<KundfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag, f) = match tolka(&id, &f, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let här = format!("/foretag/{företag}/kundfakturor?f={f}");
        let formulär = ur_fält(|namn| värde(&fält, namn));
        // #140: panelen Återbetala… skickar med att det är en återbetalning.
        let (panel, notis) = match värde(&fält, "aterbetalning").as_deref() {
            Some("1") => ("aterbetala", "aterbetald=1"),
            _ => ("betalning", "betald=1"),
        };
        match betalning::registrera(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            Faktura::Kund(f),
            &formulär,
        )
        .await
        {
            Ok(_) => Redirect::to(&format!("{här}&{notis}")).into_response(),
            Err(BetalningFel::Spärrat(_) | BetalningFel::Verifikat(_)) => {
                tillbaka(&här, &fält, &[("panel", panel), ("fel", "betalning")])
            }
            Err(BetalningFel::FinnsInte) => crate::kundfakturor::server::finns_inte(),
            Err(e) => betalningsfel(e),
        }
    }

    /// `POST /foretag/{id}/kundfakturor/{f}/betalningar/{b}/ratta`: rättar betalningen.
    pub(crate) async fn rätta_kundbetalning(
        State(state): State<Arc<KundfakturaState>>,
        Path((id, f, b)): Path<(String, String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag, f) = match tolka(&id, &f, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(b) = b.parse::<BetalningsId>() else {
            return crate::kundfakturor::server::finns_inte();
        };
        let här = format!("/foretag/{företag}/kundfakturor?f={f}");
        let datum = värde(&fält, "datum").unwrap_or_default();
        let orsak = värde(&fält, "orsak").unwrap_or_default();
        match betalning::rätta(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            b,
            &datum,
            &orsak,
        )
        .await
        {
            Ok(_) => Redirect::to(&format!("{här}&betalning_rattad=1")).into_response(),
            Err(BetalningFel::Verifikat(maya_core::verifikat::VerifikatFel::Spärrat(s))) => {
                let text = s
                    .iter()
                    .map(|s| s.text.clone())
                    .collect::<Vec<_>>()
                    .join(" ");
                let b = b.to_string();
                tillbaka(
                    &här,
                    &fält,
                    &[
                        ("panel", "ratta-betalning"),
                        ("b", &b),
                        ("fel", "ratta"),
                        ("text", &text),
                    ],
                )
            }
            Err(BetalningFel::RedanRättad) => {
                Redirect::to(&format!("{här}&fel=redan_rattad")).into_response()
            }
            Err(BetalningFel::IngenBetalning) => crate::kundfakturor::server::finns_inte(),
            Err(e) => betalningsfel(e),
        }
    }
}
