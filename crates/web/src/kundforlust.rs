//! Kundförlust och återkallelse i kundfakturans dokument (#141): panelen Konstaterad
//! kundförlust med skäl, datum och "Bokförs som", panelen Återkalla kundförlust, och
//! förlusterna i dokumentets uppgifter. Spärrarna prövas med `maya_domain::kundforlust` i
//! `core` och visas vid fälten.

use leptos::prelude::*;
use maya_domain::kundforlust::{NyKundförlust, Skäl};
use serde::{Deserialize, Serialize};

use crate::betalning::KonteringVy;

/// Ett verifikats id och nummer.
pub type VerifikatVy = (String, String);

/// En kundförlust på fakturan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KundförlustpostVy {
    pub id: String,
    pub datum: String,
    pub skälet: String,
    pub belopp: String,
    /// Förlustens verifikat: id och nummer, eller `None` när förlusten bara är en status.
    pub verifikat: Option<VerifikatVy>,
    /// Återkallelsens verifikat (om något), tid och namn, när förlusten är återkallad.
    pub återkallad: Option<(Option<VerifikatVy>, String, String)>,
}

/// Panelen Konstaterad kundförlust.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KundförlustpanelVy {
    /// Det som skrivs bort.
    pub återstår: String,
    /// Förlustens rader, eller tomt när fordran inte är bokförd.
    pub rader: Vec<KonteringVy>,
    /// Det som panelen visar när den öppnas, eller det som användaren skickade.
    pub start: NyKundförlust,
    /// Spärrarna från servern: fältets namn (`skal` eller `datum`, annars tomt), kod och text.
    pub spärrar: Vec<(String, String, String)>,
}

/// Panelen Återkalla kundförlust.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ÅterkallaVy {
    pub id: String,
    pub post: KundförlustpostVy,
    /// Återkallelsens rader: förlustens verifikat vänt.
    pub rader: Vec<KonteringVy>,
    pub idag: String,
}

/// Fakturans kundförluster som dokumentet visar dem.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KundförlustVy {
    pub poster: Vec<KundförlustpostVy>,
    /// Id:t för förlusten som gäller fakturan nu.
    pub gällande: Option<String>,
    /// Något återstår och ingen förlust gäller: Kundförlust… finns.
    pub kan_bokföras: bool,
    pub panel: Option<KundförlustpanelVy>,
    pub återkalla: Option<ÅterkallaVy>,
}

/// Felkoderna i adressen för kundförluster och deras texter.
pub fn feltext(kod: &str) -> Option<&'static str> {
    Some(match kod {
        "kundforlust" => "Kundförlusten kunde inte bokföras. Rätta det som är markerat.",
        "redan_aterkallad" => "Kundförlusten är redan återkallad.",
        // #151
        "bokfort_utifran" => {
            "En kredit har sedan bokförts på 1510 utifrån kundförlusten, så den kan inte \
             återkallas här."
        }
        "fore_forlusten" => {
            "Kundförlusten är daterad senare än i dag och kan återkallas först från den dagen."
        }
        "aterkalla" => {
            "Kundförlusten kan inte återkallas i dag: återkallelsen dateras i dag, och det \
             datumet måste gå att bokföra på."
        }
        _ => return None,
    })
}

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

