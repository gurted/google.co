use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use gurt_query::ParsedQuery;
use tantivy::collector::TopDocs;
use tantivy::doc;
use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, SchemaBuilder, TextFieldIndexing, TextOptions, FAST, INDEXED,
    STORED, STRING,
};
use tantivy::{Document as _, Index, IndexReader, IndexWriter, Term};

use crate::{IndexDocument, IndexEngine, SearchHit};

/// Field handles for fast access at query time.
#[derive(Debug, Clone)]
pub struct TantivyFields {
    pub url: Field,
    pub domain: Field,
    pub title: Field,
    pub content: Field,
    pub fetch_time: Field,
    pub language: Field,
    pub render_mode: Field,
}

/// Default Tantivy-based index engine.
pub struct TantivyIndexEngine {
    pub schema: Schema,
    pub fields: TantivyFields,
    index: Index,
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
}

impl TantivyIndexEngine {
    /// Build the Schema per requirements: url, domain, title, content,
    /// fetch_time, language, render_mode.
    pub fn build_schema() -> (Schema, TantivyFields) {
        // Indexing options for text fields: positions+freqs for BM25.
        let text_indexing = TextFieldIndexing::default()
            .set_index_option(IndexRecordOption::WithFreqsAndPositions)
            .set_tokenizer("en_stops");

        let text_with_positions = TextOptions::default()
            .set_indexing_options(text_indexing)
            .set_stored();

        let mut sb = SchemaBuilder::default();
        let url = sb.add_text_field("url", STRING | STORED);
        let domain = sb.add_text_field("domain", STRING | STORED);
        let title = sb.add_text_field("title", text_with_positions.clone());
        let content = sb.add_text_field("content", text_with_positions);
        let fetch_time = sb.add_i64_field("fetch_time", INDEXED | FAST | STORED);
        let language = sb.add_text_field("language", STRING | STORED);
        let render_mode = sb.add_text_field("render_mode", STRING | STORED);
        let schema = sb.build();
        let fields = TantivyFields {
            url,
            domain,
            title,
            content,
            fetch_time,
            language,
            render_mode,
        };
        (schema, fields)
    }

    /// Create an engine with an in-memory index (useful for quick setup).
    pub fn with_default_schema() -> Self {
        let (schema, fields) = Self::build_schema();
        let index = Index::create_in_ram(schema.clone());
        register_tokenizer_en(&index);
        let reader = index.reader().expect("build reader");
        let writer = index.writer(50_000_000).expect("build writer");
        Self {
            schema,
            fields,
            index,
            reader,
            writer: Mutex::new(writer),
        }
    }

    /// Open an existing index at `dir`, or create one if missing.
    pub fn open_or_create_in_dir<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let dir = dir.as_ref();
        let (schema, fields) = Self::build_schema();
        if !dir.exists() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("creating index dir {}", dir.display()))?;
        }
        let meta = dir.join("meta.json");
        let index = if meta.exists() {
            Index::open_in_dir(dir).context("open tantivy index")?
        } else {
            Index::create_in_dir(dir, schema.clone()).context("create tantivy index")?
        };
        register_tokenizer_en(&index);
        let reader = index.reader().context("build index reader")?;
        let writer = index.writer(50_000_000).context("create index writer")?;
        Ok(Self {
            schema,
            fields,
            index,
            reader,
            writer: Mutex::new(writer),
        })
    }

    /// Number of documents visible to the current searcher.
    pub fn num_docs(&self) -> u64 {
        self.reader.searcher().num_docs()
    }
}

