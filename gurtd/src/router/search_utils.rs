pub use super::util::escape_html;

use once_cell::sync::Lazy;

use crate::index::SearchHit;
use crate::link::{domain_trust_from_cname_depth, AuthorityStore};
use crate::search::merge_topk;
use gurt_api::response::SearchResultItem;

static AUTH_STORE: Lazy<std::sync::Mutex<AuthorityStore>> =
	Lazy::new(|| std::sync::Mutex::new(AuthorityStore::new()));

pub(crate) fn rescore_and_convert(hits: Vec<SearchHit>, k: usize) -> Vec<SearchResultItem> {
	if hits.is_empty() {
		return Vec::new();
	}
	let max_bm = hits
		.iter()
		.map(|h| h.score)
		.fold(0.0f32, |a, b| a.max(b))
		.max(1e-6);
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|d| d.as_secs() as i64)
		.unwrap_or(0);
	let half_life_secs = 7 * 24 * 3600i64; // 7 days
	let weights = (0.6f64, 0.2f64, 0.1f64, 0.1f64); // (bm25, authority, trust, recency)
	let store = AUTH_STORE.lock().unwrap();
	let mut rescored: Vec<SearchResultItem> = hits
		.into_iter()
		.map(|h| {
			let bm25 = (h.score / max_bm) as f64;
			let auth = store.get(&h.url).unwrap_or(0.0) as f64;
			let trust = domain_trust_from_cname_depth(0);
			let age = (now - h.fetch_time).max(0) as f64;
			let recency = if half_life_secs > 0 {
				(0.5f64).powf(age / (half_life_secs as f64))
			} else {
				0.0
			};
			let score = weights.0 * bm25 + weights.1 * auth + weights.2 * trust + weights.3 * recency;
			SearchResultItem { title: h.title, url: h.url, score: score as f32 }
		})
		.collect();
	rescored.sort_by(|a, b| {
		b.score
			.partial_cmp(&a.score)
			.unwrap_or(std::cmp::Ordering::Equal)
	});
	merge_topk(vec![rescored], k)
}
