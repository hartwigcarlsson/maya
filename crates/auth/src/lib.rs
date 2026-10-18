//! Passkeys: WebAuthn-ceremonierna med `webauthn-rs`, e-post först och sedan passkey, utan
//! conditional UI. Ceremony-state ligger i minnet i 5 minuter; passkeys, länkar och sessioner
//! lagras genom `maya-core`.

mod ceremonier;

use maya_core::aktor::AnvändarId;
use maya_core::inloggning::{self, Inloggad, InloggningsFel, NyPasskey, Registrering, Session};
use maya_core::installation::RelyingParty;
use maya_core::{Databas, Klocka};
use webauthn_rs::prelude::{
    AuthenticationResult, Passkey, PasskeyAuthentication, PasskeyRegistration, Url, Uuid, Webauthn,
    WebauthnBuilder,
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
    /// En inloggad användare lägger till en passkey.
    nya: Ceremonier<(AnvändarId, PasskeyRegistration)>,
    /// En inloggad användare använder en passkey för att få en färsk passkey.
    bekräftelser: Ceremonier<(AnvändarId, Vec<Passkey>, PasskeyAuthentication)>,
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
            nya: Ceremonier::default(),
            bekräftelser: Ceremonier::default(),
        })
    }

    /// Börjar registrera en passkey med en registreringslänks token.
    pub async fn börja_registrering(
        &self,
        token: &str,
    ) -> Result<(CeremoniId, CreationChallengeResponse), AuthFel> {
        let registrering = inloggning::registrering(&self.db, &self.klocka, token).await?;
        let (utmaning, state) = self.utmana_registrering(registrering)?;
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
        enhet: Option<&str>,
    ) -> Result<Session, AuthFel> {
        let (token, state) = self
            .registreringar
            .ta(self.klocka.nu(), ceremoni)
            .ok_or(AuthFel::OkändCeremoni)?;
        let ny = self.registrerad(svar, &state, namn)?;
        Ok(inloggning::slutför_registrering(&self.db, &self.klocka, &token, ny, enhet).await?)
    }

    /// Börjar logga in användaren med e-postadressen `epost`.
    pub async fn börja_inloggning(
        &self,
        epost: &str,
    ) -> Result<(CeremoniId, RequestChallengeResponse), AuthFel> {
        let grund = inloggning::inloggning_för(&self.db, epost)
            .await?
            .ok_or(AuthFel::IngenPasskey)?;
        let passkeys = passkeys(&grund.passkeys)?;
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
        enhet: Option<&str>,
    ) -> Result<Session, AuthFel> {
        let (användare, passkeys, state) = self
            .inloggningar
            .ta(self.klocka.nu(), ceremoni)
            .ok_or(AuthFel::OkändCeremoni)?;
        let resultat = self.webauthn.finish_passkey_authentication(svar, &state)?;
        let uppdaterad = uppdaterad(passkeys, &resultat);
        Ok(inloggning::logga_in(
            &self.db,
            &self.klocka,
            användare,
            resultat.cred_id(),
            uppdaterad,
            enhet,
        )
        .await?)
    }
}

impl Passkeys {
    /// Börjar lägga till en passkey för den inloggade. Kräver en färsk passkey.
    pub async fn börja_ny_passkey(
        &self,
        inloggad: &Inloggad,
    ) -> Result<(CeremoniId, CreationChallengeResponse), AuthFel> {
        let registrering = inloggning::ny_passkey(&self.db, &self.klocka, inloggad).await?;
        let (utmaning, state) = self.utmana_registrering(registrering)?;
        let id = self
            .nya
            .lägg(self.klocka.nu(), (inloggad.användare.id, state));
        Ok((id, utmaning))
    }

