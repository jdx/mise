use crate::file::display_path;
use crate::http::HTTP;
use crate::ui::info;
use crate::{Result, file, minisign};
use clap::ValueHint;
use eyre::{bail, eyre};
use std::path::{Path, PathBuf};
use xx::regex;

/// Generate a script to download+execute mise
///
/// This is designed to be used in a project where contributors may not have mise installed.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Bootstrap {
    /// Sandboxes mise internal directories like MISE_DATA_DIR and MISE_CACHE_DIR into a `.mise` directory in the project
    ///
    /// This is necessary if users may use a different version of mise outside the project.
    #[clap(long, short, verbatim_doc_comment)]
    localize: bool,
    /// Specify mise version to fetch
    #[clap(long, short = 'V', verbatim_doc_comment)]
    version: Option<String>,
    /// instead of outputting the script to stdout, write to a file and make it executable
    #[clap(long, short, verbatim_doc_comment, num_args=0..=1, default_missing_value = "./bin/mise")]
    write: Option<PathBuf>,
    /// Directory to put localized data into
    #[clap(long, verbatim_doc_comment, default_value=".mise", value_hint=ValueHint::DirPath)]
    localized_dir: PathBuf,
    /// Also write a Windows launcher, `<WRITE>.cmd`
    ///
    /// Windows cannot execute the `#!/usr/bin/env bash` script, so a contributor who clones the
    /// project on Windows has nothing to run without this.
    ///
    /// Generated on every host, not only on Windows: the file is committed, and whoever runs it
    /// on Windows is not the person who generated it. Requires `--write`, since stdout cannot
    /// carry two files.
    // Declared last because `clap-sort` requires long-only flags to be in alphabetical order, and
    // the rendered docs follow declaration order. Not a doc comment: this is not help text.
    #[clap(long, verbatim_doc_comment, requires = "write")]
    windows: bool,
}

impl Bootstrap {
    pub async fn run(self) -> eyre::Result<()> {
        let (output, version) = self.generate().await?;
        if let Some(bin) = &self.write {
            if let Some(parent) = bin.parent() {
                file::create_dir_all(parent)?;
            }
            file::write(bin, &output)?;
            file::make_executable(bin)?;
            miseprintln!("Wrote to {}", display_path(bin));
            // Opt-in: most projects never need it, and it is a second committed file.
            if self.windows {
                match windows_script_path(bin) {
                    Some(cmd) => {
                        let body = self.generate_windows(&version).await?;
                        file::write(&cmd, &body)?;
                        miseprintln!("Wrote to {}", display_path(&cmd));
                    }
                    // Asked for explicitly, so say why nothing was written rather than skip in
                    // silence: `<BIN>.cmd.cmd` would be the alternative, and cmd.exe would try to
                    // run the bash script already sitting at `<BIN>`.
                    None => warn!(
                        "--windows wrote nothing: {} already ends in an extension Windows runs. \
                         Point --write at a name without one.",
                        display_path(bin)
                    ),
                }
            }
        } else {
            miseprintln!("{output}");
        }
        Ok(())
    }

