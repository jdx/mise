use color_eyre::eyre::Context;
use eyre::Result;
use std::ffi::OsString;
use std::future::Future;
use std::sync::Arc;
use std::sync::LazyLock as Lazy;

use crate::backend::{Backend, BackendMap};
use crate::cli::args::{BackendArg, BackendResolution};
use crate::config::Settings;
use crate::env;
use crate::path_env::PathEnv;
use crate::timeout::{TimeoutError, run_with_timeout, run_with_timeout_async};
use crate::toolset::ToolVersion;

mod bun;
mod deno;
mod dotnet;
mod elixir;
mod erlang;
mod go;
mod java;
mod node;
pub(crate) mod python;
#[cfg_attr(windows, path = "ruby_windows.rs")]
mod ruby;
mod ruby_common;
mod rust;
mod swift;
mod zig;

pub static CORE_PLUGINS: Lazy<BackendMap> = Lazy::new(|| {
    let plugins: Vec<Arc<dyn Backend>> = vec![
        Arc::new(bun::BunPlugin::new()),
        Arc::new(deno::DenoPlugin::new()),
        Arc::new(dotnet::DotnetPlugin::new()),
        Arc::new(elixir::ElixirPlugin::new()),
        Arc::new(erlang::ErlangPlugin::new()),
        Arc::new(go::GoPlugin::new()),
        Arc::new(java::JavaPlugin::new()),
        Arc::new(node::NodePlugin::new()),
        Arc::new(python::PythonPlugin::new()),
        Arc::new(ruby::RubyPlugin::new()),
        Arc::new(rust::RustPlugin::new()),
        Arc::new(swift::SwiftPlugin::new()),
        Arc::new(zig::ZigPlugin::new()),
    ];
    plugins
        .into_iter()
        .map(|p| (p.id().to_string(), p))
        .collect()
});

pub fn path_env_with_tv_path(tv: &ToolVersion) -> Result<OsString> {
    let mut path_env = PathEnv::from_iter(env::PATH.clone());
    path_env.add(tv.install_path().join("bin"));
    Ok(path_env.join())
}

pub fn run_fetch_task_with_timeout<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send,
    T: Send,
{
    let timeout = Settings::get().fetch_remote_versions_timeout();
    match run_with_timeout(f, timeout) {
        Ok(v) => Ok(v),
        Err(err) => {
            // Only add a hint when the error was actually caused by a timeout
            if err.downcast_ref::<TimeoutError>().is_some() {
                Err(err).context(
                    "change with `fetch_remote_versions_timeout` or env `MISE_FETCH_REMOTE_VERSIONS_TIMEOUT`",
                )
            } else {
                Err(err)
            }
        }
    }
}

pub async fn run_fetch_task_with_timeout_async<F, Fut, T>(f: F) -> Result<T>
where
    Fut: Future<Output = Result<T>> + Send,
    T: Send,
    F: FnOnce() -> Fut,
{
    let timeout = Settings::get().fetch_remote_versions_timeout();
    match run_with_timeout_async(f, timeout).await {
        Ok(v) => Ok(v),
        Err(err) => {
            if err.downcast_ref::<TimeoutError>().is_some() {
                Err(err).context(
                    "change with `fetch_remote_versions_timeout` or env `MISE_FETCH_REMOTE_VERSIONS_TIMEOUT`",
                )
            } else {
                Err(err)
            }
        }
    }
}

pub fn new_backend_arg(tool_name: &str) -> BackendArg {
    BackendArg::new_raw(
        tool_name.to_string(),
        Some(format!("core:{tool_name}")),
        tool_name.to_string(),
        None,
        BackendResolution::new(true),
    )
}

/// Split an `apply_patches` setting into individual sources.
///
/// The setting is one newline-separated list mixing local paths and URLs, the shape
/// `ruby.apply_patches` established.
///
/// Lines are trimmed, and `lines()` rather than `split('\n')` so a CRLF config does not leave a
/// `\r` on the end of every URL. Neither a path nor a URL can legitimately carry surrounding
/// whitespace, and a TOML multi-line string indented to match the surrounding block would
/// otherwise turn every entry into a filename that starts with spaces.
pub fn patch_sources(setting: Option<&str>) -> Vec<String> {
    setting
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Read each patch source, fetching the ones that are URLs.
///
/// Returned one entry per source rather than concatenated: the `-p` strip level a patch needs is
/// decided per patch, so a caller that runs `patch` itself has to keep them apart.
pub async fn fetch_patch_contents(sources: &[String]) -> Result<Vec<String>> {
    let re = xx::regex!(r#"^[Hh][Tt][Tt][Pp][Ss]?://"#);
    let mut patches = vec![];
    for f in sources {
        if re.is_match(f) {
            patches.push(crate::http::HTTP.get_text(f).await?);
        } else {
            patches.push(crate::file::read_to_string(f)?);
        }
    }
    Ok(patches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_sources_splits_and_drops_blank_lines() {
        assert!(patch_sources(None).is_empty());
        assert!(patch_sources(Some("")).is_empty());
        assert_eq!(
            patch_sources(Some(
                "https://example.com/a.patch\n\n./patches/b.patch\n/tmp/c.patch\n"
            )),
            vec![
                "https://example.com/a.patch",
                "./patches/b.patch",
                "/tmp/c.patch"
            ]
        );
    }

    /// A TOML multi-line string indented to match its block, and a config saved with CRLF, both
    /// used to hand back sources with whitespace baked into the path or URL.
    #[test]
    fn patch_sources_ignores_surrounding_whitespace() {
        assert_eq!(
            patch_sources(Some(
                "\n  https://example.com/a.patch  \n   \n\t./patches/b.patch\n"
            )),
            vec!["https://example.com/a.patch", "./patches/b.patch"]
        );
        assert_eq!(
            patch_sources(Some("https://example.com/a.patch\r\n./patches/b.patch\r\n")),
            vec!["https://example.com/a.patch", "./patches/b.patch"]
        );
        assert!(patch_sources(Some("   \n\t\n  ")).is_empty());
    }

    /// Local sources only, so this stays offline. The `join("\n")` at the end is what ruby feeds
    /// to `ruby-build --patch` on stdin — pinned here because that side is not compiled on Windows.
    #[tokio::test]
    async fn fetch_patch_contents_reads_every_source_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.patch");
        let b = dir.path().join("b.patch");
        std::fs::write(&a, "PATCH A\n").unwrap();
        std::fs::write(&b, "PATCH B\n").unwrap();

        let sources = vec![
            a.to_string_lossy().to_string(),
            b.to_string_lossy().to_string(),
        ];
        let contents = fetch_patch_contents(&sources).await.unwrap();

        assert_eq!(contents, vec!["PATCH A\n", "PATCH B\n"]);
        assert_eq!(contents.join("\n"), "PATCH A\n\nPATCH B\n");
    }
}
