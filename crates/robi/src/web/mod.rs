//! Public web access for `web_search` and `web_fetch`.
//!
//! Search goes through [`SearchEngine`]. Fetch checks that a host resolves to
//! public addresses, then reduces HTML before the text reaches the model.

mod brave;
mod fetch;
mod html;
mod policy;
mod search;

pub use brave::BraveSearch;
pub use fetch::{
    follow_redirect, parse_fetch_url, FetchError, FetchedPage, HttpFetcher, PageFetcher,
};
pub use html::{reduce_body, ReducedPage};
pub use policy::screen_host;
pub use search::{SearchEngine, SearchError, SearchHit};
