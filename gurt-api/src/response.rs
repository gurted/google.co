#[cfg(feature = "json")]
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchResultItem {
    pub title: String,
    pub url: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchResponse {
    pub query: String,
    pub total: u64,
    pub page: u32,
    pub size: u32,
    pub results: Vec<SearchResultItem>,
}

