//! GET one public URL and stop before a private address or another host.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;
use url::Url;

use super::policy::screen_host;

const MAX_BODY: usize = 1 << 20;
const MAX_REDIRECTS: u32 = 3;
const TIMEOUT: Duration = Duration::from_secs(30);

/// A fetched body, still undecoded. `final_url` stays on the host that was approved.
#[derive(Debug, Clone)]
pub struct FetchedPage {
    pub final_url: Url,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Why a fetch did not return a page.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct FetchError(pub String);

/// Reads one URL. The production fetcher dials only addresses it has screened.
#[async_trait]
pub trait PageFetcher: Send + Sync {
    async fn get(&self, url: &Url) -> Result<FetchedPage, FetchError>;
}

/// `reqwest` with a resolver pinned to the addresses checked for that hop.
#[derive(Debug, Default)]
pub struct HttpFetcher;

#[async_trait]
impl PageFetcher for HttpFetcher {
    async fn get(&self, url: &Url) -> Result<FetchedPage, FetchError> {
        let mut current = url.clone();
        for hop in 0..=MAX_REDIRECTS {
            let host = current
                .host_str()
                .ok_or_else(|| FetchError("url must be http or https".into()))?
                .to_owned();
            let addrs = lookup_host(&host)
                .await
                .map_err(|err| FetchError(format!("resolve {host}: {err}")))?;
            screen_host(&host, &addrs).map_err(FetchError)?;
            let port = current
                .port_or_known_default()
                .ok_or_else(|| FetchError("url port is not allowed".into()))?;
            let response = pinned_get(&current, &host, &addrs, port).await?;
            let status = response.status();
            if status.is_redirection() {
                if hop == MAX_REDIRECTS {
                    return Err(FetchError("stopped after 3 redirects".into()));
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| FetchError(format!("fetch returned {status}")))?
                    .to_owned();
                current = follow_redirect(&current, &location, hop)?;
                continue;
            }
            if !status.is_success() {
                return Err(FetchError(format!("fetch returned {status}")));
            }
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_owned();
            let body = read_limited(response).await?;
            return Ok(FetchedPage {
                final_url: current,
                content_type,
                body,
            });
        }
        Err(FetchError("stopped after 3 redirects".into()))
    }
}

/// The next URL when `location` is a redirect from `current`.
///
/// A different host is refused so the model can call `web_fetch` on it and
/// that host goes through approval on its own.
pub fn follow_redirect(current: &Url, location: &str, hops: u32) -> Result<Url, FetchError> {
    if hops >= MAX_REDIRECTS {
        return Err(FetchError("stopped after 3 redirects".into()));
    }
    let next = current
        .join(location)
        .map_err(|_| FetchError(format!("redirect target {location} is not a URL")))?;
    parse_fetch_url(next.as_str())?;
    let same = current
        .host_str()
        .map(|host| {
            host.trim_end_matches('.')
                .eq_ignore_ascii_case(next.host_str().unwrap_or(""))
        })
        .unwrap_or(false);
    if !same {
        return Err(FetchError(format!(
            "refusing redirect to {next}. call web_fetch on that URL"
        )));
    }
    Ok(next)
}

/// `http` or `https`, no user info, port 80, 443, or none.
pub fn parse_fetch_url(raw: &str) -> Result<Url, FetchError> {
    let url = Url::parse(raw.trim()).map_err(|_| FetchError("url must be http or https".into()))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(FetchError("url must be http or https".into()));
    }
    if url.host_str().is_none() {
        return Err(FetchError("url must be http or https".into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(FetchError("url must not include user info".into()));
    }
    match url.port() {
        None => {}
        Some(80 | 443) => {}
        Some(_) => return Err(FetchError("url port is not allowed".into())),
    }
    Ok(url)
}

async fn lookup_host(host: &str) -> Result<Vec<IpAddr>, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }
    let port = 443;
    let mut addrs = Vec::new();
    let looked = tokio::net::lookup_host((host, port))
        .await
        .map_err(|err| err.to_string())?;
    for addr in looked {
        let ip = addr.ip();
        if !addrs.contains(&ip) {
            addrs.push(ip);
        }
    }
    Ok(addrs)
}

async fn pinned_get(
    url: &Url,
    host: &str,
    addrs: &[IpAddr],
    port: u16,
) -> Result<reqwest::Response, FetchError> {
    let sockets: Vec<SocketAddr> = addrs.iter().map(|ip| SocketAddr::new(*ip, port)).collect();
    let resolver = Arc::new(PinnedResolver {
        host: host.trim_end_matches('.').to_ascii_lowercase(),
        addrs: sockets,
    });
    let client = reqwest::Client::builder()
        .redirect(Policy::none())
        .timeout(TIMEOUT)
        .dns_resolver(resolver)
        .build()
        .map_err(|err| FetchError(format!("fetch: {err}")))?;
    client
        .get(url.clone())
        .header(
            reqwest::header::ACCEPT,
            "text/html, application/xhtml+xml, text/plain, text/markdown, application/json, text/csv, application/xml, text/xml;q=0.8",
        )
        .header(reqwest::header::USER_AGENT, "robi")
        .send()
        .await
        .map_err(|err| FetchError(format!("fetch: {err}")))
}

async fn read_limited(response: reqwest::Response) -> Result<Vec<u8>, FetchError> {
    use futures_util::StreamExt;
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| FetchError(format!("read page: {err}")))?;
        if body.len() + chunk.len() > MAX_BODY {
            let room = MAX_BODY.saturating_sub(body.len());
            body.extend_from_slice(&chunk[..room]);
            break;
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

struct PinnedResolver {
    host: String,
    addrs: Vec<SocketAddr>,
}

impl Resolve for PinnedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let asked = name.as_str().trim_end_matches('.').to_ascii_lowercase();
        let addrs = if asked == self.host {
            self.addrs.clone()
        } else {
            Vec::new()
        };
        Box::pin(async move {
            if addrs.is_empty() {
                Err(Box::from(format!("refusing to resolve {asked}")) as _)
            } else {
                let iter: Addrs = Box::new(addrs.into_iter());
                Ok(iter)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redirect_to_another_host_is_named_and_refused() {
        let current = Url::parse("https://example.com/docs").unwrap();
        let error = follow_redirect(&current, "https://other.example/next", 0).unwrap_err();
        assert!(error.to_string().contains("https://other.example/next"));
    }

    #[test]
    fn a_relative_redirect_stays_on_the_host() {
        let current = Url::parse("https://example.com/docs").unwrap();
        let next = follow_redirect(&current, "/guide", 0).unwrap();
        assert_eq!(next.as_str(), "https://example.com/guide");
    }

    #[test]
    fn user_info_and_odd_ports_are_refused() {
        assert!(parse_fetch_url("https://user:pass@example.com").is_err());
        assert!(parse_fetch_url("https://example.com:8443").is_err());
        assert!(parse_fetch_url("file:///etc/passwd").is_err());
        parse_fetch_url("https://example.com/a").unwrap();
    }
}
