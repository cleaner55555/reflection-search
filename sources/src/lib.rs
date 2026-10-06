//! Izvori rezultata (Brave, Wikipedia, ...).
//!
//! Svaki izvor implementira [`Source`]: async pretragu sa timeoutom.
//! Izvor nikad ne ruši agregator — greška se loguje i vraća prazna lista.

pub mod brave;
pub mod own_index;
pub mod wikipedia;

pub use brave::BraveSource;
pub use own_index::OwnIndexSource;
pub use wikipedia::WikipediaSource;

use search_core::{Query, SearchResult};
use std::time::Duration;

/// Timeout po izvoru — nijedan spor izvor ne sme da zakoči pretragu.
pub const SOURCE_TIMEOUT: Duration = Duration::from_secs(8);

/// Ugovor koji svaki izvor mora da ispuni.
#[async_trait::async_trait]
pub trait Source: Send + Sync {
    /// Stabilno ime izvora, ide u `SearchResult.source`.
    fn name(&self) -> &'static str;

    /// Pretraga; greška znači "ovaj izvor je prazan za ovaj upit".
    async fn search(&self, query: &Query) -> anyhow::Result<Vec<SearchResult>>;
}
