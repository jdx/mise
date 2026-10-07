//! Detect system-fallback handoffs that repeat in a Unix `exec` chain.

use crate::{env, file};
use color_eyre::eyre::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

const CHAIN_ENV: &str = "__MISE_SHIM_FALLBACK_CHAIN";
const MAX_HANDOFFS: usize = 32;

#[derive(Default, Deserialize, Serialize)]
pub(super) struct Chain {
    pid: u32,
    // Keep the path's exact bytes: Unix executable names need not be UTF-8.
    handoffs: Vec<(String, Vec<u8>)>,
}

impl Chain {
    pub(super) fn take_inherited() -> Self {
        let inherited = env::var(CHAIN_ENV).unwrap_or_default();
        // A configured tool or command wrapper ends the fallback chain. Only
        // record it again after the final resolution selects a system fallback.
        env::remove_var(CHAIN_ENV);
        Self::parse(&inherited, std::process::id())
    }

    fn parse(value: &str, pid: u32) -> Self {
        serde_json::from_str::<Self>(value)
            .ok()
            .filter(|chain| chain.pid == pid)
            .unwrap_or(Self {
                pid,
                ..Self::default()
            })
    }

    pub(super) fn record(self, bin_name: &str, fallback: &Path) -> Result<()> {
        env::set_var(CHAIN_ENV, self.extended(bin_name, fallback)?);
        Ok(())
    }

    fn extended(mut self, bin_name: &str, fallback: &Path) -> Result<String> {
        let handoff = (
            bin_name.to_owned(),
            fallback.as_os_str().as_encoded_bytes().to_vec(),
        );
        // exec preserves the PID. A child process (e.g. bun run invoking bun)
        // starts a fresh chain, so legitimate nested shim calls still work.
        if self.handoffs.contains(&handoff) {
            bail!(
                "shim fallback recursion detected for {bin_name}: {} keeps resolving back through mise. Set a version for {bin_name}, or remove mise's shim for it.",
                file::display_path(fallback)
            );
        }
        if self.handoffs.len() >= MAX_HANDOFFS {
            bail!(
                "shim fallback chain for {bin_name} reached its {MAX_HANDOFFS}-entry limit before executing {}. Set a version for {bin_name}, or remove mise's shim for it.",
                file::display_path(fallback)
            );
        }
        self.handoffs.push(handoff);
        Ok(serde_json::to_string(&self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    const PID: u32 = 4242;

    fn start() -> Chain {
        Chain::parse("", PID)
    }

    #[test]
    fn repeated_handoff_in_one_process_is_rejected() {
        let value = start().extended("npx", Path::new("/wrapper/npx")).unwrap();
        let chain = Chain::parse(&value, PID)
            .extended("node", Path::new("/wrapper/node"))
            .unwrap();
        let err = Chain::parse(&chain, PID)
            .extended("npx", Path::new("/wrapper/npx"))
            .unwrap_err();
        assert!(err.to_string().contains("recursion detected for npx"));
    }

    #[test]
    fn child_process_starts_a_new_chain() {
        let value = start().extended("bun", Path::new("/system/bun")).unwrap();
        let child = Chain::parse(&value, PID + 1);
        assert!(child.handoffs.is_empty());
        assert_eq!(child.pid, PID + 1);
        assert!(child.extended("bun", Path::new("/system/bun")).is_ok());
    }

    #[test]
    fn distinct_handoffs_are_allowed_until_the_limit() {
        let mut value = String::new();
        for n in 0..MAX_HANDOFFS {
            value = Chain::parse(&value, PID)
                .extended("tool", Path::new(&format!("/wrappers/{n}/tool")))
                .unwrap();
        }
        let err = Chain::parse(&value, PID)
            .extended("tool", Path::new("/next/tool"))
            .unwrap_err();
        assert!(err.to_string().contains("32-entry limit"));
    }

    #[test]
    fn command_name_is_part_of_the_handoff() {
        let value = start().extended("one", Path::new("/wrapper")).unwrap();
        assert!(
            Chain::parse(&value, PID)
                .extended("two", Path::new("/wrapper"))
                .is_ok()
        );
    }

    #[test]
    fn malformed_values_start_a_new_chain() {
        for value in ["", "invalid", "{}", r#"{"pid":4242,"handoffs":null}"#] {
            let chain = Chain::parse(value, PID);
            assert!(chain.handoffs.is_empty());
            assert_eq!(chain.pid, PID);
        }
    }

    #[test]
    fn handoffs_preserve_delimiters_and_non_utf8_paths() {
        let one = Path::new(OsStr::from_bytes(b"/w/\xff\x1f\x1e"));
        let two = Path::new(OsStr::from_bytes(b"/w/\xfe\x1f\x1e"));
        let value = start().extended("tool", one).unwrap();
        assert!(Chain::parse(&value, PID).extended("tool", two).is_ok());
        assert!(Chain::parse(&value, PID).extended("tool", one).is_err());
    }
}