    /// Sparar passkeyn som den inloggade registrerade. Kräver en färsk passkey.
    pub async fn slutför_ny_passkey(
        &self,
        inloggad: &Inloggad,
        ceremoni: CeremoniId,
        namn: &str,
        svar: &RegisterPublicKeyCredential,
    ) -> Result<(), AuthFel> {
        let (_, state) = self
            .nya
            .ta(self.klocka.nu(), ceremoni)
            .filter(|(användare, _)| *användare == inloggad.användare.id)
            .ok_or(AuthFel::OkändCeremoni)?;
        let ny = self.registrerad(svar, &state, namn)?;
        Ok(inloggning::lägg_till_passkey(&self.db, &self.klocka, inloggad, ny).await?)
    }

    /// Börjar en ceremoni där den inloggade använder en av sina passkeys, så att den blir färsk.
    pub async fn börja_bekräftelse(
        &self,
        inloggad: &Inloggad,
    ) -> Result<(CeremoniId, RequestChallengeResponse), AuthFel> {
        let grund = inloggning::bekräftelse_för(&self.db, inloggad).await?;
        let passkeys = passkeys(&grund.passkeys)?;
        let (utmaning, state) = self.webauthn.start_passkey_authentication(&passkeys)?;
        let id = self
            .bekräftelser
            .lägg(self.klocka.nu(), (inloggad.användare.id, passkeys, state));
        Ok((id, utmaning))
    }

    /// Slutför bekräftelsen: passkeyn är färsk i sessionen.
    pub async fn slutför_bekräftelse(
        &self,
        inloggad: &Inloggad,
        ceremoni: CeremoniId,
        svar: &PublicKeyCredential,
    ) -> Result<(), AuthFel> {
        let (_, passkeys, state) = self
            .bekräftelser
            .ta(self.klocka.nu(), ceremoni)
            .filter(|(användare, _, _)| *användare == inloggad.användare.id)
            .ok_or(AuthFel::OkändCeremoni)?;
        let resultat = self.webauthn.finish_passkey_authentication(svar, &state)?;
        let uppdaterad = uppdaterad(passkeys, &resultat);
        Ok(inloggning::bekräfta_passkey(
            &self.db,
            &self.klocka,
            inloggad,
            resultat.cred_id(),
            uppdaterad,
        )
        .await?)
    }

    fn utmana_registrering(
        &self,
        registrering: Registrering,
    ) -> Result<(CreationChallengeResponse, PasskeyRegistration), AuthFel> {
        let användare = &registrering.användare;
        let exclude = registrering
            .befintliga_passkeys
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(self.webauthn.start_passkey_registration(
            Uuid::from_bytes(användare.handle),
            &användare.epost,
            &användare.visningsnamn,
            Some(exclude),
        )?)
    }

    fn registrerad(
        &self,
        svar: &RegisterPublicKeyCredential,
        state: &PasskeyRegistration,
        namn: &str,
    ) -> Result<NyPasskey, AuthFel> {
        let passkey = self.webauthn.finish_passkey_registration(svar, state)?;
        let namn = match namn.trim() {
            "" => "Passkey",
            namn => namn,
        };
        Ok(NyPasskey {
            credential_id: passkey.cred_id().to_vec(),
            namn: namn.to_string(),
            passkey: serde_json::to_string(&passkey).expect("en passkey kan serialiseras"),
        })
    }
}

fn passkeys(json: &[String]) -> Result<Vec<Passkey>, AuthFel> {
    let passkeys: Vec<Passkey> = json
        .iter()
        .filter_map(|json| serde_json::from_str(json).ok())
        .collect();
    if passkeys.is_empty() {
        return Err(AuthFel::IngenPasskey);
    }
    Ok(passkeys)
}

/// Räknare och backupflaggor: passkeyn skrivs tillbaka bara när ceremonin ändrade den.
fn uppdaterad(passkeys: Vec<Passkey>, resultat: &AuthenticationResult) -> Option<String> {
    passkeys
        .into_iter()
        .find(|p| p.cred_id() == resultat.cred_id())
        .and_then(|mut p| {
            p.update_credential(resultat)?
                .then(|| serde_json::to_string(&p).expect("en passkey kan serialiseras"))
        })
}