    /// The bash script, and the mise version it pins.
    async fn generate(&self) -> Result<(String, String)> {
        let url = if let Some(v) = &self.version {
            format!("https://mise.jdx.dev/v{v}/install.sh")
        } else {
            "https://mise.jdx.dev/install.sh".into()
        };
        let install = HTTP.get_text(&url).await?;
        let install_sig = HTTP.get_text(format!("{url}.minisig")).await?;
        minisign::verify(&minisign::MISE_PUB_KEY, install.as_bytes(), &install_sig)?;
        let install = info::indent_by(install, "        ");
        let version = regex!(r#"version="\$\{MISE_VERSION:-v([0-9.]+)\}""#)
            .captures(&install)
            .unwrap()
            .get(1)
            .unwrap()
            .as_str();

        // install.sh honors MISE_VERSION and MISE_INSTALL_PATH, so the wrapper must not clobber
        // them. The install path is keyed by the requested version rather than the version this
        // script was generated with: `test -f "$MISE_INSTALL_PATH"` skips install.sh entirely, so
        // a fixed path would make a changed MISE_VERSION silently reuse the cached binary.
        let vars = if self.localize {
            let localized_dir = self.localized_dir.to_string_lossy();
            let localized_dir = shell_words::quote(&localized_dir);
            let localized_dir = if self.localized_dir.is_absolute() {
                localized_dir.into_owned()
            } else {
                format!(r#""$project_dir"/{localized_dir}"#)
            };
            format!(
                r#"
local project_dir=$( cd -- "$( dirname -- "${{BASH_SOURCE[0]}}" )" &> /dev/null && cd .. && pwd )
local localized_dir={localized_dir}
export MISE_DATA_DIR="$localized_dir"
export MISE_CONFIG_DIR="$localized_dir"
export MISE_CACHE_DIR="$localized_dir/cache"
export MISE_STATE_DIR="$localized_dir/state"
local mise_version="${{MISE_VERSION:-{version}}}"
mise_version="${{mise_version#v}}"
export MISE_INSTALL_PATH="${{MISE_INSTALL_PATH:-$localized_dir/mise-$mise_version}}"
export MISE_TRUSTED_CONFIG_PATHS="$project_dir${{MISE_TRUSTED_CONFIG_PATHS:+:$MISE_TRUSTED_CONFIG_PATHS}}"
export MISE_IGNORED_CONFIG_PATHS="$HOME/.config/mise${{MISE_IGNORED_CONFIG_PATHS:+:$MISE_IGNORED_CONFIG_PATHS}}"
"#
            )
        } else {
            format!(
                r#"
local cache_home="${{XDG_CACHE_HOME:-$HOME/.cache}}/mise"
local mise_version="${{MISE_VERSION:-{version}}}"
mise_version="${{mise_version#v}}"
export MISE_INSTALL_PATH="${{MISE_INSTALL_PATH:-$cache_home/mise-$mise_version}}"
"#
            )
        };
        let vars = info::indent_by(vars.trim(), "    ");
        let script = format!(
            r#"
#!/usr/bin/env bash
set -eu

__mise_bootstrap() {{
{vars}
    install() {{
        local initial_working_dir="$PWD"
{install}
        cd -- "$initial_working_dir"
    }}
    local MISE_INSTALL_HELP=0
    test -f "$MISE_INSTALL_PATH" || install
}}
__mise_bootstrap
exec -a "$0" "$MISE_INSTALL_PATH" "$@"
"#
        );
        Ok((script.trim().to_string(), version.to_string()))
    }

    /// The Windows form: a `.cmd` that downloads the standalone `mise.exe`, checks it, and runs it.
    ///
    /// Deliberately not a port of `install.sh`. The release publishes a bare
    /// `mise-v<version>-windows-<arch>.exe`, so there is nothing to unpack, and `curl` and
    /// `certutil` have shipped in `%SystemRoot%\system32` since Windows 10 1803 — no PowerShell
    /// and no new published installer are needed.
    ///
    /// The checksums are resolved and verified *here*, at generate time, so that the emitted
    /// script only has to compare two hex strings. This mirrors the bash side, which verifies
    /// `install.sh`'s minisign signature at generate time rather than in the script.
    async fn generate_windows(&self, version: &str) -> Result<String> {
        let base = format!("https://github.com/jdx/mise/releases/download/v{version}");
        let sums = HTTP.get_text(format!("{base}/SHASUMS256.txt")).await?;
        let sums_sig = HTTP
            .get_text(format!("{base}/SHASUMS256.txt.minisig"))
            .await?;
        minisign::verify(&minisign::MISE_PUB_KEY, sums.as_bytes(), &sums_sig)?;

        let checksum = |arch| {
            windows_checksum(&sums, version, arch).ok_or_else(|| {
                eyre!("SHASUMS256.txt for v{version} has no entry for windows-{arch}")
            })
        };
        let vars = if self.localize {
            let localized_dir = windows_localized_dir(&self.localized_dir)?;
            format!(
                r#"
set "project_dir=%~dp0.."
set "localized_dir={localized_dir}"
set "MISE_DATA_DIR=%localized_dir%"
set "MISE_CONFIG_DIR=%localized_dir%"
set "MISE_CACHE_DIR=%localized_dir%\cache"
set "MISE_STATE_DIR=%localized_dir%\state"
set "install_dir=%localized_dir%"
set "MISE_TRUSTED_CONFIG_PATHS=%project_dir%;%MISE_TRUSTED_CONFIG_PATHS%"
rem The global config lives under XDG_CONFIG_HOME, which mise resolves the same way on every
rem platform -- `XDG_CONFIG_HOME` or `%USERPROFILE%\.config`, *not* `%LOCALAPPDATA%`. That is
rem the data dir's rule, and using it here would leave the user's real global config loaded,
rem which is the one thing --localize exists to prevent.
set "xdg_config=%XDG_CONFIG_HOME%"
if not defined XDG_CONFIG_HOME set "xdg_config=%USERPROFILE%\.config"
set "MISE_IGNORED_CONFIG_PATHS=%xdg_config%\mise;%MISE_IGNORED_CONFIG_PATHS%""#
            )
        } else {
            r#"
set "install_dir=%LOCALAPPDATA%\mise""#
                .to_string()
        };
        Ok(windows_script(
            version,
            checksum("x64")?,
            checksum("arm64")?,
            vars.trim_end(),
        ))
    }
}

/// Characters Windows refuses inside a path component. The drive colon is the only legal one, and
/// it is stripped off with the root before this is applied.
const WINDOWS_FORBIDDEN_IN_COMPONENT: [char; 7] = ['<', '>', ':', '"', '|', '?', '*'];

/// `dir` with any root removed — the part that has to survive as path components.
fn windows_path_components(dir: &str) -> &str {
    let bytes = dir.as_bytes();
    let rest = match bytes {
        [drive, b':', b'\\' | b'/', ..] if drive.is_ascii_alphabetic() => &dir[2..],
        _ => dir,
    };
    rest.trim_start_matches(['\\', '/'])
}

/// The `localized_dir` value for the batch script — the same two rules the bash branch applies:
/// escape the value, and join it to the project directory only when it is relative.
///
/// Refuses a value Windows could not hold rather than emitting one. A directory named `C:foo` is
/// ordinary on Linux and the bash half installs into it happily, but on Windows `C:foo` is
/// drive-*relative*, so `%project_dir%\C:foo` is not a path at all and `mkdir` fails — on the
/// contributor's machine, not on the machine that generated the file. Calling it rooted instead
/// would be worse: that drops `%project_dir%\` and installs wherever cmd's per-drive working
/// directory happens to point, silently diverging from the bash half.
fn windows_localized_dir(dir: &Path) -> Result<String> {
    let raw = dir.to_string_lossy();
    if let Some(bad) = windows_path_components(&raw)
        .chars()
        .find(|c| WINDOWS_FORBIDDEN_IN_COMPONENT.contains(c))
    {
        bail!(
            "--localized-dir {raw:?} cannot be carried to Windows: {bad:?} is not allowed in a \
             path component there, so the generated launcher could not create the directory. \
             Drop --windows, or pick a name Windows accepts."
        );
    }
    let escaped = cmd_escape(&raw);
    match is_windows_absolute(&escaped) {
        true => Ok(escaped),
        false => Ok(format!(r"%project_dir%\{escaped}")),
    }
}

/// Escape a value for interpolation into a `set "var=…"` line in the launcher.
///
/// Only `%` needs it. The script keeps delayed expansion off, so `!` is already an ordinary
/// character there — measured: with it off, `set "d=…\p6-od!d"` followed by `mkdir "%d%"` creates
/// `p6-od!d`, and with it on the same two lines create `p6-odd`. A raw `%` would still be read at
/// parse time, so with `x` set a directory named `my%x%dir` becomes `myCLOBBEREDdir`.
///
/// The bash branch has always had the equivalent, via `shell_words::quote`.
fn cmd_escape(value: &str) -> String {
    value.replace('%', "%%")
}

/// Whether Windows would read this as already rooted.
///
/// Decided by inspection rather than by asking [`Path::is_absolute`], because the launcher is
/// generated on Linux and macOS too — the file is committed, and whoever runs it on Windows is not
/// whoever generated it. On those hosts `Path::is_absolute` calls `C:\tools` *relative*, and the
/// script would then join an already-rooted path onto the project directory.
fn is_windows_absolute(dir: &str) -> bool {
    let bytes = dir.as_bytes();
    if matches!(bytes.first(), Some(b'\\' | b'/')) {
        return true;
    }
    // `C:foo` is drive-*relative*, so a separator after the colon is required.
    matches!(bytes, [drive, b':', b'\\' | b'/', ..] if drive.is_ascii_alphabetic())
}

/// Where the Windows form of `--write <bin>` goes, or `None` when `<bin>` is already executable
/// on Windows by name — that keeps `--write bin/mise.cmd` from producing `bin/mise.cmd.cmd`.
fn windows_script_path(bin: &Path) -> Option<PathBuf> {
    let name = bin.file_name()?.to_str()?;
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    if matches!(ext.as_deref(), Some("cmd" | "bat" | "exe")) {
        return None;
    }
    Some(bin.with_file_name(format!("{name}.cmd")))
}

/// The checksum `SHASUMS256.txt` publishes for one Windows binary.
///
/// Matched on the full file name rather than on `windows-{arch}`, so that the `.zip` beside each
/// `.exe` — and `x64` as a tail of nothing else — cannot be picked up by accident.
fn windows_checksum<'a>(sums: &'a str, version: &str, arch: &str) -> Option<&'a str> {
    let name = format!("mise-v{version}-windows-{arch}.exe");
    sums.lines()
        .find(|line| line.trim_end().ends_with(&name))
        .and_then(|line| line.split_whitespace().next())
}

