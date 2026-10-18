//! Passkeys: WebAuthn-ceremonierna med `webauthn-rs`, e-post först och sedan passkey, utan
//! conditional UI. Ceremony-state ligger i minnet i 5 minuter; passkeys, länkar och sessioner
//! lagras genom `maya-core`.

mod ceremonier;

use maya_core::aktor::AnvändarId;
use maya_core::inloggning::{self, InloggningsFel, NyPasskey, Session};
use maya_core::installation::RelyingParty;
use maya_core::{Databas, Klocka};
use webauthn_rs::prelude::{
    Passkey, PasskeyAuthentication, PasskeyRegistration, Url, Uuid, Webauthn, WebauthnBuilder,
};

pub use ceremonier::{CEREMONI_TTL, Ceremonier};
pub use webauthn_rs::prelude::{
    CreationChallengeResponse, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse,
};

#[derive(Debug, thiserror::Error)]
pub enum AuthFel {
    #[error("ceremonin är okänd eller för gammal; börja om")]
    OkändCeremoni,
    #[error("det finns ingen användare med passkey för den e-postadressen")]
    IngenPasskey,
    #[error("passkeyn kunde inte verifieras")]
    Webauthn(#[from] webauthn_rs::prelude::WebauthnError),
    #[error(transparent)]
    Inloggning(#[from] InloggningsFel),
}

impl AuthFel {
    /// Om felet beror på klienten (fel länk, okänd passkey) och inte på servern.
    pub fn klientens(&self) -> bool {
        !matches!(
            self,
            AuthFel::Inloggning(InloggningsFel::Databas(_))
                | AuthFel::Inloggning(InloggningsFel::KanalenFårInte)
        )
    }
}

/// En ceremoni i flykt, som klienten skickar tillbaka när den är klar.
pub type CeremoniId = Uuid;

/// Installationens passkeys.
pub struct Passkeys {
    webauthn: Webauthn,
    db: Databas,
    klocka: Klocka,
    registreringar: Ceremonier<(String, PasskeyRegistration)>,
    inloggningar: Ceremonier<(AnvändarId, Vec<Passkey>, PasskeyAuthentication)>,
}

impl Passkeys {
    pub fn new(rp: &RelyingParty, db: Databas, klocka: Klocka) -> Result<Self, AuthFel> {
        let origin = Url::parse(&rp.origin)
            .map_err(|_| AuthFel::Webauthn(webauthn_rs::prelude::WebauthnError::InvalidRPOrigin))?;
        let webauthn = WebauthnBuilder::new(&rp.rp_id, &origin)?
            .rp_name("Maya")
            .build()?;
        Ok(Passkeys {
            webauthn,
            db,
            klocka,
            registreringar: Ceremonier::default(),
            inloggningar: Ceremonier::default(),
        })
    }

    /// Börjar registrera en passkey med en registreringslänks token.
    pub async fn börja_registrering(
        &self,
        token: &str,
    ) -> Result<(CeremoniId, CreationChallengeResponse), AuthFel> {
        let registrering = inloggning::registrering(&self.db, &self.klocka, token).await?;
        let användare = &registrering.användare;
        let exclude = registrering
            .befintliga_passkeys
            .into_iter()
            .map(Into::into)
            .collect();
        let (utmaning, state) = self.webauthn.start_passkey_registration(
            Uuid::from_bytes(användare.handle),
            &användare.epost,
            &användare.visningsnamn,
            Some(exclude),
        )?;
        let id = self
            .registreringar
            .lägg(self.klocka.nu(), (token.to_string(), state));
        Ok((id, utmaning))
    }

    /// Slutför registreringen, förbrukar länken och loggar in användaren.
    pub async fn slutför_registrering(
        &self,
        ceremoni: CeremoniId,
        namn: &str,
        svar: &RegisterPublicKeyCredential,
    ) -> Result<Session, AuthFel> {
        let (token, state) = self
            .registreringar
            .ta(self.klocka.nu(), ceremoni)
            .ok_or(AuthFel::OkändCeremoni)?;
        let passkey = self.webauthn.finish_passkey_registration(svar, &state)?;
        let namn = match namn.trim() {
            "" => "Passkey",
            namn => namn,
        };
        let ny = NyPasskey {
            credential_id: passkey.cred_id().to_vec(),
            namn: namn.to_string(),
            passkey: serde_json::to_string(&passkey).expect("en passkey kan serialiseras"),
        };
        Ok(inloggning::slutför_registrering(&self.db, &self.klocka, &token, ny).await?)
    }

    /// Börjar logga in användaren med e-postadressen `epost`.
    pub async fn börja_inloggning(
        &self,
        epost: &str,
    ) -> Result<(CeremoniId, RequestChallengeResponse), AuthFel> {
        let grund = inloggning::inloggning_för(&self.db, epost)
            .await?
            .ok_or(AuthFel::IngenPasskey)?;
        let passkeys: Vec<Passkey> = grund
            .passkeys
            .iter()
            .filter_map(|json| serde_json::from_str(json).ok())
            .collect();
        if passkeys.is_empty() {
            return Err(AuthFel::IngenPasskey);
        }
        let (utmaning, state) = self.webauthn.start_passkey_authentication(&passkeys)?;
        let id = self
            .inloggningar
            .lägg(self.klocka.nu(), (grund.användare.id, passkeys, state));
        Ok((id, utmaning))
    }

    /// Slutför inloggningen och ger en ny session.
    pub async fn slutför_inloggning(
        &self,
        ceremoni: CeremoniId,
        svar: &PublicKeyCredential,
    ) -> Result<Session, AuthFel> {
        let (användare, passkeys, state) = self
            .inloggningar
            .ta(self.klocka.nu(), ceremoni)
            .ok_or(AuthFel::OkändCeremoni)?;
        let resultat = self.webauthn.finish_passkey_authentication(svar, &state)?;
        // Räknare och backupflaggor: passkeyn skrivs tillbaka bara när ceremonin ändrade den.
        let uppdaterad = passkeys
            .into_iter()
            .find(|p| p.cred_id() == resultat.cred_id())
            .and_then(|mut p| {
                p.update_credential(&resultat)?
                    .then(|| serde_json::to_string(&p).expect("en passkey kan serialiseras"))
            });
        Ok(inloggning::logga_in(
            &self.db,
            &self.klocka,
            användare,
            resultat.cred_id(),
            uppdaterad,
        )
        .await?)
    }
}
