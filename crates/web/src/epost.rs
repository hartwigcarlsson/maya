//! E-postadaptern (#138): skickar fakturamejl, som `core` ger genom porten
//! [`maya_core::utskick::Epost`], med installationens SMTP (`MAYA_SMTP_…`) och SMTP-klienten i
//! `maya_core::smtp`. Utan `MAYA_SMTP_HOST` finns ingen adapter, och webben visar då ingen
//! kryssruta för e-post.

use jiff::Timestamp;
use maya_core::config::{Config, Smtp};
use maya_core::smtp::{self, Brev};
use maya_core::utskick::{Epost, Fakturamejl};

#[derive(Debug, Clone)]
pub(crate) struct SmtpEpost {
    smtp: Smtp,
    /// Värden Maya presenterar sig som (`EHLO`).
    värd: String,
    från: String,
}

impl SmtpEpost {
    /// Adaptern för installationens SMTP, eller `None` utan SMTP.
    pub(crate) fn ur(config: &Config) -> Option<Self> {
        Some(SmtpEpost {
            smtp: config.smtp.clone()?,
            värd: config.public_host(),
            från: config.smtp_från()?,
        })
    }
}

impl Epost for SmtpEpost {
    async fn skicka(&self, mejl: &Fakturamejl, nu: Timestamp) -> Result<(), String> {
        let brev = Brev {
            från: self.från.clone(),
            från_namn: (!mejl.avsändarnamn.is_empty()).then(|| mejl.avsändarnamn.clone()),
            till: mejl.till.clone(),
            svara_till: mejl.svara_till.clone(),
            ämne: mejl.ämne.clone(),
            text: mejl.text.clone(),
            bilagor: vec![mejl.bilaga.clone()],
        };
        smtp::skicka(&self.smtp, &self.värd, brev, nu).await
    }
}