/// The emitted `.cmd`.
///
/// Notes on the parts that are not obvious, all of them measured against a real `cmd.exe`:
///
/// - `certutil` prints the digest on its **second** line. Its surrounding labels are localized —
///   on a Japanese install they are Japanese — so the line has to be taken by position, never by
///   matching the text. The spaces are stripped because older builds print the hex in byte pairs.
/// - `PROCESSOR_ARCHITEW6432` is read as well as `PROCESSOR_ARCHITECTURE`: a 32-bit parent process
///   makes the latter report `x86` on an arm64 machine.
/// - `MISE_VERSION` clears the embedded checksum and falls back to fetching `SHASUMS256.txt`.
///   Without that the override would either be rejected or, worse, checked against the wrong
///   version's hash. The bash side guards the same property by keying the install path on the
///   requested version.
fn windows_script(version: &str, sum_x64: &str, sum_arm64: &str, vars: &str) -> String {
    format!(
        r#"@echo off
rem Delayed expansion stays OFF for the whole script. With it on, cmd runs a second expansion pass
rem over every already-substituted line, so a `!` anywhere in a path -- the project directory this
rem sits in, MISE_INSTALL_PATH, TEMP -- is silently eaten and the script reads and writes a
rem different path than the one it was given. Measured, with delayed expansion off:
rem   set "d=%TEMP%\p6-od!d" & mkdir "%d%"   ->  created ...\p6-od!d
rem and with it on the same two lines create `...\p6-odd` instead. That also covers `%*` on the
rem :run line, which would otherwise be re-expanded and corrupt an argument containing `!`.
rem
rem The one thing this costs is reading a variable assigned inside a parenthesised block, which is
rem why the checksum fallback below is written with a label rather than `if not defined (...)`.
setlocal DisableDelayedExpansion
rem generated by mise generate bootstrap
{vars}

rem Cleared before use, not merely assigned later: the failure paths below delete whatever these
rem name, and cmd inherits the caller's environment. A stray value here would make this script
rem delete something it never created.
set "download_path="
set "sums="

set "pinned_version={version}"
set "sum_x64={sum_x64}"
set "sum_arm64={sum_arm64}"

rem MISE_VERSION itself is never written to. Everything here runs inside `setlocal`, so assigning
rem a fallback to it would hand the launched mise an env var the bash branch does not set.
rem
rem The name has to differ from MISE_VERSION by more than case: cmd variable names are
rem case-insensitive, so a local called `mise_version` *is* MISE_VERSION and would overwrite the
rem caller's value before this line could read it.
set "resolved_version=%pinned_version%"
if defined MISE_VERSION set "resolved_version=%MISE_VERSION%"
if "%resolved_version:~0,1%"=="v" set "resolved_version=%resolved_version:~1%"

set "arch=x64"
if /i "%PROCESSOR_ARCHITECTURE%"=="ARM64" set "arch=arm64"
if /i "%PROCESSOR_ARCHITEW6432%"=="ARM64" set "arch=arm64"

if not defined MISE_INSTALL_PATH set "MISE_INSTALL_PATH=%install_dir%\mise-%resolved_version%.exe"
if exist "%MISE_INSTALL_PATH%" goto :run

set "release=https://github.com/jdx/mise/releases/download/v%resolved_version%"
if "%arch%"=="arm64" (set "expected=%sum_arm64%") else (set "expected=%sum_x64%")
if not "%resolved_version%"=="%pinned_version%" set "expected="

rem A label rather than `if not defined expected (...)`: `sums` is assigned and then read in the
rem same block, which is the one thing plain expansion cannot do. Delayed expansion is the usual
rem answer and is exactly what this script must not turn on -- see the top.
if defined expected goto :have_checksum
set "sums=%TEMP%\mise-shasums-%RANDOM%%RANDOM%.txt"
curl -fsSL -o "%sums%" "%release%/SHASUMS256.txt" || goto :fail_download
for /f "tokens=1" %%s in ('findstr /c:"mise-v%resolved_version%-windows-%arch%.exe" "%sums%"') do set "expected=%%s"
del "%sums%" 2>nul
:have_checksum
if not defined expected (
  echo mise bootstrap: no checksum published for windows-%arch% at v%resolved_version% 1>&2
  exit /b 1
)

set "url=%release%/mise-v%resolved_version%-windows-%arch%.exe"
set "download_path=%TEMP%\mise-bootstrap-%RANDOM%%RANDOM%.exe"
curl -fsSL -o "%download_path%" "%url%" || goto :fail_download

set "actual="
for /f "skip=1 delims=" %%h in ('certutil -hashfile "%download_path%" SHA256') do if not defined actual set "actual=%%h"
set "actual=%actual: =%"

rem `actual` is set above rather than inside this block, so plain expansion reads it correctly:
rem the whole `if (...)` is parsed when it is reached, which is after the `for` loop ran.
if /i not "%actual%"=="%expected%" (
  echo mise bootstrap: checksum mismatch for %url% 1>&2
  echo   expected %expected% 1>&2
  echo   actual   %actual% 1>&2
  del "%download_path%" 2>nul
  exit /b 1
)

rem The parent of MISE_INSTALL_PATH, not install_dir: a caller-supplied MISE_INSTALL_PATH can
rem name a file anywhere, and creating install_dir instead would leave the move below with no
rem destination directory. install.sh does the same -- `mkdir -p "$(dirname "$install_path")"` --
rem and the default path lives under install_dir, so this covers that case too.
for %%i in ("%MISE_INSTALL_PATH%") do set "install_parent=%%~dpi"
if not exist "%install_parent%" mkdir "%install_parent%"
rem Checked, because falling through to :run with the move having failed would execute a path
rem that does not exist and report cmd's error instead of this one.
move /y "%download_path%" "%MISE_INSTALL_PATH%" >nul || goto :fail_install

:run
"%MISE_INSTALL_PATH%" %*
exit /b %ERRORLEVEL%

:fail_download
rem A 404 leaves nothing behind on current curl -- measured on 8.21.0 -- but an interrupted
rem transfer does, and this download is over 100MB, so the partial file is worth removing.
rem
rem The name matters: `tmp` here would be `%TMP%`, because cmd names are case-insensitive, and
rem `del` on a directory prompts to erase its contents. Measured, before this was renamed:
rem   C:\Users\<user>\AppData\Local\Temp\*, Are you sure (Y/N)?
if defined sums del "%sums%" 2>nul
if defined download_path del "%download_path%" 2>nul
echo mise bootstrap: failed to download from %release% 1>&2
exit /b 1

:fail_install
del "%download_path%" 2>nul
echo mise bootstrap: could not move the downloaded binary to %MISE_INSTALL_PATH% 1>&2
exit /b 1
"#
    )
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise generate bootstrap --write ./bin/mise</bold>
    $ <bold>./bin/mise install</bold>                                    <dim># downloads mise to .mise if not already installed</dim>

    <dim># add a launcher for contributors who clone the project on Windows</dim>
    $ <bold>mise generate bootstrap --write ./bin/mise --windows</bold>  <dim># also writes bin/mise.cmd</dim>
    $ <bold>.\bin\mise.cmd install</bold>
"#
);

