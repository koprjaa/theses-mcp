// Project: theses-mcp
// File: src/lib.rs
// Description: MCP server for theses.cz: searches Czech university theses, reads their metadata, and lists their documents.
// Author: Jan Alexandr Kopřiva
// License: AGPL-3.0-or-later
//! MCP server for theses.cz. Search Czech university theses and their metadata.

pub mod dispatch;
pub mod dom;
pub mod error;
pub mod http;
pub mod login;
pub mod selftest;
pub mod server;
pub mod session;
pub mod theses;
pub mod tools;
pub mod util;

use http::Transport;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Everything the tools share: the network, the hosts with a session attached, and the
/// directory where sessions are kept between runs.
pub struct Theses {
    pub(crate) http: Arc<dyn Transport>,
    pub(crate) authenticated: Mutex<Vec<String>>,
    pub(crate) store: PathBuf,
}

impl Theses {
    /// The real thing: live network, `~/.theses-mcp`, `$THESES_COOKIES`.
    pub fn from_env() -> Self {
        Theses::new(
            Arc::new(http::ReqwestTransport::new()),
            session::default_store(),
            std::env::var("THESES_COOKIES").ok().as_deref(),
        )
    }
}
