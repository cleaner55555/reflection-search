//! Pristojnost: pauza između zahteva ka istom domenu.
//!
//! Re-eksport za testiranje; logika živi u `lib.rs::polite_wait`.

#[cfg(test)]
mod tests {
    #[test]
    fn delay_constant_is_reasonable() {
        assert!(super::super::DOMAIN_DELAY.as_secs() >= 1);
    }
}
