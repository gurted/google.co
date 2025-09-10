use std::time::{Duration, Instant};
use std::{env, fs, io};
use serde_json::json;

use gurtd::index::{IndexDocument, IndexEngine};
use gurtd::index::tantivy::TantivyIndexEngine;
use gurtd::index::noop::NoopIndexEngine;
use gurtd::query::{ParsedQuery, QueryFilters};

fn main() -> io::Result<()> {
    let engine_name = env::var("BENCH_ENGINE").unwrap_or_else(|_| "tantivy".to_string());
    let docs: usize = env::var("BENCH_DOCS").ok().and_then(|s| s.parse().ok()).unwrap_or(2_000);
    let queries: usize = env::var("BENCH_QUERIES").ok().and_then(|s| s.parse().ok()).unwrap_or(1_000);
    let output = env::var("BENCH_OUTPUT").unwrap_or_else(|_| "json".to_string());
    let dir_opt = env::var("BENCH_DIR").ok();

    // Build engine
    let (engine_impl, index_dir): (Box<dyn IndexEngine>, Option<std::path::PathBuf>) = match engine_name.as_str() {
        "tantivy" => {
            let path = dir_opt.map(std::path::PathBuf::from).unwrap_or_else(|| tempdir());
            let eng = TantivyIndexEngine::open_or_create_in_dir(&path).expect("open/create tantivy");
            (Box::new(eng), Some(path))
        }
        "noop" => (Box::new(NoopIndexEngine::default()), None),
        other => {
            eprintln!("Unknown engine '{}', supported: tantivy|noop", other);
            std::process::exit(2);
        }
    };

    // Generate synthetic documents
    let mut contents = Vec::with_capacity(docs);
    for i in 0..docs {
        let content = if i % 3 == 0 { "rust rust programming language" } else if i % 3 == 1 { "hello world example content" } else { "lorem ipsum dolor sit amet" };
        contents.push(IndexDocument {
            url: format!("gurt://example.real/doc{}", i),
            domain: "example.real".into(),
            title: format!("Doc {}", i),
            content: content.into(),
            fetch_time: 1_700_000_000 + i as i64,
            language: "en".into(),
            render_mode: "static".into(),
        });
    }

    // Indexing benchmark
    let t0 = Instant::now();
    for d in &contents { engine_impl.add(d.clone()).expect("add"); }
    engine_impl.commit().expect("commit");
    engine_impl.refresh().expect("refresh");
    let index_elapsed = t0.elapsed();

    // Search benchmark
    let queries_terms = [vec!["rust".into()], vec!["hello".into()], vec!["lorem".into()]];
    let mut latencies: Vec<u128> = Vec::with_capacity(queries);
    let t1 = Instant::now();
    for i in 0..queries {
        let terms = &queries_terms[i % queries_terms.len()];
        let pq = ParsedQuery { terms: terms.clone(), filters: QueryFilters::default() };
        let start = Instant::now();
        let _ = engine_impl.search(&pq, 1, 10);
        latencies.push(start.elapsed().as_micros());
    }
    let search_elapsed = t1.elapsed();

    latencies.sort_unstable();
    let p = |q: f64| -> u128 {
        if latencies.is_empty() { return 0; }
        let idx = ((latencies.len() as f64 - 1.0) * q).round() as usize;
        latencies[idx]
    };
    let p50 = p(0.50);
    let p95 = p(0.95);
    let p99 = p(0.99);
    let mean = if latencies.is_empty() { 0.0 } else { (latencies.iter().sum::<u128>() as f64) / (latencies.len() as f64) };

    let index_throughput = if index_elapsed.as_secs_f64() > 0.0 { (docs as f64) / index_elapsed.as_secs_f64() } else { f64::INFINITY };
    let qps = if search_elapsed.as_secs_f64() > 0.0 { (queries as f64) / search_elapsed.as_secs_f64() } else { f64::INFINITY };

    let rss_bytes = read_rss_bytes().unwrap_or(0);
    let index_size_bytes = index_dir.as_ref().and_then(|p| dir_size_bytes(p).ok()).unwrap_or(0);

    if output == "json" {
        let out = json!({
            "engine": engine_name,
            "docs_indexed": docs,
            "index_time_ms": index_elapsed.as_millis(),
            "index_throughput_docs_per_sec": index_throughput,
            "queries": queries,
            "search_time_ms": search_elapsed.as_millis(),
            "qps": qps,
            "latency_us": { "p50": p50, "p95": p95, "p99": p99, "mean": mean },
            "memory": { "rss_bytes": rss_bytes },
            "storage": { "index_size_bytes": index_size_bytes },
        });
        println!("{}", out);
    } else {
        println!("engine={} docs={} index_time_ms={} throughput={:.1} qps={:.1} p50_us={} p95_us={} p99_us={} rss={}B idx_size={}B",
                 engine_name, docs, index_elapsed.as_millis(), index_throughput, qps, p50, p95, p99, rss_bytes, index_size_bytes);
    }

    Ok(())
}

fn tempdir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("gurtd-bench-{}-{}", std::process::id(), nanos()));
    p
}

fn nanos() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
}

fn dir_size_bytes(path: &std::path::Path) -> io::Result<u64> {
    let mut size = 0u64;
    for entry in walk(path)? { size += entry.metadata()?.len(); }
    Ok(size)
}

fn walk(path: &std::path::Path) -> io::Result<Vec<fs::DirEntry>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(path)? { out.push(entry?); }
    let mut i = 0;
    while i < out.len() {
        let entry = &out[i];
        let meta = entry.metadata()?;
        if meta.is_dir() {
            for sub in fs::read_dir(entry.path())? { out.push(sub?); }
        }
        i += 1;
    }
    Ok(out)
}

fn read_rss_bytes() -> Option<u64> {
    // Linux /proc-based RSS reader. On non-Linux, returns None.
    let s = fs::read_to_string("/proc/self/status").ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let num: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(kb) = num.parse::<u64>() { return Some(kb * 1024); }
        }
    }
    None
}

