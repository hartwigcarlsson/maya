//! E-postadresser: en användare identifieras av sin adress, utan skillnad på stora och små
//! bokstäver.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OgiltigEpost(pub String);

impl std::fmt::Display for OgiltigEpost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}\" är ingen e-postadress", self.0)
    }
}

impl std::error::Error for OgiltigEpost {}

/// Adressen utan blanktecken runt och med små bokstäver, eller fel om den inte ser ut som en
/// e-postadress.
pub fn normalisera(adress: &str) -> Result<String, OgiltigEpost> {
    let adress = adress.trim().to_lowercase();
    let giltig = match adress.split_once('@') {
        Some((lokal, domän)) => {
            !lokal.is_empty()
                && !domän.is_empty()
                && !domän.contains('@')
                && !adress.chars().any(char::is_whitespace)
        }
        None => false,
    };
    if giltig {
        Ok(adress)
    } else {
        Err(OgiltigEpost(adress))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adressen_far_sma_bokstaver_utan_blanktecken_runt() {
        assert_eq!(
            normalisera("  Anna@Example.SE ").unwrap(),
            "anna@example.se"
        );
    }

    #[test]
    fn en_adress_utan_snabel_a_eller_med_tomma_delar_ar_ogiltig() {
        for fel in ["anna", "@example.se", "anna@", "a@b@c", "an na@example.se", ""] {
            assert!(normalisera(fel).is_err(), "{fel}");
        }
    }
}
