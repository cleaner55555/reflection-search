//! Sopstveni full-text indeks nad Tantivy-jem.
//!
//! Schema: `title` (TEXT, boostiran), `body` (TEXT), `url` (STORED).
//! In-memory za testove; perzistencija na disk stiže po potrebi.

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    IndexRecordOption, OwnedValue, Schema, TextFieldIndexing, TextOptions, STORED, TEXT,
};
use tantivy::{doc, Index, IndexWriter};

/// Broj rezultata po pretrazi indeksa.
pub const TOP_N: usize = 10;

/// Greške indeksa.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    /// Tantivy greška — tekst bez internih detalja.
    #[error("index error")]
    Tantivy,
}

/// Pretraživi dokument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    /// Naslov strane.
    pub title: String,
    /// Vidljiv tekst strane.
    pub body: String,
    /// Kanonski URL.
    pub url: String,
}

/// Pogodak pretrage sa skorom.
#[derive(Debug, Clone)]
pub struct Hit {
    /// URL pogotka.
    pub url: String,
    /// Naslov pogotka.
    pub title: String,
    /// BM25-ish skor (relativan, za sortiranje).
    pub score: f32,
}

/// Full-text indeks u memoriji.
pub struct SearchIndex {
    index: Index,
    reader: tantivy::IndexReader,
    writer: IndexWriter,
    title: tantivy::schema::Field,
    body: tantivy::schema::Field,
    url: tantivy::schema::Field,
}

fn schema() -> (
    Schema,
    tantivy::schema::Field,
    tantivy::schema::Field,
    tantivy::schema::Field,
) {
    let mut builder = Schema::builder();
    let title_opts = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default().set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    let title = builder.add_text_field("title", title_opts);
    let body = builder.add_text_field("body", TEXT);
    let url = builder.add_text_field("url", STORED);
    (builder.build(), title, body, url)
}

impl SearchIndex {
    /// Pravi prazan in-memory indeks.
    pub fn open() -> Result<Self, IndexError> {
        let (schema, title, body, url) = schema();
        let index = Index::create_in_ram(schema);
        let reader = index.reader().map_err(|_| IndexError::Tantivy)?;
        let writer = index.writer(50_000_000).map_err(|_| IndexError::Tantivy)?;
        Ok(Self {
            index,
            reader,
            writer,
            title,
            body,
            url,
        })
    }

    /// Dodaje dokumente i komituje.
    pub fn add(&mut self, docs: &[Doc]) -> Result<(), IndexError> {
        for d in docs {
            self.writer
                .add_document(doc!(
                    self.title => d.title.clone(),
                    self.body => d.body.clone(),
                    self.url => d.url.clone(),
                ))
                .map_err(|_| IndexError::Tantivy)?;
        }
        self.writer.commit().map_err(|_| IndexError::Tantivy)?;
        self.reader.reload().map_err(|_| IndexError::Tantivy)?;
        Ok(())
    }

    /// Pretražuje naslov+telo; vraća do `TOP_N` pogodaka po skoru.
    pub fn search(&self, query: &str) -> Result<Vec<Hit>, IndexError> {
        let searcher = self.reader.searcher();
        let parser = QueryParser::for_index(&self.index, vec![self.title, self.body]);
        let parsed = parser.parse_query(query).map_err(|_| IndexError::Tantivy)?;
        let top = searcher
            .search(&parsed, &TopDocs::with_limit(TOP_N))
            .map_err(|_| IndexError::Tantivy)?;
        let mut hits = Vec::new();
        for (score, addr) in top {
            let doc: tantivy::TantivyDocument =
                searcher.doc(addr).map_err(|_| IndexError::Tantivy)?;
            let url = text_of(doc.get_first(self.url));
            let title = text_of(doc.get_first(self.title));
            if !url.is_empty() {
                hits.push(Hit { url, title, score });
            }
        }
        Ok(hits)
    }
}

fn text_of(value: Option<&OwnedValue>) -> String {
    match value {
        Some(OwnedValue::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn docs(n: usize) -> Vec<Doc> {
        (0..n)
            .map(|i| Doc {
                title: format!("Rust search engine {i}"),
                body: format!(
                    "tantivy full text indexing tutorial number {i} with unique word w{i}"
                ),
                url: format!("https://example.com/{i}"),
            })
            .collect()
    }

    #[test]
    fn finds_by_unique_word() {
        let mut idx = SearchIndex::open().expect("open");
        idx.add(&docs(100)).expect("add");
        let hits = idx.search("w42").expect("search");
        assert!(hits.iter().any(|h| h.url == "https://example.com/42"));
    }

    #[test]
    fn thousand_docs_p95_under_100ms() {
        let mut idx = SearchIndex::open().expect("open");
        idx.add(&docs(1000)).expect("add");
        let mut times: Vec<u128> = (0..20)
            .map(|i| {
                let start = Instant::now();
                let hits = idx
                    .search(&format!("tutorial w{}", i * 50))
                    .expect("search");
                assert!(!hits.is_empty());
                start.elapsed().as_millis()
            })
            .collect();
        times.sort_unstable();
        let p95 = times[(times.len() as f32 * 0.95) as usize - 1];
        assert!(p95 < 100, "p95 {p95}ms preko 100ms");
    }
}