/// Not gated on `#[cfg(unix)]`: these are string handling, so Windows should run them too — and
/// Windows is the platform the code they cover exists for.
#[cfg(test)]
mod windows_bootstrap_tests {
    use super::*;

    // Shaped like the real file, including the `./` prefix and the `.zip` beside each `.exe`.
    const SUMS: &str = "\
64d7073fe0fe336d49582755acf89991f93d0a182a9d1279fb949080d66e9849  ./mise-v2026.8.4-windows-arm64.exe
0c1f3f0553459e366b284db2a2bf2f0b2c52c6838dddccee96fb494309c97126  ./mise-v2026.8.4-windows-arm64.zip
6f18aaa1a1dc60f929d872380117ddc86f842df03816197fb601b9e3c8254228  ./mise-v2026.8.4-windows-x64.exe
9ce8169533c129c05b8d0daf0325eb5d72fcaa1908cd49adff35a82e3f4f69e1  ./mise-v2026.8.4-windows-x64.zip
b6760c6c4d5e629c31e31cb8a5018316338b01592408062a2aed673cec63cb2d  ./mise-v2026.8.4-linux-x64.tar.gz
";

    #[test]
    fn checksum_picks_the_exe_for_the_right_arch() {
        assert_eq!(
            windows_checksum(SUMS, "2026.8.4", "x64"),
            Some("6f18aaa1a1dc60f929d872380117ddc86f842df03816197fb601b9e3c8254228")
        );
        assert_eq!(
            windows_checksum(SUMS, "2026.8.4", "arm64"),
            Some("64d7073fe0fe336d49582755acf89991f93d0a182a9d1279fb949080d66e9849")
        );
    }

