use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug)]
pub struct RouteInfo {
    pub method: &'static str,
    pub path: &'static str,
    pub key: &'static str,
}

type RouteKey = (&'static str, &'static str);

static REGISTRY: OnceLock<Mutex<HashMap<RouteKey, &'static str>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<RouteKey, &'static str>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register_route(method: &'static str, path: &'static str, key: &'static str) {
    let mut lock = registry().lock().expect("route registry poisoned");
    lock.entry((method, path)).or_insert(key);
}

pub fn is_registered(method: &str, path: &str) -> bool {
    let lock = registry().lock().expect("route registry poisoned");
    lock.contains_key(&(method, path))
}

pub fn routes() -> Vec<RouteInfo> {
    let lock = registry().lock().expect("route registry poisoned");
    lock.iter()
        .map(|(&(m, p), &k)| RouteInfo {
            method: m,
            path: p,
            key: k,
        })
        .collect()
}
