#[cfg(any(unix, test))]
use std::path::Path;

use eyre::{Result, WrapErr, bail};

/// Print the `opt` path of an installed Homebrew formula
///
/// Prints a path such as /opt/homebrew/opt/openssl@3 that keeps pointing at
/// the installed version across upgrades. Name the formula as `brew:<formula>`
/// or `brew:<owner>/<tap>/<formula>`, using its installed name; aliases are not
/// resolved, and a tap-qualified name matches by formula name only. The
/// command does not read mise config files, so only environment variables and
/// global flags apply. If the formula is not installed, it prints nothing and
/// exits with a nonzero status. Works on macOS on Apple silicon and on Linux
/// (x86_64 and arm64).
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    example(
        r#"if root="$(mise bootstrap packages where brew:unzip)"; then
  export PATH="$root/bin:$PATH"
fi"#,
        help = "Put a formula's bin directory on PATH when it is installed"
    )
)]
pub(crate) struct SystemWhere {
    /// Formula to locate, as `brew:<formula>` or `brew:<owner>/<tap>/<formula>`
    package: String,
}

impl SystemWhere {
    /// Print one stable opt path after local validation succeeds; return errors before writing stdout.
    pub(crate) async fn run(self) -> Result<()> {
        let (manager, name) = crate::system::parse_spec(&self.package)
            .wrap_err("use brew:<formula> or brew:<owner>/<tap>/<formula> for package lookup")?;
        if manager != "brew" {
            bail!(
                "bootstrap packages where currently supports brew: formulae; unsupported package {}",
                self.package
            );
        }
        #[cfg(unix)]
        {
            use crate::system::packages::{SystemPackageManager, brew};
            if !brew::BrewManager::new().is_available() {
                bail!("brew:{name}: package lookup supports macOS arm64 and Linux x86_64/arm64");
            }
            let root = brew::package_root(&name)?;
            miseprintln!("{}", path_for_output(&root)?);
            Ok(())
        }
        #[cfg(not(unix))]
        bail!("brew:{name}: package lookup supports macOS arm64 and Linux x86_64/arm64")
    }
}

#[cfg(any(unix, test))]
/// Borrow a lossless, single-line UTF-8 path suitable for shell command substitution.
fn path_for_output(path: &Path) -> Result<&str> {
    match path.to_str() {
        Some(value) if !value.contains(['\r', '\n']) => Ok(value),
        _ => bail!(
            "the prefix cannot be printed as a single UTF-8 path; use a compatible prefix path"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Preserve spaces and the original prefix spelling in the path returned to scripts.
    fn packages_where_output_preserves_valid_utf8_path_exactly() {
        let path = Path::new("/prefix with spaces/opt/widget");
        assert_eq!(
            path_for_output(path).unwrap(),
            "/prefix with spaces/opt/widget"
        );
    }

    #[test]
    /// Reject line breaks that would turn one lookup result into multiple output lines.
    fn packages_where_output_rejects_cr_and_lf() {
        for prefix in ["/prefix\n/opt/widget", "/prefix\r/opt/widget"] {
            let error = path_for_output(Path::new(prefix)).unwrap_err();
            assert!(format!("{error:#}").contains("UTF-8"));
        }
    }

    #[cfg(unix)]
    #[test]
    /// Reject unrepresentable bytes independently of whether the path exists.
    fn packages_where_output_rejects_non_utf8_without_filesystem_access() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path =
            std::path::PathBuf::from(OsString::from_vec(b"/prefix-\xff/opt/widget".to_vec()));
        let error = path_for_output(&path).unwrap_err();
        assert!(format!("{error:#}").contains("UTF-8"));
    }
}
