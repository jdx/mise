#[cfg(any(unix, test))]
use std::path::Path;

use eyre::{Result, WrapErr, bail};

/// Print the stable root of an installed Homebrew formula
///
/// Use an explicit brew:<formula> or brew:<owner>/<tap>/<formula> with the
/// canonical installed formula name. Qualified names use the final component
/// as the local rack name; aliases and tap provenance are not resolved.
/// Settings come from environment variables and global CLI options only.
/// The returned opt path follows upgrades and may change after this lookup.
/// Missing or invalid installations produce empty stdout and a nonzero exit status.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment)]
pub(crate) struct SystemWhere {
    /// Explicit brew formula to locate
    package: String,
}

impl SystemWhere {
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
    fn packages_where_output_preserves_valid_utf8_path_exactly() {
        let path = Path::new("/prefix with spaces/opt/widget");
        assert_eq!(
            path_for_output(path).unwrap(),
            "/prefix with spaces/opt/widget"
        );
    }

    #[test]
    fn packages_where_output_rejects_cr_and_lf() {
        for prefix in ["/prefix\n/opt/widget", "/prefix\r/opt/widget"] {
            let error = path_for_output(Path::new(prefix)).unwrap_err();
            assert!(format!("{error:#}").contains("UTF-8"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn packages_where_output_rejects_non_utf8_without_filesystem_access() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path =
            std::path::PathBuf::from(OsString::from_vec(b"/prefix-\xff/opt/widget".to_vec()));
        let error = path_for_output(&path).unwrap_err();
        assert!(format!("{error:#}").contains("UTF-8"));
    }
}
