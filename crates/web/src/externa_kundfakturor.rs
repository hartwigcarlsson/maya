//! Externa kundfakturor i webben (#143): Ny → Extern faktura laddar upp originalet och skapar
//! ett utkast, som skrivs av med originalet till vänster och fälten till höger, som en
//! leverantörsfaktura. En registrerad extern faktura visas i Kundfakturor som ett dokument med
//! originalet, konteringen, betalningarna och kundförlusten som Mayas egna, och handlingarna
//! Registrera kreditfaktura… och Rätta registrering…; en upplåst faktura skrivs av igen.
//!
//! Sidan för en ny extern faktura är `/foretag/<id>/kundfakturor/extern/ny`; en vald extern
//! faktura visas i `/foretag/<id>/kundfakturor?f=<faktura>`. Formuläret prövas med
//! `maya_domain::extern_kundfaktura` i webbläsaren och i `core`.

use jiff::civil::Date;
use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use maya_domain::belopp::Saldo;
use maya_domain::extern_kundfaktura::{
    Fält, Förutsättningar, Krediterar, NyExternFaktura, NyExternRad, Radfält, Spärr,
};
use maya_domain::foretag::Momsperiod;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kund::NyKund;
use maya_domain::kundfaktura::{Fakturakund, NY};
use maya_domain::rakenskapsar::Räkenskapsår;
use serde::{Deserialize, Serialize};

use crate::betalning::{Betalningar, Betalningspanel, Märke, ReskontraVy, RättaBetalningspanel};
use crate::kundfakturor::{NYA_KUNDFÄLT, ny_kunds};
use crate::kundforlust::KundförlustVy;
use crate::underlag::{Förhandsvisning, UnderlagVy};
use crate::verifikat::UtkastBadge;

fn kr(öre: i64) -> String {
    Saldo::från_öre(öre).to_string()
}

const MOMSSATSER: [u8; 4] = [25, 12, 6, 0];

/// Det som formuläret prövas mot, i webbläsaren med samma regler som på servern.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternFörutsättningarVy {
    pub kunder: Vec<Fakturakund>,
    pub konton: Vec<(u16, bool)>,
    /// Aktiva konton i klass 3 med namn, för kontoväljaren.
    pub intäktskonton: Vec<(u16, String)>,
    pub räkenskapsår: Vec<(String, String)>,
    pub momsperioder: Vec<(String, String)>,
    pub upptagna: Vec<String>,
    pub krediterar: Option<Krediterar>,
    pub betalt: i64,
    /// Bokföringsmetodens historik: från vilket datum och metodens kod (#149).
    #[serde(default)]
    pub bokföringsmetoder: Vec<(String, String)>,
}

impl ExternFörutsättningarVy {
    fn domän(&self) -> Förutsättningar {
        let datum = |d: &str| d.parse::<Date>().ok();
        Förutsättningar {
            kunder: self.kunder.clone(),
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
            momsperioder: self
                .momsperioder
                .iter()
                .filter_map(|(d, kod)| Some((datum(d)?, Momsperiod::från_kod(kod)?)))
                .collect(),
            upptagna: self.upptagna.clone(),
            krediterar: self.krediterar.clone(),
            betalt: self.betalt,
        }
    }
}

/// En rad i konteringen: konto, kontonamn, debet och kredit.
pub type KonteringVy = (String, String, String, String);

/// En extern kundfaktura: ett utkast eller en upplåst faktura med formuläret, eller en
/// registrerad faktura som dokument.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternVy {
    pub id: String,
    /// Fakturans registrering är vänd med Rätta registrering, och den skrivs av igen.
    pub upplåst: bool,
    pub original: UnderlagVy,
    pub formulär: NyExternFaktura,
    pub kund: String,
    /// Registreringens verifikat: id och nummer.
    pub verifikat: Option<(String, String)>,
    /// Tid och namn.
    pub skapad: (String, String),
    pub registrerad: Option<(String, String)>,
    pub total: String,
    /// Radernas moms per sats, "Moms 25 % på 8 000,00" och momsen, för dokumentet.
    pub momsrader: Vec<(String, String)>,
    /// Registreringens rader: momssats, konto och belopp.
    pub rader: Vec<(String, String, String)>,
    pub kontering: Vec<KonteringVy>,
    /// Originalets id och hänvisningens text när fakturan är en kreditfaktura.
    pub krediterar: Option<(String, String)>,
    /// Kreditfakturorna som krediterar fakturan: id, nummer och total.
    pub krediteras_av: Vec<(String, String, String)>,
    /// Upplåsningarna: registreringens verifikat, rättelsen, orsaken, tid och namn.
    pub rättelser: Vec<(String, String, String, String, String)>,
    /// Tid, text och namn.
    pub historik: Vec<(String, String, String)>,
    pub reskontra: Option<ReskontraVy>,
    pub kundförlust: Option<KundförlustVy>,
    /// För ett utkast och en upplåst faktura.
    pub förutsättningar: Option<ExternFörutsättningarVy>,
}

/// Felkoderna i adressen och deras texter.
pub fn feltext(kod: &str) -> Option<&'static str> {
    Some(match kod {
        "extern_utkast" => "Utkastet kunde inte sparas. Rätta det som är markerat.",
        "extern_sparrat" => "Fakturan kan inte registreras än. Rätta det som är markerat.",
        "extern_finns_inte" => "Utkastet finns inte längre, eller är redan registrerat.",
        "extern_ingen_fil" => "Välj fakturans PDF att ladda upp.",
        "extern_for_stort" => "Ett underlag får vara högst 25 MB.",
        "har_kundforlust" => {
            "En kundförlust gäller fakturan. Återkalla den innan registreringen rättas."
        }
        "har_kreditfakturor" => {
            "Kreditfakturor krediterar fakturan. Registreringen kan inte rättas medan de finns."
        }
        // #151
        "i_t2" => {
            "Fakturan är med i det bokförda T2-verifikatet, som inte vänds. Registreringen kan inte \
             rättas."
        }
        "extern_rattelse" => {
            "Registreringen kan inte rättas i dag: rättelsen dateras i dag, och det datumet måste \
             gå att bokföra på och vara tidigast registreringens datum."
        }
        _ => return None,
    })
}