    #[test]
    fn checksum_is_absent_rather_than_wrong() {
        // A version with no Windows build must not silently borrow another version's hash.
        assert_eq!(windows_checksum(SUMS, "2026.8.3", "x64"), None);
        assert_eq!(windows_checksum(SUMS, "2026.8.4", "riscv64"), None);
    }

    #[test]
    fn script_carries_both_checksums_and_the_marker() {
        let script = windows_script(
            "2026.8.4",
            windows_checksum(SUMS, "2026.8.4", "x64").unwrap(),
            windows_checksum(SUMS, "2026.8.4", "arm64").unwrap(),
            "\nset \"install_dir=%LOCALAPPDATA%\\mise\"",
        );
        // Both, because the host that generates this is not necessarily the host that runs it.
        assert!(
            script.contains("6f18aaa1a1dc60f929d872380117ddc86f842df03816197fb601b9e3c8254228")
        );
        assert!(
            script.contains("64d7073fe0fe336d49582755acf89991f93d0a182a9d1279fb949080d66e9849")
        );
        assert!(script.contains("rem generated by mise generate bootstrap"));
        // The override has to clear the embedded hash, or it would be checked against the wrong
        // version's binary.
        assert!(
            script.contains(r#"if not "%resolved_version%"=="%pinned_version%" set "expected=""#)
        );
        // cmd variable names are case-insensitive: a local called `mise_version` *is*
        // MISE_VERSION, so it would overwrite the caller's value before it could be read, and
        // then leak the pinned version into the launched process.
        assert!(!script.contains(r#"set "mise_version="#));
    }

    #[test]
    fn the_cmd_sits_beside_the_script() {
        assert_eq!(
            windows_script_path(Path::new("bin/mise")),
            Some(PathBuf::from("bin/mise.cmd"))
        );
    }

    #[test]
    fn a_name_that_windows_can_already_run_gets_none() {
        for name in ["mise.cmd", "mise.BAT", "mise.exe"] {
            assert_eq!(windows_script_path(Path::new(name)), None, "{name}");
        }
    }

    #[test]
    fn a_relative_localized_dir_is_joined_to_the_project() {
        assert_eq!(
            windows_localized_dir(Path::new(".mise")).unwrap(),
            r"%project_dir%\.mise"
        );
    }

    /// `C:foo` is legal on Linux and unrepresentable on Windows, so there is no value the launcher
    /// could carry: joining gives `%project_dir%\C:foo`, and rooting it drops the project.
    #[test]
    fn a_localized_dir_windows_cannot_hold_is_refused() {
        for dir in ["C:foo", "a:b", "tools/x|y", "we?ird", r"C:\tools\a:b"] {
            let err = windows_localized_dir(Path::new(dir))
                .unwrap_err()
                .to_string();
            assert!(err.contains("cannot be carried to Windows"), "{dir}: {err}");
        }
    }

    /// The control: the drive colon is the one legal colon, and it is not mistaken for a component.
    #[test]
    fn a_drive_colon_is_not_treated_as_a_forbidden_character() {
        for dir in [
            r"C:\tools\mise",
            "C:/tools/mise",
            r"\\server\share",
            "/rooted",
        ] {
            assert!(windows_localized_dir(Path::new(dir)).is_ok(), "{dir}");
        }
    }

    #[test]
    fn an_absolute_localized_dir_is_left_alone() {
        // Joining these onto %project_dir% produced `%project_dir%\C:\tools`, which resolves
        // nowhere. The bash branch has always had this split; the Windows one had not.
        for dir in [
            r"C:\tools\mise",
            "C:/tools/mise",
            r"\\server\share",
            "/rooted",
        ] {
            assert_eq!(
                windows_localized_dir(Path::new(dir)).unwrap(),
                cmd_escape(dir),
                "{dir}"
            );
        }
    }

    #[test]
    fn drive_relative_and_plain_names_are_not_absolute() {
        // The control for the test above. `C:foo` is relative *to the drive's current directory*,
        // so treating it as rooted would drop the project entirely; a bare name obviously is too.
        for dir in ["C:foo", ".mise", "tools/mise", "C-drive"] {
            assert!(!is_windows_absolute(dir), "{dir}");
        }
    }

    #[test]
    fn a_percent_in_the_directory_is_escaped() {
        // With `x` set, a raw `my%x%dir` reaches cmd as `myCLOBBEREDdir`.
        assert_eq!(
            windows_localized_dir(Path::new("my%x%dir")).unwrap(),
            r"%project_dir%\my%%x%%dir"
        );
        // `!` needs no escape because the script keeps delayed expansion off -- that is the
        // property the script test below pins, and this asserts they agree.
        assert_eq!(
            windows_localized_dir(Path::new("my!x!dir")).unwrap(),
            r"%project_dir%\my!x!dir"
        );
    }

    #[test]
    fn the_launcher_creates_the_parent_of_the_install_path() {
        let script = windows_script(
            "2026.8.4",
            windows_checksum(SUMS, "2026.8.4", "x64").unwrap(),
            windows_checksum(SUMS, "2026.8.4", "arm64").unwrap(),
            "\nset \"install_dir=%LOCALAPPDATA%\\mise\"",
        );
        // A caller-supplied MISE_INSTALL_PATH can name a file anywhere. install.sh creates
        // `dirname "$install_path"`; creating install_dir instead leaves the move with no
        // destination, and the failure handler then deletes the download.
        assert!(
            script.contains(r#"for %%i in ("%MISE_INSTALL_PATH%") do set "install_parent=%%~dpi""#)
        );
        assert!(script.contains(r#"if not exist "%install_parent%" mkdir "%install_parent%""#));
        // The control: the mkdir it replaced must be gone, not merely joined by the new one.
        assert!(!script.contains(r#"if not exist "%install_dir%" mkdir "%install_dir%""#));
    }

    #[test]
    fn the_launcher_never_enables_delayed_expansion() {
        let script = windows_script(
            "2026.8.4",
            windows_checksum(SUMS, "2026.8.4", "x64").unwrap(),
            windows_checksum(SUMS, "2026.8.4", "arm64").unwrap(),
            "\nset \"install_dir=%LOCALAPPDATA%\\mise\"",
        );
        // The property every path in this script depends on. With delayed expansion on, a `!`
        // anywhere in the project directory, MISE_INSTALL_PATH or TEMP is eaten on every line
        // that expands it, so the launcher reads and writes a different path than it was given.
        assert!(!script.contains("EnableDelayedExpansion"));
        assert!(script.contains("setlocal DisableDelayedExpansion"));
        // And nothing may reintroduce a `!var!` read, which is what would need it back on.
        assert!(
            !regex!(r"![A-Za-z_][A-Za-z0-9_]*!").is_match(&script),
            "script still contains a delayed-expansion read"
        );
    }
}
