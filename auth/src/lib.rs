//! Nalozi: registracija, login (JWT), dnevna kvota.
//!
//! Lozinke: argon2id + nasumična so po korisniku. Nikad plain text.
//! Tokeni: HS256 JWT, tajna iz env (`JWT_SECRET`), rok 30 dana.
//! Skladište: SQLite (fajl iz env `USERS_DB_PATH` ili `:memory:` za testove).

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Rok tokena: 30 dana.
pub const TOKEN_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
/// Dnevna kvota pretraga ulogovanog korisnika.
pub const DAILY_QUOTA: u32 = 1000;
/// Minimalna dužina lozinke.
pub const MIN_PASSWORD_LEN: usize = 10;

/// Greške auth sloja (bez osetljivih detalja).
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Pogrešan email ili lozinka (namerno ista poruka za oba).
    #[error("invalid email or password")]
    BadCredentials,
    /// Email zauzet.
    #[error("email already registered")]
    Taken,
    /// Neispravan email/lozinka format.
    #[error("invalid input")]
    BadInput,
    /// Kvota potrošena.
    #[error("daily quota exceeded")]
    Quota,
    /// Nema dovoljno kredita za AI poziv (402).
    #[error("insufficient credits")]
    NoCredits,
    /// Neispravan/istekao token.
    #[error("invalid token")]
    BadToken,
    /// Tehnička greška (detalj ide u log, ne klijentu).
    #[error("storage error")]
    Storage,
}

/// JWT claimi.
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
}

/// Korisnička prodavnica (SQLite iza brave — autentično retko, brzo).
#[derive(Clone)]
pub struct UserStore {
    db: Arc<Mutex<rusqlite::Connection>>,
    enc: Arc<EncodingKey>,
    dec: Arc<DecodingKey>,
}

impl UserStore {
    /// Otvara (i migrira) bazu na putanji; `:memory:` za testove.
    pub fn open(path: &str, jwt_secret: &[u8]) -> Result<Self, AuthError> {
        if jwt_secret.len() < 32 {
            return Err(AuthError::BadInput);
        }
        let db = rusqlite::Connection::open(path).map_err(|_| AuthError::Storage)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS users(
               id TEXT PRIMARY KEY, email TEXT UNIQUE NOT NULL,
               hash TEXT NOT NULL, created_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS usage(
               user_id TEXT NOT NULL, day TEXT NOT NULL, count INTEGER NOT NULL,
               PRIMARY KEY (user_id, day));",
        )
        .map_err(|_| AuthError::Storage)?;
        // Migracija: krediti za AI (prepaid). Postojeće baze dobijaju kolonu sa 0.
        if let Err(e) = db.execute(
            "ALTER TABLE users ADD COLUMN credits REAL NOT NULL DEFAULT 0",
            [],
        ) {
            if !e.to_string().contains("duplicate column") {
                return Err(AuthError::Storage);
            }
        }
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            enc: Arc::new(EncodingKey::from_secret(jwt_secret)),
            dec: Arc::new(DecodingKey::from_secret(jwt_secret)),
        })
    }

    /// Proverava email format (minimalno, bez regex zavisnosti).
    fn valid_email(email: &str) -> bool {
        let email = email.trim();
        match email.find('@') {
            Some(at) if at > 0 && at < email.len() - 1 => email[at + 1..].contains('.'),
            _ => false,
        }
    }

    /// Registruje korisnika; vraća njegov id.
    pub fn register(&self, email: &str, password: &str) -> Result<String, AuthError> {
        let email = email.trim().to_lowercase();
        if !Self::valid_email(&email) || password.len() < MIN_PASSWORD_LEN {
            return Err(AuthError::BadInput);
        }
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| AuthError::Storage)?
            .to_string();
        let id = uuid_simple();
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.db
            .lock()
            .map_err(|_| AuthError::Storage)?
            .execute(
                "INSERT INTO users(id, email, hash, created_ms) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, email, hash, now_ms],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE") {
                    AuthError::Taken
                } else {
                    AuthError::Storage
                }
            })?;
        Ok(id)
    }

    /// Login: proverava lozinku, vraća JWT.
    pub fn login(&self, email: &str, password: &str) -> Result<String, AuthError> {
        let email = email.trim().to_lowercase();
        let (id, hash): (String, String) = self
            .db
            .lock()
            .map_err(|_| AuthError::Storage)?
            .query_row(
                "SELECT id, hash FROM users WHERE email = ?1",
                [&email],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| AuthError::BadCredentials)?;
        let parsed = PasswordHash::new(&hash).map_err(|_| AuthError::Storage)?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| AuthError::BadCredentials)?;
        let ttl = chrono::Duration::from_std(TOKEN_TTL).map_err(|_| AuthError::Storage)?;
        let exp = (chrono::Utc::now() + ttl).timestamp() as usize;
        encode(&Header::default(), &Claims { sub: id, exp }, &self.enc)
            .map_err(|_| AuthError::Storage)
    }

    /// Verifikuje token, vraća id korisnika.
    pub fn verify(&self, token: &str) -> Result<String, AuthError> {
        let mut validation = Validation::default();
        validation.validate_exp = true;
        let data =
            decode::<Claims>(token, &self.dec, &validation).map_err(|_| AuthError::BadToken)?;
        Ok(data.claims.sub)
    }

    /// Troši 1 iz dnevne kvote; `false` = potrošena.
    pub fn spend(&self, user_id: &str) -> Result<bool, AuthError> {
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let db = self.db.lock().map_err(|_| AuthError::Storage)?;
        let count: u32 = db
            .query_row(
                "SELECT count FROM usage WHERE user_id = ?1 AND day = ?2",
                rusqlite::params![user_id, day],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if count >= DAILY_QUOTA {
            return Ok(false);
        }
        db.execute(
            "INSERT INTO usage(user_id, day, count) VALUES (?1, ?2, 1)
             ON CONFLICT(user_id, day) DO UPDATE SET count = count + 1",
            rusqlite::params![user_id, day],
        )
        .map_err(|_| AuthError::Storage)?;
        Ok(true)
    }

    /// Stanje AI kredita korisnika (USD, prepaid).
    pub fn credits(&self, user_id: &str) -> Result<f64, AuthError> {
        self.db
            .lock()
            .map_err(|_| AuthError::Storage)?
            .query_row(
                "SELECT credits FROM users WHERE id = ?1",
                rusqlite::params![user_id],
                |r| r.get(0),
            )
            .map_err(|_| AuthError::Storage)
    }

    /// Uplata kredita (Polar webhook / admin); vraća novo stanje.
    pub fn grant_credits(&self, user_id: &str, amount: f64) -> Result<f64, AuthError> {
        if !amount.is_finite() || amount <= 0.0 {
            return Err(AuthError::BadInput);
        }
        self.db
            .lock()
            .map_err(|_| AuthError::Storage)?
            .execute(
                "UPDATE users SET credits = credits + ?1 WHERE id = ?2",
                rusqlite::params![amount, user_id],
            )
            .map_err(|_| AuthError::Storage)?;
        self.credits(user_id)
    }

    /// Skida trošak AI poziva; vraća novo stanje (može u mali minus —
    /// gate pre poziva je kontrola, ovo je knjiženje stvarnog troška).
    pub fn spend_credits(&self, user_id: &str, cost: f64) -> Result<f64, AuthError> {
        if !cost.is_finite() || cost < 0.0 {
            return Err(AuthError::BadInput);
        }
        self.db
            .lock()
            .map_err(|_| AuthError::Storage)?
            .execute(
                "UPDATE users SET credits = credits - ?1 WHERE id = ?2",
                rusqlite::params![cost, user_id],
            )
            .map_err(|_| AuthError::Storage)?;
        self.credits(user_id)
    }
}

