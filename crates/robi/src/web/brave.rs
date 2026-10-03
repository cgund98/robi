//! Brave Search web results.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use crate::domain::settings::{keys::BRAVE_SEARCH_API_KEY, SettingsService};

use super::search::{SearchEngine, SearchError, SearchHit};

const ENDPOINT: &str = "https://api.search.brave.com/res/v1/web/search";
const COUNT: u8 = 5;
const SNIPPET_CHARS: usize = 300;

/// Calls the Brave web search API. The subscription token is read on each call.
pub struct BraveSearch {
    settings: Arc<SettingsService>,
    client: reqwest::Client,
}

impl BraveSearch {
    pub fn new(settings: Arc<SettingsService>, client: reqwest::Client) -> Self {
        Self { settings, client }
    }
}

#[async_trait]
impl SearchEngine for BraveSearch {
    async fn search(&self, query: &str) -> Result<Vec<SearchHit>, SearchError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(SearchError("query is required".into()));
        }
        let key = self
            .settings
            .get(BRAVE_SEARCH_API_KEY)
            .await
            .map_err(|err| SearchError(err.to_string()))?
            .value
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| SearchError(format!("{BRAVE_SEARCH_API_KEY} is not set")))?;

        let response = self
            .client
            .get(ENDPOINT)
            .timeout(Duration::from_secs(15))
            .header("Accept", "application/json")
            .header("X-Subscription-Token", key)
            .query(&[("q", query), ("count", &COUNT.to_string())])
            .send()
            .await
            .map_err(|err| SearchError(format!("search: {err}")))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| SearchError(format!("read search response: {err}")))?;
        if !status.is_success() {
            return Err(SearchError(format!("search returned {status}")));
        }
        parse_hits(&body).map_err(|err| SearchError(format!("decode search response: {err}")))
    }
}

#[derive(Deserialize)]
struct BraveBody {
    #[serde(default)]
    web: Option<BraveWeb>,
}

#[derive(Deserialize)]
struct BraveWeb {
    #[serde(default)]
    results: Vec<BraveResult>,
}

#[derive(Deserialize)]
struct BraveResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    description: String,
}

pub(crate) fn parse_hits(body: &str) -> Result<Vec<SearchHit>, serde_json::Error> {
    let payload: BraveBody = serde_json::from_str(body)?;
    let mut hits = Vec::new();
    let Some(web) = payload.web else {
        return Ok(hits);
    };
    for item in web.results {
        let url = item.url.trim();
        if url.is_empty() {
            continue;
        }
        if hits.len() == COUNT as usize {
            break;
        }
        hits.push(SearchHit {
            title: item.title.trim().to_owned(),
            url: url.to_owned(),
            snippet: clip(item.description.trim(), SNIPPET_CHARS),
        });
    }
    Ok(hits)
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    text.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_keep_title_url_and_a_clipped_snippet() {
        let snippet = "a".repeat(400);
        let body = format!(
            r#"{{"web":{{"results":[{{"title":" Docs ","url":" https://example.com ","description":"{snippet}"}},{{"title":"skip","url":"  "}}]}}}}"#
        );
        let hits = parse_hits(&body).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Docs");
        assert_eq!(hits[0].url, "https://example.com");
        assert_eq!(hits[0].snippet.chars().count(), 300);
    }
}
