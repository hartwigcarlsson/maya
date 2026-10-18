//! Utskrift som PDF/A-2b (A2): ett enskilt verifikat och kontoplanen. Skriv ut och exporten
//! är samma PDF. Sidfoten får klockans tid och aktörens namn; resten fyller `maya-pdf` i.
//!
//! PDF:en skrivs om vid varje utskrift och sparas inte. Det som arkiveras är datat; en PDF som
//! ska arkiveras som dokument sparas som fil av den som tar ut den (A3, #65).

use maya_domain::kontoplan::bas;
use maya_domain::rakenskapsar::periodnamn;
use maya_pdf::{Dokument, Sidfot};

use crate::aktor::Aktör;
use crate::atkomst::ÅtkomstFel;
use crate::foretag::{self, FöretagsId};
use crate::verifikat::{self, Signatur, VerifikatId, Verifikationsnummer};
use crate::{Databas, DatabasFel, Klocka, kontoplan};

/// En färdig PDF med ett filnamn att spara den under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pdf {
    pub filnamn: String,
    pub innehåll: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum UtskriftFel {
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
    #[error(transparent)]
    Pdf(#[from] maya_pdf::PdfFel),
}

impl From<DatabasFel> for UtskriftFel {
    fn from(e: DatabasFel) -> Self {
        UtskriftFel::Åtkomst(e.into())
    }
}

impl From<sqlx::Error> for UtskriftFel {
    fn from(e: sqlx::Error) -> Self {
        UtskriftFel::Åtkomst(e.into())
    }
}

/// Verifikatet som PDF: rubrik, nummer, metadata, rader, underlagsförteckning och historik.
/// `None` om verifikatet inte finns i företaget.
pub async fn verifikat(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: VerifikatId,
) -> Result<Option<Pdf>, UtskriftFel> {
    let Some(v) = verifikat::verifikatet(db, aktör, företag, id).await? else {
        return Ok(None);
    };
    let f = foretag::företaget(db, aktör, företag).await?;
    let konton = kontoplan::konton(db, aktör, företag).await?;
    let kontonamn = |nr: Option<_>| {
        konton
            .iter()
            .find(|k| Some(k.nummer) == nr)
            .map(|k| k.namn.clone())
            .unwrap_or_default()
    };
    let signatur = |s: &Signatur| maya_pdf::Signatur {
        tid: s.tid,
        namn: s.namn.clone(),
        kanal: s.aktör.kanal.namn().to_string(),
    };
    let filnamn = verifikatets_filnamn(v.nummer);
    let nummer = v.nummer.map(|n| n.to_string());
    let dokument = Dokument::Verifikat(maya_pdf::Verifikat {
        företag: maya_pdf::Företag {
            namn: f.namn,
            orgnr: f.orgnr.to_string(),
        },
        nummer,
        beskrivning: v.beskrivning,
        datum: v.datum,
        period: v.datum.map(periodnamn),
        skapad: signatur(&v.skapad),
        bokförd: v.bokförd.as_ref().map(signatur),
        rader: v
            .rader
            .iter()
            .map(|r| maya_pdf::Rad {
                konto: r.konto,
                kontonamn: kontonamn(r.konto),
                text: r.text.clone(),
                debet: r.debet,
                kredit: r.kredit,
            })
            .collect(),
        // Referenserna till underlagen och var de finns: filnamn och hash i Maya (VF2).
        underlag: v
            .underlag
            .iter()
            .map(|u| maya_pdf::Underlag {
                filnamn: u.filnamn.clone(),
                sha256: u.sha256.clone(),
                storlek: u.storlek,
                komplettering: u.komplettering.then(|| signatur(&u.kopplad)),
            })
            .collect(),
        historik: v
            .historik
            .iter()
            .map(|h| maya_pdf::Historikpost {
                tid: h.post.tid,
                typ: h.post.typ.clone(),
                namn: h.namn.clone(),
                kanal: h.post.aktör.kanal.namn().to_string(),
            })
            .collect(),
    });
    Ok(Some(skriv(db, klocka, aktör, filnamn, &dokument).await?))
}

/// Filnamnet som [`verifikat`] ger PDF:en, t.ex. `V5.pdf`.
pub fn verifikatets_filnamn(nummer: Option<Verifikationsnummer>) -> String {
    match nummer {
        Some(n) => format!("{n}.pdf"),
        None => "Verifikatutkast.pdf".into(),
    }
}

/// Hela kontoplanen som PDF, aktiva och inaktiva konton (S1).
pub async fn kontoplan(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Pdf, UtskriftFel> {
    let konton = kontoplan::konton(db, aktör, företag).await?;
    let f = foretag::företaget(db, aktör, företag).await?;
    // Den senaste BAS-versionen som kontoplanen har konton ur.
    let bas_version = konton
        .iter()
        .filter_map(|k| k.bas_version.clone())
        .max()
        .unwrap_or_else(|| bas::VERSION.to_string());
    let dokument = Dokument::Kontoplan(maya_pdf::Kontoplan {
        företag: maya_pdf::Företag {
            namn: f.namn,
            orgnr: f.orgnr.to_string(),
        },
        bas_version,
        konton: konton
            .into_iter()
            .map(|k| maya_pdf::Konto {
                nummer: k.nummer,
                namn: k.namn,
                aktiv: k.aktiv,
            })
            .collect(),
    });
    skriv(db, klocka, aktör, "kontoplan.pdf".into(), &dokument).await
}

async fn skriv(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    filnamn: String,
    dokument: &Dokument,
) -> Result<Pdf, UtskriftFel> {
    let mut conn = db.pool().acquire().await?;
    let sidfot = Sidfot {
        tidpunkt: klocka.nu(),
        användare: verifikat::namn(&mut conn, aktör).await?,
    };
    drop(conn);
    Ok(Pdf {
        filnamn,
        innehåll: maya_pdf::skriv(dokument, &sidfot)?,
    })
}