/// Panelen Konstaterad kundförlust: skälet, datumet och förlustens verifikat innan det bokförs.
#[component]
pub fn Kundförlustpanel(action: String, stäng: String, p: KundförlustpanelVy) -> impl IntoView {
    let spärr_vid = |fält: &str| {
        p.spärrar
            .iter()
            .filter(|(f, _, _)| f == fält)
            .map(|(_, kod, text)| {
                view! {
                    <p class="fel" role="alert" data-kod=kod.clone()>
                        {text.clone()}
                    </p>
                }
            })
            .collect_view()
    };
    let skälen = Skäl::ALLA
        .iter()
        .map(|s| {
            view! {
                <label class="kryss">
                    <input
                        type="radio"
                        name="skal"
                        value=s.kod()
                        checked=p.start.skäl == s.kod()
                    />
                    {format!(" {}", s.namn())}
                </label>
            }
        })
        .collect_view();
    let bokförs_som = if p.rader.is_empty() {
        view! {
            <p class="diskret">
                "Fordran är inte bokförd, så förlusten blir bara en status på fakturan."
            </p>
        }
        .into_any()
    } else {
        view! {
            <h4>{format!("Bokförs som verifikat · {}", p.start.datum)}</h4>
            {kontering(&p.rader)}
        }
        .into_any()
    };
    view! {
        <section class="panel kundforlust" aria-labelledby="kundforlust-rubrik">
            <header class="panelhuvud">
                <h3 id="kundforlust-rubrik">"Konstaterad kundförlust"</h3>
                <a class="knapp knapp-diskret" href=stäng>
                    "Avbryt"
                </a>
            </header>
            <p class="diskret">
                {format!(
                    "Bara en konstaterad förlust hanteras här. {} kr skrivs bort och utgående \
                     moms minskas. En befarad kundförlust bokförs med ett manuellt verifikat.",
                    p.återstår,
                )}
            </p>
            {spärr_vid("")}
            <form method="post" action=action novalidate>
                <fieldset class="skal">
                    <legend>"Skäl"</legend>
                    {skälen}
                    <label for="annat">"Beskriv skälet (vid annat)"</label>
                    <input id="annat" name="annat" type="text" value=p.start.annat.clone() />
                    {spärr_vid("skal")}
                </fieldset>
                <div class="falt-grupp">
                    <div>
                        <label for="datum">"Datum"</label>
                        <input
                            id="datum"
                            name="datum"
                            type="text"
                            inputmode="numeric"
                            placeholder="ÅÅÅÅ-MM-DD"
                            value=p.start.datum.clone()
                        />
                        {spärr_vid("datum")}
                    </div>
                </div>
                <div class="kont">{bokförs_som}</div>
                <p class="knappar">
                    <button class="knapp knapp-fara" type="submit">
                        {if p.rader.is_empty() {
                            "Markera som kundförlust"
                        } else {
                            "Bokför kundförlust"
                        }}
                    </button>
                </p>
                <p class="diskret">"Kundförlusten kan återkallas senare med ett nytt verifikat."</p>
            </form>
        </section>
    }
}

/// Panelen Återkalla kundförlust: verifikatet som vänder förlustens, daterat i dag.
#[component]
pub fn KundförlustÅterkallelsepanel(
    action: String,
    stäng: String,
    vy: ÅterkallaVy,
) -> impl IntoView {
    let å = vy;
    let verifikat = å
        .post
        .verifikat
        .as_ref()
        .map(|(_, nr)| format!(" ({nr})"))
        .unwrap_or_default();
    let bokförs_som = (!å.rader.is_empty()).then(|| {
        view! {
            <div class="kont">
                <h4>{format!("Bokförs som verifikat · {}", å.idag)}</h4>
                {kontering(&å.rader)}
            </div>
        }
    });
    view! {
        <section class="panel rattelse" aria-labelledby="aterkalla-rubrik">
            <header class="panelhuvud">
                <h3 id="aterkalla-rubrik">"Återkalla kundförlust"</h3>
                <a class="knapp knapp-diskret" href=stäng>
                    "Avbryt"
                </a>
            </header>
            <p>
                {format!(
                    "Kundförlusten {} kr den {}{verifikat} vänds med ett nytt verifikat daterat \
                     i dag, och fakturan kan betalas igen.",
                    å.post.belopp,
                    å.post.datum,
                )}
            </p>
            {bokförs_som}
            <form method="post" action=action>
                <p class="knappar">
                    <button class="knapp knapp-primar" type="submit">
                        "Återkalla kundförlust"
                    </button>
                </p>
            </form>
        </section>
    }
}

/// Förlusterna i dokumentets uppgifter: datum, skäl, belopp och verifikat, och om förlusten är
/// återkallad.
#[component]
pub fn Kundförlusterna(bas: String, poster: Vec<KundförlustpostVy>) -> impl IntoView {
    poster
        .into_iter()
        .map(|p| {
            let verifikat = p.verifikat.clone().map(|(id, nr)| {
                view! {
                    " · "
                    <a href=format!("{bas}/verifikat?v={id}")>{nr}</a>
                }
            });
            let återkallad = p.återkallad.clone().map(|(v, tid, namn)| {
                let v = v.map(|(id, nr)| {
                    view! {
                        " med "
                        <a href=format!("{bas}/verifikat?v={id}")>{nr}</a>
                    }
                });
                view! {
                    <span class="diskret">" (återkallad" {v} " · " {tid} " · " {namn} ")"</span>
                }
            });
            view! {
                <dt>"Kundförlust"</dt>
                <dd>
                    {format!("{} · {} · {} kr", p.datum, p.skälet, p.belopp)}
                    {verifikat}
                    {återkallad}
                </dd>
            }
        })
        .collect_view()
}

