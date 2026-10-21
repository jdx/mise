//! A language server for mise config files.
//!
//! The server speaks LSP over a pair of byte streams. A document is whatever the editor sent; the
//! only files it reads are the configs above an open file, to see whether one lists it as a task
//! include. What a config may contain comes from the JSON schema the host
//! passes in, which mise generates from the types it parses config with, so the server answers
//! with what mise reads rather than a copy of it. A config is only parsed here, never
//! rendered, so a template in an untrusted file cannot run.

mod complete;
mod diagnostics;
mod doc;
mod hover;
mod includes;
mod keypath;
mod rpc;
mod schema;
mod server;
mod tasks;
mod text;

use std::io::{self, BufRead, Write};

/// Rewrites a document the way `mise fmt` does, or says why it cannot.
pub type Formatter = Box<dyn Fn(&str) -> Result<String, String> + Send>;

pub struct Options {
    /// `schema/mise.json`, parsed.
    pub schema: serde_json::Value,
    /// Offers document formatting when set.
    pub format: Option<Formatter>,
    /// The server's version, reported to the client.
    pub version: String,
}

/// Serves one client until it disconnects or sends `exit`.
pub fn serve(options: Options, input: impl BufRead, output: impl Write) -> io::Result<()> {
    server::Server::new(options).run(input, output)
}