impl IndexEngine for TantivyIndexEngine {
    fn engine_name(&self) -> &'static str {
        "tantivy"
    }

    fn add(&self, doc: IndexDocument) -> Result<()> {
        let tdoc = doc!(
            self.fields.url => doc.url,
            self.fields.domain => doc.domain,
            self.fields.title => doc.title,
            self.fields.content => doc.content,
            self.fields.fetch_time => doc.fetch_time,
            self.fields.language => doc.language,
            self.fields.render_mode => doc.render_mode
        );
        let mut writer = self.writer.lock().expect("writer lock");
        let _ = writer.add_document(tdoc);
        Ok(())
    }

    fn commit(&self) -> Result<()> {
        let mut writer = self.writer.lock().expect("writer lock");
        writer.commit().context("writer commit")?;
        Ok(())
    }

    fn refresh(&self) -> Result<()> {
        self.reader.reload().context("reader reload")?;
        Ok(())
    }

    fn search(&self, query: &ParsedQuery, page: usize, size: usize) -> Result<Vec<SearchHit>> {
        // Build a BM25-backed boolean query from analyzed terms over title + content.
        let page = page.max(1);
        let size = size.max(1);
        let offset = (page - 1) * size;

        let tokens = analyze_terms(&query.terms);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for t in tokens {
            let term_title = Term::from_field_text(self.fields.title, &t);
            let term_content = Term::from_field_text(self.fields.content, &t);
            clauses.push((
                Occur::Should,
                Box::new(TermQuery::new(
                    term_title,
                    IndexRecordOption::WithFreqsAndPositions,
                )),
            ));
            clauses.push((
                Occur::Should,
                Box::new(TermQuery::new(
                    term_content,
                    IndexRecordOption::WithFreqsAndPositions,
                )),
            ));
        }
        if clauses.is_empty() {
            return Ok(Vec::new());
        }
        let bool_query = BooleanQuery::new(clauses);
        let searcher = self.reader.searcher();
        let top_docs =
            searcher.search(&bool_query, &TopDocs::with_limit(size).and_offset(offset))?;

        fn first_str(v: &serde_json::Value) -> Option<String> {
            match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Array(arr) => {
                    arr.iter().find_map(|x| x.as_str().map(|s| s.to_string()))
                }
                serde_json::Value::Object(map) => {
                    // Sometimes Tantivy representations can be object-y; try common keys
                    for key in ["value", "text", "raw"] {
                        if let Some(s) = map.get(key).and_then(|x| x.as_str()) {
                            return Some(s.to_string());
                        }
                    }
                    None
                }
                _ => None,
            }
        }
        fn first_i64(v: &serde_json::Value) -> Option<i64> {
            match v {
                serde_json::Value::Number(n) => n.as_i64(),
                serde_json::Value::Array(arr) => arr.iter().find_map(|x| x.as_i64()),
                serde_json::Value::Object(map) => map.get("value").and_then(|x| x.as_i64()),
                _ => None,
            }
        }

        let mut out = Vec::with_capacity(top_docs.len());
        for (score, addr) in top_docs {
            let doc = searcher.doc::<tantivy::TantivyDocument>(addr)?;
            let json = doc.to_json(&self.schema);
            let v: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::json!({}));
            let title = v.get("title").and_then(first_str).unwrap_or_default();
            let url = v.get("url").and_then(first_str).unwrap_or_default();
            let domain = v.get("domain").and_then(first_str).unwrap_or_default();
            let fetch_time = v.get("fetch_time").and_then(first_i64).unwrap_or(0);
            out.push(SearchHit {
                title,
                url,
                domain,
                fetch_time,
                score,
            });
        }
        Ok(out)
    }
}

fn analyze_terms(raw_terms: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for term in raw_terms {
        for tok in term.split(|c: char| !c.is_alphanumeric()) {
            let t = tok.to_ascii_lowercase();
            if t.is_empty() {
                continue;
            }
            if is_stopword(&t) {
                continue;
            }
            out.push(t);
        }
    }
    out
}

fn is_stopword(t: &str) -> bool {
    matches!(
        t,
        "a" | "an"
            | "the"
            | "and"
            | "or"
            | "of"
            | "in"
            | "to"
            | "for"
            | "on"
            | "with"
            | "is"
            | "it"
            | "this"
            | "that"
            | "by"
            | "be"
            | "as"
            | "at"
            | "from"
    )
}

fn register_tokenizer_en(index: &Index) {
    use tantivy::tokenizer::{LowerCaser, SimpleTokenizer, StopWordFilter, TextAnalyzer};
    // A minimal English analyzer: lowercase + stopwords removal.
    let stopwords: Vec<String> = vec![
        "a", "an", "the", "and", "or", "of", "in", "to", "for", "on", "with", "is", "it", "this",
        "that", "by", "be", "as", "at", "from",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect();
    let analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(LowerCaser)
        .filter(StopWordFilter::remove(stopwords))
        .build();
    index.tokenizers().register("en_stops", analyzer);
}

use gurt_macros::register_index_engine;

register_index_engine!("tantivy", TantivyIndexEngine::with_default_schema());