/// Nasumični id bez nove zavisnosti (rand 0.8 + hex ručno).
fn uuid_simple() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> UserStore {
        UserStore::open(":memory:", b"test-secret-32-bytes-long-xxxxxx").expect("open")
    }

    #[test]
    fn register_login_verify_roundtrip() {
        let s = store();
        let id = s
            .register("a@b.com", "dovoljno-dugacka-1")
            .expect("register");
        let token = s.login("a@b.com", "dovoljno-dugacka-1").expect("login");
        assert_eq!(s.verify(&token).expect("verify"), id);
    }

    #[test]
    fn wrong_password_and_unknown_user_same_error() {
        let s = store();
        s.register("a@b.com", "dovoljno-dugacka-1")
            .expect("register");
        assert!(matches!(
            s.login("a@b.com", "pogresna-lozinka-2"),
            Err(AuthError::BadCredentials)
        ));
        assert!(matches!(
            s.login("nema@ga.com", "bilo-sta-dugo-33"),
            Err(AuthError::BadCredentials)
        ));
    }

    #[test]
    fn duplicate_and_bad_input_rejected() {
        let s = store();
        s.register("a@b.com", "dovoljno-dugacka-1")
            .expect("register");
        assert!(matches!(
            s.register("a@b.com", "druga-dovoljna-22"),
            Err(AuthError::Taken)
        ));
        assert!(matches!(
            s.register("nije-email", "dovoljno-dugacka-1"),
            Err(AuthError::BadInput)
        ));
        assert!(matches!(
            s.register("b@c.com", "kratka"),
            Err(AuthError::BadInput)
        ));
    }

    #[test]
    fn quota_eventually_blocks() {
        let s = store();
        let id = s
            .register("a@b.com", "dovoljno-dugacka-1")
            .expect("register");
        for _ in 0..DAILY_QUOTA {
            assert!(s.spend(&id).expect("spend"));
        }
        assert!(!s.spend(&id).expect("over"));
    }

    #[test]
    fn credits_grant_spend_roundtrip() {
        let s = store();
        let id = s
            .register("a@b.com", "dovoljno-dugacka-1")
            .expect("register");
        assert!((s.credits(&id).expect("credits") - 0.0).abs() < f64::EPSILON);
        assert!((s.grant_credits(&id, 1.0).expect("grant") - 1.0).abs() < 1e-9);
        let left = s.spend_credits(&id, 0.25).expect("spend");
        assert!((left - 0.75).abs() < 1e-9);
        assert!(matches!(
            s.grant_credits(&id, f64::NAN),
            Err(AuthError::BadInput)
        ));
        assert!(matches!(
            s.spend_credits(&id, -1.0),
            Err(AuthError::BadInput)
        ));
    }

    #[test]
    fn bad_token_rejected() {
        let s = store();
        assert!(matches!(
            s.verify("ne.token.uopste"),
            Err(AuthError::BadToken)
        ));
    }

    #[test]
    fn short_jwt_secret_rejected() {
        assert!(matches!(
            UserStore::open(":memory:", b"kratko"),
            Err(AuthError::BadInput)
        ));
    }
}
