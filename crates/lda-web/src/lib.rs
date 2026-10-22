//! Web research for a configured endpoint.
//!
//! Nothing here calls a public SearXNG instance on its own. Page text is data.
//! The caller must not treat it as instructions.

#![forbid(unsafe_code)]

mod client;
mod extract;
mod guard;
mod search;

pub use client::WebClient;
pub use extract::{extract_html, Extracted};
pub use search::{parse_search_json, snippet_sources, SearchHit};
