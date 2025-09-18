use std::collections::HashMap;
use std::time::{Duration, Instant};

use gurt_api::response::{SearchResponse, SearchResultItem};

use crate::query::ParsedQuery;

/// Create a normalized cache key from a parsed query (terms + filters).
pub fn normalize_key(pq: &ParsedQuery) -> String {
    let mut parts: Vec<String> = Vec::new();
    // keep term order but lowercase
    for t in &pq.terms {
        parts.push(t.to_ascii_lowercase());
    }
    if let Some(site) = &pq.filters.site {
        parts.push(format!("site={}", site.to_ascii_lowercase()));
    }
    if let Some(ft) = &pq.filters.filetype {
        parts.push(format!("filetype={}", ft.to_ascii_lowercase()));
    }
    parts.join("\u{1f}") // use a non-space separator
}

#[derive(Clone)]
pub struct CacheEntry {
    pub inserted: Instant,
    pub response: SearchResponse,
}

/// A simple hot query cache with TTL.
pub struct HotQueryCache {
    ttl: Duration,
    map: std::sync::Mutex<HashMap<String, CacheEntry>>,
}

impl HotQueryCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            map: std::sync::Mutex::new(HashMap::new()),
        }
    }
    pub fn get(&self, key: &str) -> Option<SearchResponse> {
        let mut m = self.map.lock().unwrap();
        if let Some(entry) = m.get(key) {
            if entry.inserted.elapsed() <= self.ttl {
                return Some(entry.response.clone());
            }
        }
        m.remove(key);
        None
    }
    pub fn put(&self, key: String, resp: SearchResponse) {
        let mut m = self.map.lock().unwrap();
        m.insert(
            key,
            CacheEntry {
                inserted: Instant::now(),
                response: resp,
            },
        );
        // optional pruning for size constraints could be added here
    }
}

/// Merge multiple shard result lists into a top-k by score, stable across shards.
pub fn merge_topk(mut shards: Vec<Vec<SearchResultItem>>, k: usize) -> Vec<SearchResultItem> {
    // simple k-way merge by repeatedly picking max; suitable for small k in v1
    let mut out: Vec<SearchResultItem> = Vec::new();
    while out.len() < k {
        let mut best_idx: Option<(usize, usize, f32)> = None; // (shard_i, item_i, score)
        for (si, items) in shards.iter().enumerate() {
            if let Some(it) = items.first() {
                let sc = it.score;
                match best_idx {
                    None => best_idx = Some((si, 0, sc)),
                    Some((_bsi, _bi, bscore)) => {
                        if sc > bscore {
                            best_idx = Some((si, 0, sc));
                        }
                    }
                }
            }
        }
        if let Some((si, _bi, _)) = best_idx {
            let it = shards[si].remove(0);
            out.push(it);
        } else {
            break;
        }
    }
    out
}

/// Gather shard results with a per-shard timeout. Late shards are dropped.
pub async fn gather_with_timeout(
    futures: Vec<
        std::pin::Pin<Box<dyn std::future::Future<Output = Vec<SearchResultItem>> + Send>>,
    >,
    per_shard_timeout: Duration,
) -> Vec<Vec<SearchResultItem>> {
    let mut out = Vec::new();
    let mut handles = Vec::new();
    for fut in futures {
        handles.push(tokio::spawn(fut));
    }
    for h in handles {
        match tokio::time::timeout(per_shard_timeout, h).await {
            Ok(Ok(v)) => out.push(v),
            _ => { /* drop timed out shard */ }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn hot_cache_ttl_expires() {
        let cache = HotQueryCache::new(Duration::from_millis(30));
        let resp = SearchResponse {
            query: "k".into(),
            total: 0,
            page: 1,
            size: 10,
            results: vec![],
        };
        cache.put("a".into(), resp.clone());
        assert!(cache.get("a").is_some());
        thread::sleep(Duration::from_millis(40));
        assert!(cache.get("a").is_none());
    }

    #[test]
    fn merge_topk_picks_highest_scores() {
        let s1 = vec![
            SearchResultItem {
                title: "t1".into(),
                url: "u1".into(),
                score: 0.2,
            },
            SearchResultItem {
                title: "t2".into(),
                url: "u2".into(),
                score: 0.1,
            },
        ];
        let s2 = vec![SearchResultItem {
            title: "t3".into(),
            url: "u3".into(),
            score: 0.5,
        }];
        let merged = merge_topk(vec![s1, s2], 2);
        assert_eq!(merged[0].url, "u3");
        assert_eq!(merged[1].url, "u1");
    }

    #[tokio::test]
    async fn gather_drops_timed_out_shard() {
        let f1 = Box::pin(async {
            vec![SearchResultItem {
                title: "a".into(),
                url: "a".into(),
                score: 1.0,
            }]
        });
        let f2 = Box::pin(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            vec![SearchResultItem {
                title: "b".into(),
                url: "b".into(),
                score: 2.0,
            }]
        });
        let shards = gather_with_timeout(vec![f1, f2], Duration::from_millis(10)).await;
        assert_eq!(shards.len(), 1);
        assert_eq!(shards[0][0].url, "a");
    }
}
