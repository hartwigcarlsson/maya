//! Ceremony-state i minnet. En ceremoni gäller i [`CEREMONI_TTL`] och kan bara tas en gång.

use std::collections::HashMap;
use std::sync::Mutex;

use jiff::{SignedDuration, Timestamp};
use webauthn_rs::prelude::Uuid;

/// Så länge en påbörjad ceremoni gäller.
pub const CEREMONI_TTL: SignedDuration = SignedDuration::from_mins(5);

pub struct Ceremonier<T> {
    pågående: Mutex<HashMap<Uuid, (Timestamp, T)>>,
}

impl<T> Default for Ceremonier<T> {
    fn default() -> Self {
        Ceremonier {
            pågående: Mutex::new(HashMap::new()),
        }
    }
}

impl<T> Ceremonier<T> {
    /// Sparar `state` och ger ceremonins id. Rensar bort gamla ceremonier.
    pub fn lägg(&self, nu: Timestamp, state: T) -> Uuid {
        let id = Uuid::new_v4();
        let mut pågående = self.pågående.lock().expect("ceremonierna");
        pågående.retain(|_, (start, _)| nu < *start + CEREMONI_TTL);
        pågående.insert(id, (nu, state));
        id
    }

    /// Tar ceremonin `id` om den finns och inte är för gammal.
    pub fn ta(&self, nu: Timestamp, id: Uuid) -> Option<T> {
        let (start, state) = self.pågående.lock().expect("ceremonierna").remove(&id)?;
        (nu < start + CEREMONI_TTL).then_some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tid(sekunder: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + sekunder).unwrap()
    }

    #[test]
    fn en_ceremoni_kan_tas_en_gang_inom_5_minuter() {
        let ceremonier = Ceremonier::default();
        let id = ceremonier.lägg(tid(0), "state");

        assert_eq!(ceremonier.ta(tid(299), id), Some("state"));
        assert_eq!(ceremonier.ta(tid(299), id), None);
    }

    #[test]
    fn en_ceremoni_som_ar_aldre_an_5_minuter_galler_inte() {
        let ceremonier = Ceremonier::default();
        let id = ceremonier.lägg(tid(0), "state");

        assert_eq!(ceremonier.ta(tid(300), id), None);
    }
}
