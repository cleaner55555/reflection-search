//! Sponzorisani slot — strogo odvojen od organskih rezultata.
//!
//! Pravilo: oglas nikad ne ulazi u organski ranking niti menja
//! njegov redosled. Inventar (oglašivači) stiže kasnije —
//! do tada provajder vraća `None` i slot se ne prikazuje.

use serde::Serialize;

/// Jedan označeni oglas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SponsoredAd {
    /// Naslov oglasa.
    pub title: String,
    /// Odredišni URL.
    pub url: String,
    /// Zašto je prikazan (kontekst upita, bez profiliranja).
    pub reason: String,
}

/// Vraća oglas za upit ili `None` (trenutno uvek `None` — nema inventara).
#[must_use]
pub fn get_ad(_query: &str) -> Option<SponsoredAd> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_inventory_means_no_ad() {
        assert_eq!(get_ad("laptop"), None);
    }
}