/// Kundförlusten ur fälten i ett inskickat formulär eller i adressen.
pub fn ur_fält(värde: impl Fn(&str) -> Option<String>) -> NyKundförlust {
    NyKundförlust {
        skäl: värde("skal").unwrap_or_default(),
        annat: värde("annat").unwrap_or_default(),
        datum: värde("datum").unwrap_or_default(),
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::{Extension, Form};
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::kundfaktura::KundfakturaId;
    use maya_core::kundforlust::{self, KundförlustFel, KundförlustId, Kundförlustpost};
    use maya_core::{kontoplan, verifikat};
    use maya_domain::kundforlust::Fält;
    use maya_domain::verifikat::Rad;

    use maya_domain::belopp::Saldo;

    use super::*;
    use crate::kundfakturor::server::KundfakturaState;
    use crate::sidodata::Sidbegäran;
    use crate::svar::{ingen_åtkomst, serverfel};
    use crate::verifikat::server::tid;

    /// Ett belopp i öre som det visas.
    fn kr(öre: i64) -> String {
        Saldo::från_öre(öre).to_string()
    }

    fn länk(l: &verifikat::Länk) -> (String, String) {
        (
            l.id.to_string(),
            l.nummer.map(|n| n.to_string()).unwrap_or_default(),
        )
    }

    fn postvy(p: &Kundförlustpost) -> KundförlustpostVy {
        KundförlustpostVy {
            id: p.id.to_string(),
            datum: p.datum.to_string(),
            skälet: p.skälet.clone(),
            belopp: kr(p.belopp),
            verifikat: p.verifikat.as_ref().map(länk),
            återkallad: p.återkallad.as_ref().map(|å| {
                (
                    å.verifikat.as_ref().map(länk),
                    tid(å.återkallad.tid),
                    å.återkallad.namn.clone(),
                )
            }),
        }
    }

    /// Fakturans kundförluster för dokumentet, med panelen som `?panel=` öppnar.
    pub(crate) async fn kundförlusterna(
        b: &Sidbegäran,
        företag: FöretagsId,
        faktura: KundfakturaId,
        panel: Option<&str>,
    ) -> Result<KundförlustVy, KundförlustFel> {
        let aktör = &b.inloggad.aktör;
        let poster = kundforlust::kundförluster(&b.db, aktör, företag, faktura).await?;
        let f = kundforlust::förutsättningar(&b.db, aktör, företag, faktura).await?;
        let gällande = poster.iter().find(|p| p.återkallad.is_none());
        let mut vy = KundförlustVy {
            poster: poster.iter().map(postvy).collect(),
            gällande: gällande.map(|p| p.id.to_string()),
            kan_bokföras: f.återstår > 0 && gällande.is_none(),
            panel: None,
            återkalla: None,
        };
        let idag = b.klocka.idag();
        let konton = kontoplan::konton(&b.db, aktör, företag).await?;
        let vy_av = |rader: &[Rad]| -> Vec<KonteringVy> {
            rader
                .iter()
                .map(|r| {
                    (
                        r.konto.to_string(),
                        konton
                            .iter()
                            .find(|k| k.nummer == r.konto)
                            .map(|k| k.namn.clone())
                            .unwrap_or_default(),
                        r.debet.eller_tomt(),
                        r.kredit.eller_tomt(),
                    )
                })
                .collect()
        };
        match panel {
            Some("kundforlust") if vy.kan_bokföras => {
                let tillbaka = b.värde("fel").as_deref() == Some("kundforlust");
                let start = if tillbaka {
                    ur_fält(|namn| b.värde(namn))
                } else {
                    NyKundförlust::förvald(idag)
                };
                let spärrar = if tillbaka {
                    start
                        .kontrollera(&f)
                        .err()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| {
                            let fält = match s.fält {
                                Fält::Fakturan => "",
                                Fält::Skäl => "skal",
                                Fält::Datum => "datum",
                            };
                            (fält.to_string(), s.kod.to_string(), s.text)
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let rader = f
                    .utfärdandet
                    .as_deref()
                    .map(|u| maya_domain::kundforlust::kontering(f.återstår, u))
                    .unwrap_or_default();
                vy.panel = Some(KundförlustpanelVy {
                    återstår: kr(f.återstår),
                    rader: vy_av(&rader),
                    start,
                    spärrar,
                });
            }
            Some("aterkalla-kundforlust") => {
                if let Some(p) = gällande {
                    let rader = match &p.verifikat {
                        Some(l) => verifikat::verifikatet(&b.db, aktör, företag, l.id)
                            .await?
                            .map(|v| v.rader)
                            .unwrap_or_default()
                            .into_iter()
                            .filter_map(|r| {
                                Some(Rad {
                                    konto: r.konto?,
                                    debet: r.debet,
                                    kredit: r.kredit,
                                    text: r.text,
                                })
                            })
                            .collect(),
                        None => Vec::new(),
                    };
                    vy.återkalla = Some(ÅterkallaVy {
                        id: p.id.to_string(),
                        post: postvy(p),
                        rader: vy_av(&maya_domain::kundforlust::återkallelse(&rader)),
                        idag: idag.to_string(),
                    });
                }
            }
            _ => {}
        }
        Ok(vy)
    }

    type Fältlista = Vec<(String, String)>;

    fn värde(fält: &Fältlista, namn: &str) -> Option<String> {
        fält.iter().find(|(k, _)| k == namn).map(|(_, v)| v.clone())
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

    fn förlustfel(e: KundförlustFel) -> Response {
        match e {
            KundförlustFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            KundförlustFel::FinnsInte | KundförlustFel::IngenKundförlust => {
                crate::kundfakturor::server::finns_inte()
            }
            e => serverfel(e),
        }
    }

    /// `POST /foretag/{id}/kundfakturor/{f}/kundforlust`: bokför kundförlusten och visar
    /// fakturan, eller panelen igen med spärrarna vid fälten.
    pub(crate) async fn bokför(
        State(state): State<Arc<KundfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fältlista>,
    ) -> Response {
        let (inloggad, företag, f) = match tolka(&id, &f, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let här = format!("/foretag/{företag}/kundfakturor?f={f}");
        let formulär = ur_fält(|namn| värde(&fält, namn));
        match kundforlust::bokför(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            f,
            &formulär,
        )
        .await
        {
            Ok(_) => Redirect::to(&format!("{här}&kundforlust=1")).into_response(),
            Err(KundförlustFel::Spärrat(_) | KundförlustFel::Verifikat(_)) => {
                let mut query = url::form_urlencoded::Serializer::new(String::new());
                query.append_pair("panel", "kundforlust");
                query.append_pair("fel", "kundforlust");
                query.append_pair("skal", &formulär.skäl);
                query.append_pair("annat", &formulär.annat);
                query.append_pair("datum", &formulär.datum);
                Redirect::to(&format!("{här}&{}", query.finish())).into_response()
            }
            Err(e) => förlustfel(e),
        }
    }

    /// `POST /foretag/{id}/kundfakturor/{f}/kundforlust/{k}/aterkalla`: återkallar
    /// kundförlusten med ett nytt verifikat daterat i dag.
    pub(crate) async fn återkalla(
        State(state): State<Arc<KundfakturaState>>,
        Path((id, f, k)): Path<(String, String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let (inloggad, företag, f) = match tolka(&id, &f, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(k) = k.parse::<KundförlustId>() else {
            return crate::kundfakturor::server::finns_inte();
        };
        let här = format!("/foretag/{företag}/kundfakturor?f={f}");
        match kundforlust::återkalla(&state.db, &state.klocka, &inloggad.aktör, företag, k).await
        {
            Ok(_) => Redirect::to(&format!("{här}&kundforlust_aterkallad=1")).into_response(),
            Err(KundförlustFel::RedanÅterkallad) => {
                Redirect::to(&format!("{här}&fel=redan_aterkallad")).into_response()
            }
            Err(KundförlustFel::FöreFörlusten) => {
                Redirect::to(&format!("{här}&fel=fore_forlusten")).into_response()
            }
            Err(KundförlustFel::BokförtUtifrån) => {
                Redirect::to(&format!("{här}&fel=bokfort_utifran")).into_response()
            }
            Err(KundförlustFel::Verifikat(_)) => {
                Redirect::to(&format!("{här}&fel=aterkalla")).into_response()
            }
            Err(e) => förlustfel(e),
        }
    }
}