/// Formuläret ur fälten i ett inskickat formulär eller i adressen.
pub(crate) fn ur_fält(värde: impl Fn(&str) -> Option<String>) -> NyExternFaktura {
    let fält = |namn: &str| värde(namn).unwrap_or_default();
    let antal = (0..500)
        .take_while(|i| värde(&format!("belopp_{i}")).is_some())
        .count();
    // #153 Den nya kunden i det streckade kortet.
    let mut ny_kund = NyKund::default();
    for (namn, _, _) in NYA_KUNDFÄLT {
        if let Some(v) = värde(namn) {
            *ny_kunds(&mut ny_kund, namn) = v;
        }
    }
    NyExternFaktura {
        kund: fält("kund"),
        ny_kund,
        fakturanummer: fält("fakturanummer"),
        fakturadatum: fält("fakturadatum"),
        förfallodag: fält("forfallodag"),
        rader: (0..antal)
            .map(|i| NyExternRad {
                momssats: fält(&format!("momssats_{i}")),
                konto: fält(&format!("konto_{i}")),
                belopp: fält(&format!("belopp_{i}")),
            })
            .collect(),
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{DefaultBodyLimit, Path, Request, State};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::betalning::{BetalningFel, Faktura};
    use maya_core::extern_kundfaktura::{self, ExternFel, ExternKundfaktura};
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::kundfaktura::{KundfakturaId, Status};
    use maya_core::underlag::{self, UnderlagFel};
    use maya_core::{Databas, Klocka, kontoplan, verifikat};
    use maya_domain::reskontra::Märke;

    use super::*;
    use crate::sidodata::Sidbegäran;
    use crate::svar::{ingen_åtkomst, serverfel, text};
    use crate::underlag::server::{STÖRSTA_BEGÄRAN, läs_formulär, underlagsfel, vy};
    use crate::verifikat::server::tid;

    #[derive(Clone)]
    struct ExternState {
        db: Databas,
        klocka: Klocka,
    }

    pub(crate) fn routes(db: Databas, klocka: Klocka) -> Router {
        Router::new()
            .route("/foretag/{id}/kundfakturor/extern", post(ladda_upp))
            .route("/foretag/{id}/kundfakturor/extern/{f}", post(skicka))
            .route("/foretag/{id}/kundfakturor/extern/{f}/radera", post(radera))
            .route(
                "/foretag/{id}/kundfakturor/extern/{f}/kreditera",
                post(kreditera),
            )
            .route(
                "/foretag/{id}/kundfakturor/extern/{f}/ratta-registrering",
                post(rätta),
            )
            .layer(DefaultBodyLimit::max(STÖRSTA_BEGÄRAN))
            .with_state(Arc::new(ExternState { db, klocka }))
    }

    /// Det som Kundfakturor visar för den externa fakturan `f`.
    pub(crate) async fn extern_vy(
        b: &Sidbegäran,
        företag: FöretagsId,
        f: &ExternKundfaktura,
    ) -> Result<ExternVy, ÅtkomstFel> {
        let (db, aktör) = (&b.db, &b.inloggad.aktör);
        let original = underlag::underlaget(db, aktör, företag, f.underlag)
            .await?
            .map(|u| vy(&u))
            .unwrap_or_default();
        let konton = kontoplan::konton(db, aktör, företag).await?;
        let kontonamn = |nr: u16| {
            konton
                .iter()
                .find(|k| k.nummer.get() == nr)
                .map(|k| k.namn.clone())
                .unwrap_or_default()
        };
        let kontering = match (&f.verifikat, f.status) {
            (Some(l), Status::Registrerad) => verifikat::verifikatet(db, aktör, företag, l.id)
                .await?
                .map(|v| {
                    v.rader
                        .iter()
                        .map(|r| {
                            let konto = r.konto.map(|k| k.get()).unwrap_or_default();
                            (
                                konto.to_string(),
                                kontonamn(konto),
                                r.debet.eller_tomt(),
                                r.kredit.eller_tomt(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let beräkning = f.formulär.beräkna();
        let panel = b.värde("panel");
        // Betalningarna och det som återstår, som på Mayas egna; en kreditfaktura räknas av mot
        // originalet och har ingen egen post.
        let reskontra = match (f.status, &f.krediterar) {
            (Status::Registrerad | Status::Upplåst, None) => {
                let mut r = crate::betalning::server::reskontra(
                    b,
                    företag,
                    Faktura::Kund(f.id),
                    panel.as_deref().filter(|_| f.status == Status::Registrerad),
                )
                .await
                .map_err(|e| match e {
                    BetalningFel::Åtkomst(e) => e,
                    e => ÅtkomstFel::Databas(maya_core::DatabasFel::Korrupt(e.to_string())),
                })?;
                if f.status == Status::Upplåst {
                    r.märke = (Märke::Upplåst.kod().into(), Märke::Upplåst.namn().into());
                }
                Some(r)
            }
            _ => None,
        };
        let kundförlust = match (f.status, &f.krediterar) {
            (Status::Registrerad, None) => Some(
                crate::kundforlust::server::kundförlusterna(b, företag, f.id, panel.as_deref())
                    .await
                    .map_err(|e| match e {
                        maya_core::kundforlust::KundförlustFel::Åtkomst(e) => e,
                        e => ÅtkomstFel::Databas(maya_core::DatabasFel::Korrupt(e.to_string())),
                    })?,
            ),
            _ => None,
        };
        let förutsättningar = match f.status {
            Status::Utkast | Status::Upplåst => {
                let fö =
                    extern_kundfaktura::förutsättningar(db, aktör, företag, Some(f.id)).await?;
                Some(ExternFörutsättningarVy {
                    kunder: fö.kunder.clone(),
                    konton: fö.konton.iter().map(|(k, a)| (k.get(), *a)).collect(),
                    intäktskonton: konton
                        .iter()
                        .filter(|k| k.aktiv && k.nummer.klass() == 3)
                        .map(|k| (k.nummer.get(), k.namn.clone()))
                        .collect(),
                    räkenskapsår: fö
                        .räkenskapsår
                        .iter()
                        .map(|å| (å.start().to_string(), å.slut().to_string()))
                        .collect(),
                    momsperioder: fö
                        .momsperioder
                        .iter()
                        .map(|(d, p)| (d.to_string(), p.kod().to_string()))
                        .collect(),
                    upptagna: fö.upptagna.clone(),
                    krediterar: fö.krediterar.clone(),
                    betalt: fö.betalt,
                    bokföringsmetoder: maya_core::foretag::företaget(db, aktör, företag)
                        .await?
                        .bokföringsmetoder
                        .iter()
                        .map(|(d, m)| (d.to_string(), m.kod().to_string()))
                        .collect(),
                })
            }
            _ => None,
        };
        let nummer = |l: &Option<verifikat::Länk>| {
            l.as_ref()
                .and_then(|l| l.nummer)
                .map(|n| n.to_string())
                .unwrap_or_default()
        };
        Ok(ExternVy {
            id: f.id.to_string(),
            upplåst: f.status == Status::Upplåst,
            original,
            formulär: f.formulär.clone(),
            kund: f.kund.clone(),
            verifikat: f.verifikat.as_ref().map(|l| {
                (
                    l.id.to_string(),
                    l.nummer.map(|n| n.to_string()).unwrap_or_default(),
                )
            }),
            skapad: (tid(f.skapad.tid), f.skapad.namn.clone()),
            registrerad: f.registrerad.as_ref().map(|s| (tid(s.tid), s.namn.clone())),
            total: kr(f.total.unwrap_or(beräkning.total)),
            momsrader: beräkning
                .momsrader
                .iter()
                .filter(|m| m.moms != 0)
                .map(|m| (format!("Moms {} på {}", m.sats, kr(m.underlag)), kr(m.moms)))
                .collect(),
            rader: f
                .formulär
                .rader
                .iter()
                .map(|r| {
                    (
                        format!("{} %", r.momssats),
                        r.konto.clone(),
                        maya_domain::belopp::tolka_med_tecken(&r.belopp)
                            .map(kr)
                            .unwrap_or_else(|| r.belopp.clone()),
                    )
                })
                .collect(),
            kontering,
            krediterar: f
                .krediterar
                .as_ref()
                .map(|k| (k.faktura.to_string(), k.text())),
            krediteras_av: f
                .krediteras_av
                .iter()
                .map(|k| (k.id.to_string(), k.fakturanummer.clone(), kr(k.total)))
                .collect(),
            rättelser: f
                .rättelser
                .iter()
                .map(|r| {
                    (
                        nummer(&r.verifikat),
                        nummer(&r.rättelse),
                        r.orsak.clone(),
                        tid(r.rättad.tid),
                        r.rättad.namn.clone(),
                    )
                })
                .collect(),
            historik: f
                .historik
                .iter()
                .map(|h| (tid(h.post.tid), h.post.text(), h.namn.clone()))
                .collect(),
            reskontra,
            kundförlust,
            förutsättningar,
        })
    }

    type Fält = Vec<(String, String)>;

    fn värde(fält: &Fält, namn: &str) -> Option<String> {
        fält.iter().find(|(k, _)| k == namn).map(|(_, v)| v.clone())
    }

    fn lista(företag: FöretagsId) -> String {
        format!("/foretag/{företag}/kundfakturor")
    }

    /// Tillbaka till fakturan med det användaren skrev och felkoden.
    fn tillbaka(dokumentet: &str, fält: &Fält, fel: &str) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("tillbaka", "1");
        query.append_pair("fel", fel);
        for (namn, v) in fält.iter().filter(|(namn, _)| namn != "handling") {
            query.append_pair(namn, v);
        }
        Redirect::to(&format!("{dokumentet}&{}", query.finish())).into_response()
    }

    fn tolka(
        id: &str,
        f: Option<&str>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Result<(Inloggad, FöretagsId, Option<KundfakturaId>), Box<Response>> {
        let Some(Extension(inloggad)) = inloggad else {
            return Err(Box::new(Redirect::to("/").into_response()));
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Err(Box::new(ingen_åtkomst()));
        };
        let f = match f.map(str::parse::<KundfakturaId>) {
            Some(Ok(f)) => Some(f),
            Some(Err(_)) => return Err(Box::new(finns_inte())),
            None => None,
        };
        Ok((inloggad, företag, f))
    }

    fn finns_inte() -> Response {
        text(
            axum::http::StatusCode::NOT_FOUND,
            "Den externa fakturan finns inte.\n",
        )
    }

    fn fel(e: ExternFel, sida: &str) -> Response {
        match e {
            ExternFel::IngetUtkast => {
                Redirect::to(&format!("{sida}?fel=extern_finns_inte")).into_response()
            }
            ExternFel::FinnsInte | ExternFel::InteRegistrerad => finns_inte(),
            ExternFel::Underlag(UnderlagFel::FörStort) => {
                Redirect::to(&format!("{sida}/extern/ny?fel=extern_for_stort")).into_response()
            }
            ExternFel::Underlag(e) => underlagsfel(e),
            ExternFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            e => serverfel(e),
        }
    }

    /// `POST /foretag/{id}/kundfakturor/extern`: laddar upp originalet och skapar utkastet.
    async fn ladda_upp(
        State(state): State<Arc<ExternState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        req: Request,
    ) -> Response {
        let (inloggad, företag, _) = match tolka(&id, None, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let sida = lista(företag);
        let formulär = match läs_formulär(req).await {
            Ok(f) => f,
            Err(r) => return *r,
        };
        if formulär.för_stort {
            return Redirect::to(&format!("{sida}/extern/ny?fel=extern_for_stort")).into_response();
        }
        let Some(fil) = formulär.fil else {
            return Redirect::to(&format!("{sida}/extern/ny?fel=extern_ingen_fil")).into_response();
        };
        match extern_kundfaktura::ladda_upp(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            (&fil.filnamn, &fil.mime, &fil.innehåll),
        )
        .await
        {
            Ok((f, _)) => Redirect::to(&format!("{sida}?f={f}&uppladdat=1")).into_response(),
            Err(e) => fel(e, &sida),
        }
    }

    /// `POST /foretag/{id}/kundfakturor/extern/{f}`: Spara utkast, eller Registrera och bokför.
    async fn skicka(
        State(state): State<Arc<ExternState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag, Some(f)) = (match tolka(&id, Some(&f), inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        }) else {
            return finns_inte();
        };
        let sida = lista(företag);
        let dokumentet = format!("{sida}?f={f}");
        let formulär = ur_fält(|n| värde(&fält, n));
        let aktör = &inloggad.aktör;
        if värde(&fält, "handling").as_deref() == Some("registrera") {
            match extern_kundfaktura::registrera(
                &state.db,
                &state.klocka,
                aktör,
                företag,
                f,
                &formulär,
            )
            .await
            {
                // #149: under kontantmetoden bokförs registreringen inte.
                Ok(r) => Redirect::to(&format!(
                    "{dokumentet}&registrerad={}",
                    r.verifikat
                        .map_or_else(|| "0".to_string(), |v| v.nummer.to_string())
                ))
                .into_response(),
                Err(ExternFel::Spärrat(_)) => tillbaka(&dokumentet, &fält, "extern_sparrat"),
                Err(e) => fel(e, &sida),
            }
        } else {
            match extern_kundfaktura::spara_utkast(
                &state.db,
                &state.klocka,
                aktör,
                företag,
                f,
                &formulär,
            )
            .await
            {
                Ok(()) => Redirect::to(&format!("{dokumentet}&sparat=1")).into_response(),
                Err(ExternFel::Spärrat(_)) => tillbaka(&dokumentet, &fält, "extern_utkast"),
                Err(e) => fel(e, &sida),
            }
        }
    }

    async fn radera(
        State(state): State<Arc<ExternState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let (inloggad, företag, Some(f)) = (match tolka(&id, Some(&f), inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        }) else {
            return finns_inte();
        };
        let sida = lista(företag);
        match extern_kundfaktura::radera_utkast(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            f,
        )
        .await
        {
            Ok(()) => Redirect::to(&format!("{sida}?raderad=1")).into_response(),
            Err(e) => fel(e, &sida),
        }
    }

    /// `POST …/extern/{f}/kreditera`: laddar upp kreditfakturans original och skapar utkastet
    /// med hänvisningen.
    async fn kreditera(
        State(state): State<Arc<ExternState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        req: Request,
    ) -> Response {
        let (inloggad, företag, Some(f)) = (match tolka(&id, Some(&f), inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        }) else {
            return finns_inte();
        };
        let sida = lista(företag);
        let formulär = match läs_formulär(req).await {
            Ok(f) => f,
            Err(r) => return *r,
        };
        let tillbaka = format!("{sida}?f={f}&panel=kreditera-extern");
        if formulär.för_stort {
            return Redirect::to(&format!("{tillbaka}&fel=extern_for_stort")).into_response();
        }
        let Some(fil) = formulär.fil else {
            return Redirect::to(&format!("{tillbaka}&fel=extern_ingen_fil")).into_response();
        };
        match extern_kundfaktura::kreditera(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            f,
            (&fil.filnamn, &fil.mime, &fil.innehåll),
        )
        .await
        {
            Ok(k) => Redirect::to(&format!("{sida}?f={k}&extern_kreditutkast=1")).into_response(),
            Err(e) => fel(e, &sida),
        }
    }

    /// `POST …/extern/{f}/ratta-registrering`: vänder registreringen och låser upp fakturan.
    async fn rätta(
        State(state): State<Arc<ExternState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag, Some(f)) = (match tolka(&id, Some(&f), inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        }) else {
            return finns_inte();
        };
        let sida = lista(företag);
        let dokumentet = format!("{sida}?f={f}");
        let orsak = värde(&fält, "orsak").unwrap_or_default();
        match extern_kundfaktura::rätta_registrering(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            f,
            &orsak,
        )
        .await
        {
            Ok(r) => Redirect::to(&format!(
                "{dokumentet}&registrering_rattad={}",
                r.rättelse
                    .map(|r| r.nummer.to_string())
                    .unwrap_or_else(|| "1".into())
            ))
            .into_response(),
            Err(ExternFel::HarKundförlust) => {
                Redirect::to(&format!("{dokumentet}&fel=har_kundforlust")).into_response()
            }
            Err(ExternFel::HarKreditfakturor) => {
                Redirect::to(&format!("{dokumentet}&fel=har_kreditfakturor")).into_response()
            }
            Err(ExternFel::IBokförtT2) => {
                Redirect::to(&format!("{dokumentet}&fel=i_t2")).into_response()
            }
            Err(ExternFel::Rättelse(_)) => {
                Redirect::to(&format!("{dokumentet}&fel=extern_rattelse")).into_response()
            }
            Err(e) => fel(e, &sida),
        }
    }
}

/// Ny → Extern faktura: ladda upp fakturans PDF från det andra systemet.
#[component]
pub fn NyExternFakturaFormulär(bas: String) -> impl IntoView {
    view! {
        <section class="formular">
            <h2>"Ny extern kundfaktura"</h2>
            <p class="diskret">
                "En kundfaktura som har skapats i ett annat system registreras med sin PDF som underlag. PDF:en arkiveras med SHA-256 och visas bredvid fälten medan du skriver av fakturan."
            </p>
            <form
                class="uppladdning"
                method="post"
                action=format!("{bas}/kundfakturor/extern")
                enctype="multipart/form-data"
            >
                <label class="slapp">
                    <span>"Släpp fakturans PDF här eller välj en fil"</span>
                    <input type="file" name="fil" required />
                </label>
                <button class="knapp knapp-primar" type="submit">
                    "Ladda upp"
                </button>
            </form>
        </section>
    }
}

/// En extern kundfaktura i Kundfakturor: formuläret för ett utkast eller en upplåst faktura,
/// annars dokumentet.
#[component]
pub fn ExternFaktura(bas: String, d: ExternVy) -> impl IntoView {
    match d.förutsättningar.clone() {
        Some(f) => view! { <Avskrift bas=bas d=d f=f /> }.into_any(),
        None => view! { <Dokument bas=bas d=d /> }.into_any(),
    }
}

/// Utkastet eller den upplåsta fakturan: originalet till vänster och formuläret till höger.
#[component]
fn Avskrift(bas: String, d: ExternVy, f: ExternFörutsättningarVy) -> impl IntoView {
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let mut start = if fråga("tillbaka").is_some() {
        ur_fält(|n| fråga(n))
    } else {
        d.formulär.clone()
    };
    let antal = start.rader.len().max(2);
    start.rader.resize(antal, NyExternRad::default());
    let lista = format!("{bas}/kundfakturor");
    let här = format!("{lista}?f={}", d.id);
    let upplåst = d.upplåst;
    let dialog = (fråga("radera").is_some() && !upplåst).then(|| {
        view! {
            <section class="dialog" role="alertdialog" aria-labelledby="radera-rubrik">
                <h2 id="radera-rubrik">"Radera utkastet?"</h2>
                <p class="diskret">
                    "Raderingen loggas. PDF:en blir ett okopplat underlag igen."
                </p>
                <form method="post" action=format!("{lista}/extern/{}/radera", d.id)>
                    <a class="knapp" href=här.clone()>
                        "Avbryt"
                    </a>
                    " "
                    <button class="knapp knapp-fara" type="submit">
                        "Radera utkast"
                    </button>
                </form>
            </section>
        }
    });
    let betalningar = d
        .reskontra
        .clone()
        .map(|r| view! { <Betalningar bas=bas.clone() här=här.clone() r=r /> });
    let rättelser = view! { <Rättelser bas=bas.clone() d=d.clone() /> };
    view! {
        <div class="med-underlag">
            <aside class="underlagsspalt" aria-label="Originalet">
                <Förhandsvisning bas=bas.clone() u=d.original.clone() />
            </aside>
            <div>
                {dialog}
                <Formulär bas=bas.clone() d=d.clone() start=start f=f />
                {rättelser}
                {betalningar}
            </div>
        </div>
    }
}

/// Formuläret med spärrarna vid fälten medan användaren skriver. Registrera och bokför är
/// inaktiv med antalet problem kvar; Spara utkast går alltid att trycka på.
#[component]
fn Formulär(
    bas: String,
    d: ExternVy,
    start: NyExternFaktura,
    f: ExternFörutsättningarVy,
) -> impl IntoView {
    let faktura = RwSignal::new(start.clone());
    let fö = f.domän();
    let spärrar = {
        let fö = fö.clone();
        Memo::new(move |_| faktura.with(|x| x.kontrollera(&fö).err().unwrap_or_default()))
    };
    let kontering = {
        let fö = fö.clone();
        Memo::new(move |_| {
            faktura.with(|x| {
                x.kontrollera(&fö)
                    .map(|r| r.kontering())
                    .unwrap_or_default()
            })
        })
    };
    let hydrerad = RwSignal::new(false);
    Effect::new(move |_| hydrerad.set(true));
    let registrera = move |ev: leptos::ev::MouseEvent| {
        if !spärrar.with_untracked(Vec::is_empty) {
            ev.prevent_default();
        }
    };
    let spärr_vid = move |fält: Fält| {
        move || {
            spärrar.with(|s| {
                s.iter()
                    .filter(|s| s.fält == fält)
                    .map(|s: &Spärr| {
                        view! {
                            <p class="fel" role="alert" data-kod=s.kod>
                                {s.text.clone()}
                            </p>
                        }
                    })
                    .collect_view()
            })
        }
    };
    // #149: under kontantmetoden på fakturadatumet heter knappen Registrera. En kreditfaktura
    // följer originalets fordran.
    let kontant = {
        let metoder = f.bokföringsmetoder.clone();
        let kredit = f.krediterar.is_some();
        Memo::new(move |_| {
            !kredit
                && faktura.with(|x| crate::betalning::kontantmetoden_på(&metoder, &x.fakturadatum))
        })
    };
    let registrera_text = move || {
        let knapp = if kontant.get() {
            "Registrera"
        } else {
            "Registrera och bokför"
        };
        match spärrar.with(Vec::len) {
            0 => knapp.to_string(),
            1 => format!("{knapp} · 1 problem kvar"),
            n => format!("{knapp} · {n} problem kvar"),
        }
    };
    let textfält = move |namn: &'static str,
                         etikett: &'static str,
                         typ: &'static str,
                         fält: Fält,
                         läs: fn(&NyExternFaktura) -> String,
                         sätt: fn(&mut NyExternFaktura, String)| {
        view! {
            <div>
                <label for=namn>{etikett}</label>
                <input
                    id=namn
                    name=namn
                    type=typ
                    prop:value=move || faktura.with(läs)
                    value=faktura.with_untracked(läs)
                    on:input=move |ev| faktura.update(|x| sätt(x, event_target_value(&ev)))
                />
                {spärr_vid(fält)}
            </div>
        }
    };
    let kredit = f.krediterar.clone();
    let kundval = match &kredit {
        Some(k) => view! {
            <div class="bred">
                <label>"Kund"</label>
                <p>{k.kund.namn.clone()}</p>
                <p class="diskret">{k.text()}</p>
            </div>
        }
        .into_any(),
        None => {
            let vald = start.kund.clone();
            let ny_vald = vald.trim() == NY;
            let val = f
                .kunder
                .iter()
                .map(|k| {
                    let id = k.id.to_string();
                    view! {
                        <option value=id.clone() selected=id == vald>
                            {k.namn.clone()}
                        </option>
                    }
                })
                .collect_view();
            view! {
                <div class="bred">
                    <label for="kund">"Kund"</label>
                    <select
                        id="kund"
                        name="kund"
                        on:change=move |ev| faktura.update(|x| x.kund = event_target_value(&ev))
                    >
                        <option value="" selected=start.kund.is_empty()>
                            "Välj kund…"
                        </option>
                        {val}
                        <option value=NY selected=ny_vald>
                            "+ Ny kund…"
                        </option>
                    </select>
                    {spärr_vid(Fält::Kund)}
                </div>
            }
            .into_any()
        }
    };
    // #153 "+ Ny kund…": ett streckat kort med kundregistrets fält, som på Mayas egna utkast
    // (#137). Kunden läggs i registret när fakturan registreras.
    let ny_kund = kredit.is_none().then(|| {
        let ny_vald = Memo::new(move |_| faktura.with(|x| x.kund.trim() == NY));
        let nya_fält = NYA_KUNDFÄLT
            .into_iter()
            .map(|(namn, etikett, fält)| {
                let fel = spärr_vid(Fält::NyKund(fält));
                let värde =
                    faktura.with_untracked(|x| ny_kunds(&mut x.ny_kund.clone(), namn).clone());
                let sätt = move |ev: leptos::ev::Event| {
                    faktura.update(|x| *ny_kunds(&mut x.ny_kund, namn) = event_target_value(&ev));
                };
                let fältet = if namn == "ny_adress" {
                    view! { <textarea id=namn name=namn rows="3" on:input=sätt>{värde}</textarea> }
                        .into_any()
                } else {
                    view! { <input id=namn name=namn type="text" value=värde on:input=sätt /> }
                        .into_any()
                };
                view! {
                    <div>
                        <label for=namn>{etikett}</label>
                        {fältet}
                        {fel}
                    </div>
                }
            })
            .collect_view();
        view! {
            <div class="kort-streckat ny-kund" class:dold=move || !ny_vald.get()>
                <b>"Ny kund"</b>
                <div class="lf-rutnat">{nya_fält}</div>
                <p class="diskret">
                    "Läggs i kundregistret när fakturan registreras. Raderas utkastet försvinner förslaget."
                </p>
            </div>
        }
    });
    let antal = Memo::new(move |_| faktura.with(|x| x.rader.len()));
    let intäktskonton = f
        .intäktskonton
        .iter()
        .map(|(k, namn)| view! { <option value=k.to_string() label=namn.clone()></option> })
        .collect_view();
    let rader = move || {
        (0..antal.get())
            .map(|i| {
                let fält = move |g: fn(&NyExternRad) -> &String| {
                    faktura.with_untracked(|v| g(&v.rader[i]).clone())
                };
                let sats = fält(|r| &r.momssats);
                let satser = MOMSSATSER
                    .into_iter()
                    .map(|s| {
                        view! {
                            <option value=s.to_string() selected=s.to_string() == sats>
                                {format!("{s} %")}
                            </option>
                        }
                    })
                    .collect_view();
                view! {
                    <tr>
                        <td>
                            <select
                                name=format!("momssats_{i}")
                                aria-label="Momssats"
                                on:change=move |ev| {
                                    faktura.update(|x| x.rader[i].momssats = event_target_value(&ev))
                                }
                            >
                                {satser}
                            </select>
                            {spärr_vid(Fält::Rad(i, Radfält::Momssats))}
                        </td>
                        <td>
                            <input
                                name=format!("konto_{i}")
                                type="text"
                                inputmode="numeric"
                                list="intaktskonton"
                                aria-label="Intäktskonto"
                                value=fält(|r| &r.konto)
                                on:input=move |ev| {
                                    faktura.update(|x| x.rader[i].konto = event_target_value(&ev))
                                }
                            />
                            {spärr_vid(Fält::Rad(i, Radfält::Konto))}
                        </td>
                        <td>
                            <input
                                name=format!("belopp_{i}")
                                type="text"
                                inputmode="decimal"
                                class="belopp"
                                aria-label="Belopp exklusive moms"
                                value=fält(|r| &r.belopp)
                                on:input=move |ev| {
                                    faktura.update(|x| x.rader[i].belopp = event_target_value(&ev))
                                }
                            />
                            {spärr_vid(Fält::Rad(i, Radfält::Belopp))}
                        </td>
                    </tr>
                }
            })
            .collect_view()
    };
    let lägg_till_rad = move |_| faktura.update(|x| x.rader.push(NyExternRad::default()));
    let summa = move || {
        let b = faktura.with(|x| x.beräkna());
        format!(
            "Netto {} kr · moms {} kr · totalt {} kr",
            kr(b.netto),
            kr(b.moms),
            kr(b.total)
        )
    };
    let bokförs_som = move || {
        let rader = kontering.with(|k| {
            k.iter()
                .map(|r| {
                    view! {
                        <tr>
                            <td>{r.konto.to_string()}</td>
                            <td class="belopp">{r.debet.eller_tomt()}</td>
                            <td class="belopp">{r.kredit.eller_tomt()}</td>
                        </tr>
                    }
                })
                .collect_view()
        });
        if kontant.get() {
            return view! {
                <h3>"Bokförs vid betalningen"</h3>
                <p class="diskret">
                    "Under kontantmetoden bokförs inget när fakturan registreras. \
                     Betalningen bokför intäkten och momsen."
                </p>
            }
            .into_any();
        }
        view! {
            <h3>"Bokförs som verifikat på fakturadatumet"</h3>
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Konto"</th>
                        <th class="belopp">"Debet"</th>
                        <th class="belopp">"Kredit"</th>
                    </tr>
                </thead>
                <tbody>{rader}</tbody>
            </table>
        }
        .into_any()
    };
    let lista = format!("{bas}/kundfakturor");
    let upplåst = d.upplåst;
    let märke = if upplåst {
        view! { <Märke kod="upplast".to_string() namn="Upplåst".to_string() /> }.into_any()
    } else {
        view! { <UtkastBadge /> }.into_any()
    };
    let radera = (!upplåst).then(|| {
        view! {
            <a class="knapp knapp-fara-diskret" href=format!("{lista}?f={}&radera=1", d.id)>
                "Radera utkast"
            </a>
        }
    });
    let rubrik = if kredit.is_some() {
        "Extern kreditfaktura"
    } else {
        "Extern kundfaktura"
    };
    view! {
        <form
            id="externformular"
            class="lf-formular"
            method="post"
            action=format!("{lista}/extern/{}", d.id)
            novalidate
        >
            <p class="dokument-nummer">{rubrik}</p>
            <h2>{if d.kund.is_empty() { "Ny extern faktura".to_string() } else { d.kund.clone() }}</h2>
            <p>
                {märke}
                " "
                <Märke kod="extern".to_string() namn="Extern".to_string() />
                " "
                <span class="diskret">"Skapat av " {d.skapad.1.clone()} " · " {d.skapad.0.clone()}</span>
            </p>
            <div class="falt-rad">{kundval}</div>
            {ny_kund}
            <div class="lf-rutnat">
                {textfält(
                    "fakturanummer",
                    "Fakturanummer",
                    "text",
                    Fält::Fakturanummer,
                    |x| x.fakturanummer.clone(),
                    |x, v| x.fakturanummer = v,
                )}
                {textfält(
                    "fakturadatum",
                    "Fakturadatum",
                    "date",
                    Fält::Fakturadatum,
                    |x| x.fakturadatum.clone(),
                    |x, v| x.fakturadatum = v,
                )}
                {textfält(
                    "forfallodag",
                    "Förfallodag",
                    "date",
                    Fält::Förfallodag,
                    |x| x.förfallodag.clone(),
                    |x, v| x.förfallodag = v,
                )}
            </div>
            <datalist id="intaktskonton">{intäktskonton}</datalist>
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Momssats"</th>
                        <th>"Intäktskonto"</th>
                        <th class="belopp">"Belopp exkl. moms"</th>
                    </tr>
                </thead>
                <tbody>{rader}</tbody>
            </table>
            <p>
                <button class="knapp knapp-diskret" type="button" on:click=lägg_till_rad>
                    "+ Rad"
                </button>
                " "
                <span class="diskret summa">{summa}</span>
            </p>
            {spärr_vid(Fält::Rader)}
            {spärr_vid(Fält::Summa)}
            <section class="bokfors-som">{bokförs_som}</section>
            <p class="knappar">
                <button
                    class="knapp knapp-primar"
                    type="submit"
                    name="handling"
                    value="registrera"
                    on:click=registrera
                    disabled=move || hydrerad.get() && !spärrar.with(Vec::is_empty)
                >
                    {registrera_text}
                </button>
                <button class="knapp" type="submit" name="handling" value="utkast">
                    "Spara utkast"
                </button>
                {radera}
            </p>
            <p class="diskret">
                "Registrera låser fakturan och bokför den på fakturadatumet med PDF:en som underlag. Externa fakturor ingår inte i fakturaserien."
            </p>
        </form>
    }
}

/// Upplåsningarna med Rätta registrering.
#[component]
fn Rättelser(bas: String, d: ExternVy) -> impl IntoView {
    let _ = bas;
    (!d.rättelser.is_empty()).then(|| {
        let rader = d
            .rättelser
            .iter()
            .map(|(verifikat, rättelse, orsak, tid, namn)| {
                let text = match (verifikat.is_empty(), orsak.is_empty()) {
                    (true, _) => "Registreringen låstes upp".to_string(),
                    (false, true) => format!("{verifikat} vändes med {rättelse}"),
                    (false, false) => format!("{verifikat} vändes med {rättelse}: {orsak}"),
                };
                view! {
                    <li>
                        <span class="diskret">{tid.clone()}</span>
                        " "
                        {text}
                        " "
                        <span class="diskret">"· " {namn.clone()}</span>
                    </li>
                }
            })
            .collect_view();
        view! {
            <section class="registreringsrattelser">
                <h3>"Rättade registreringar"</h3>
                <ul class="historik">{rader}</ul>
            </section>
        }
    })
}

/// En registrerad extern faktura: originalet till vänster och uppgifterna, konteringen,
/// betalningarna och historiken till höger, med handlingarna som paneler.
#[component]
fn Dokument(bas: String, d: ExternVy) -> impl IntoView {
    let lista = format!("{bas}/kundfakturor");
    let här = format!("{lista}?f={}", d.id);
    let vald_panel = use_query_map().with_untracked(|q| q.get("panel"));
    let rad = |etikett: &'static str, v: String| {
        (!v.trim().is_empty()).then(|| {
            view! {
                <dt>{etikett}</dt>
                <dd>{v}</dd>
            }
        })
    };
    let (status, kvar, betala, betalpanel, betalningar) = match d.reskontra.clone() {
        Some(r) => {
            let status =
                view! { <Märke kod=r.märke.0.clone() namn=r.märke.1.clone() /> }.into_any();
            let kvar = match r.återstår_öre {
                ..0 => Some(format!(
                    " · Kunden har tillgodo {} kr",
                    Saldo::från_öre(-r.återstår_öre)
                )),
                1.. => Some(format!(" · {} kvar", r.återstår)),
                0 => None,
            }
            .map(|t| view! { <span class="diskret">{t}</span> });
            let betala = match r.återstår_öre {
                1.. => Some(view! {
                    <a class="knapp knapp-primar" href=format!("{här}&panel=betalning")>
                        "Registrera betalning"
                    </a>
                }),
                ..0 => Some(view! {
                    <a class="knapp knapp-primar" href=format!("{här}&panel=aterbetala")>
                        "Återbetala…"
                    </a>
                }),
                0 => None,
            };
            let betalpanel = match (vald_panel.as_deref(), r.betalning.clone(), r.rätta.clone()) {
                (Some("betalning" | "aterbetala"), Some(v), _) => Some(
                    view! {
                        <Betalningspanel
                            action=format!("{lista}/{}/betalning", d.id)
                            stäng=här.clone()
                            v=v
                        />
                    }
                    .into_any(),
                ),
                (Some("ratta-betalning"), _, Some(rv)) => Some(
                    view! {
                        <RättaBetalningspanel
                            action=format!("{lista}/{}/betalningar/{}/ratta", d.id, rv.id)
                            stäng=här.clone()
                            r=rv
                        />
                    }
                    .into_any(),
                ),
                _ => None,
            };
            let betalningar =
                view! { <Betalningar bas=bas.clone() här=här.clone() r=r /> }.into_any();
            (status, kvar.into_any(), betala, betalpanel, betalningar)
        }
        None => (
            view! { <Märke kod="kreditfaktura".to_string() namn="Kreditfaktura".to_string() /> }
                .into_any(),
            ().into_any(),
            None,
            None,
            ().into_any(),
        ),
    };
    let förlustpanel = d.kundförlust.as_ref().and_then(|k| {
        match (vald_panel.as_deref(), k.panel.clone(), k.återkalla.clone()) {
            (Some("kundforlust"), Some(p), _) => Some(
                view! {
                    <crate::kundforlust::Kundförlustpanel
                        action=format!("{lista}/{}/kundforlust", d.id)
                        stäng=här.clone()
                        p=p
                    />
                }
                .into_any(),
            ),
            (Some("aterkalla-kundforlust"), _, Some(å)) => Some(
                view! {
                    <crate::kundforlust::KundförlustÅterkallelsepanel
                        action=format!("{lista}/{}/kundforlust/{}/aterkalla", d.id, å.id)
                        stäng=här.clone()
                        vy=å
                    />
                }
                .into_any(),
            ),
            _ => None,
        }
    });
    let kreditfaktura = d.krediterar.is_some();
    let kreditpanel = (vald_panel.as_deref() == Some("kreditera-extern") && !kreditfaktura).then(
        || {
            view! {
                <section class="panel" aria-labelledby="kreditera-rubrik">
                    <h3 id="kreditera-rubrik">"Registrera kreditfaktura"</h3>
                    <p class="diskret">
                        "Ladda upp kreditfakturans PDF från det andra systemet. Den registreras med negativa belopp och en hänvisning till den här fakturan."
                    </p>
                    <form
                        class="uppladdning"
                        method="post"
                        action=format!("{lista}/extern/{}/kreditera", d.id)
                        enctype="multipart/form-data"
                    >
                        <label class="slapp">
                            <span>"Släpp kreditfakturans PDF här eller välj en fil"</span>
                            <input type="file" name="fil" required />
                        </label>
                        <a class="knapp" href=här.clone()>
                            "Avbryt"
                        </a>
                        " "
                        <button class="knapp knapp-primar" type="submit">
                            "Ladda upp"
                        </button>
                    </form>
                </section>
            }
            .into_any()
        },
    );
    let rättapanel = (vald_panel.as_deref() == Some("ratta-registrering")).then(|| {
        view! {
            <section class="panel" aria-labelledby="ratta-registrering-rubrik">
                <h3 id="ratta-registrering-rubrik">"Rätta registrering"</h3>
                <p class="diskret">
                    "Registreringens verifikat vänds med en rättelse daterad i dag, och fakturan låses upp så att du kan skriva av den igen och registrera om. Betalningarna ligger kvar."
                </p>
                <form method="post" action=format!("{lista}/extern/{}/ratta-registrering", d.id)>
                    <div class="falt-grupp">
                        <label for="orsak">"Orsak (valfri)"</label>
                        <input id="orsak" name="orsak" type="text" />
                    </div>
                    <a class="knapp" href=här.clone()>
                        "Avbryt"
                    </a>
                    " "
                    <button class="knapp knapp-primar" type="submit">
                        "Rätta registrering"
                    </button>
                </form>
            </section>
        }
        .into_any()
    });
    let panel = betalpanel.or(förlustpanel).or(kreditpanel).or(rättapanel);
    let handlingar = panel.is_none().then(|| {
        let kundförlust = d.kundförlust.as_ref().and_then(|k| {
            if k.gällande.is_some() {
                Some(
                    view! {
                        <a class="knapp" href=format!("{här}&panel=aterkalla-kundforlust")>
                            "Återkalla kundförlust"
                        </a>
                    }
                    .into_any(),
                )
            } else if k.kan_bokföras {
                Some(
                    view! {
                        <a class="knapp knapp-fara-diskret" href=format!("{här}&panel=kundforlust")>
                            "Kundförlust…"
                        </a>
                    }
                    .into_any(),
                )
            } else {
                None
            }
        });
        let kreditera = (!kreditfaktura).then(|| {
            view! {
                <a class="knapp knapp-diskret" href=format!("{här}&panel=kreditera-extern")>
                    "Registrera kreditfaktura…"
                </a>
            }
        });
        view! {
            <p class="dokument-handlingar">
                {betala}
                {kundförlust}
                {kreditera}
                <a class="knapp knapp-diskret" href=format!("{här}&panel=ratta-registrering")>
                    "Rätta registrering…"
                </a>
            </p>
        }
    });
    let verifikat = d.verifikat.clone().map(|(id, nummer)| {
        view! {
            <dt>"Verifikat"</dt>
            <dd>
                <a href=format!("{bas}/verifikat?v={id}")>{nummer}</a>
                <span class="diskret">" registrering"</span>
            </dd>
        }
    });
    let krediterar = d.krediterar.clone().map(|(id, text)| {
        view! {
            <dt>"Krediterar"</dt>
            <dd>
                <a href=format!("{lista}?f={id}")>{text}</a>
            </dd>
        }
    });
    let krediteras_av = (!d.krediteras_av.is_empty()).then(|| {
        let länkar = d
            .krediteras_av
            .iter()
            .map(|(id, nummer, total)| {
                view! {
                    <span class="adressrad">
                        <a href=format!("{lista}?f={id}")>{format!("Kreditfaktura {nummer}")}</a>
                        <span class="diskret">{format!(" · {total} kr")}</span>
                    </span>
                }
            })
            .collect_view();
        view! {
            <dt>"Krediteras av"</dt>
            <dd>{länkar}</dd>
        }
    });
    let kundförlusterna = d.kundförlust.as_ref().map(|k| {
        view! { <crate::kundforlust::Kundförlusterna bas=bas.clone() poster=k.poster.clone() /> }
    });
    let rader = d
        .rader
        .iter()
        .map(|(sats, konto, belopp)| {
            view! {
                <tr>
                    <td>{sats.clone()}</td>
                    <td>{konto.clone()}</td>
                    <td class="belopp">{belopp.clone()}</td>
                </tr>
            }
        })
        .collect_view();
    let moms = d
        .momsrader
        .iter()
        .map(|(text, belopp)| {
            view! {
                <tr>
                    <td colspan="2">{text.clone()}</td>
                    <td class="belopp">{belopp.clone()}</td>
                </tr>
            }
        })
        .collect_view();
    let kontering = d
        .kontering
        .iter()
        .map(|(konto, namn, debet, kredit)| {
            view! {
                <tr>
                    <td>{konto.clone()}</td>
                    <td class="diskret">{namn.clone()}</td>
                    <td class="belopp">{debet.clone()}</td>
                    <td class="belopp">{kredit.clone()}</td>
                </tr>
            }
        })
        .collect_view();
    let historik = d
        .historik
        .iter()
        .map(|(tid, text, namn)| {
            view! {
                <li>
                    <span class="diskret">{tid.clone()}</span>
                    " "
                    {text.clone()}
                    " "
                    <span class="diskret">"· " {namn.clone()}</span>
                </li>
            }
        })
        .collect_view();
    let registrerad = d
        .registrerad
        .clone()
        .map(|(tid, namn)| rad("Registrerad", format!("{tid} av {namn}")));
    let f = &d.formulär;
    view! {
        <div class="med-underlag">
            <aside class="underlagsspalt" aria-label="Originalet">
                <Förhandsvisning bas=bas.clone() u=d.original.clone() />
            </aside>
            <article class="dokument kundfaktura extern">
                <header>
                    <p class="dokument-nummer">
                        {format!(
                            "{} {}",
                            if kreditfaktura { "Extern kreditfaktura" } else { "Extern kundfaktura" },
                            f.fakturanummer,
                        )}
                    </p>
                    <h2>{d.kund.clone()}</h2>
                    <p>
                        {status}
                        " "
                        <Märke kod="extern".to_string() namn="Extern".to_string() />
                        " "
                        <span class="belopp">{d.total.clone()} " kr"</span>
                        {kvar}
                    </p>
                </header>
                {handlingar}
                {panel}
                <dl class="metadata">
                    {krediterar}
                    {krediteras_av}
                    {rad("Fakturadatum", f.fakturadatum.clone())}
                    {rad("Förfallodag", f.förfallodag.clone())}
                    {verifikat}
                    {registrerad}
                    {rad("Original", d.original.filnamn.clone())}
                    {kundförlusterna}
                </dl>
                <table class="rader">
                    <thead>
                        <tr>
                            <th>"Momssats"</th>
                            <th>"Intäktskonto"</th>
                            <th class="belopp">"Belopp exkl. moms"</th>
                        </tr>
                    </thead>
                    <tbody>{rader}</tbody>
                    <tfoot>
                        {moms}
                        <tr class="summa">
                            <td colspan="2">"Totalt med moms"</td>
                            <td class="belopp">{d.total.clone()}</td>
                        </tr>
                    </tfoot>
                </table>
                <h3>"Kontering"</h3>
                <table class="rader">
                    <thead>
                        <tr>
                            <th>"Konto"</th>
                            <th>"Kontonamn"</th>
                            <th class="belopp">"Debet"</th>
                            <th class="belopp">"Kredit"</th>
                        </tr>
                    </thead>
                    <tbody>{kontering}</tbody>
                </table>
                {betalningar}
                <Rättelser bas=bas.clone() d=d.clone() />
                <section>
                    <h3>"Historik"</h3>
                    <ul class="historik">{historik}</ul>
                </section>
            </article>
        </div>
    }
}
