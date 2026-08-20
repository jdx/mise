//! Native Linux source builds for formulae without a usable bottle.
//!
//! Building a formula means running its Ruby `install` method. mise does
//! this without Homebrew: it provisions a mise-managed ruby (precompiled,
//! via the normal tool machinery), downloads the formula's .rb from
//! homebrew/core (sha256-verified against the API metadata), stages the
//! sha256-verified source archive, and evaluates the formula with the
//! Formula-DSL shim in shim.rb. Build dependencies are poured as bottles
//! beforehand by the regular closure machinery (see resolve.rs), so the
//! build environment points at real kegs in the canonical prefix.
//!
//! macOS remains bottle-only. `sandbox-exec` cannot contain a descendant that
//! double-forks or creates a new session, so a detached formula process could
//! retain a writable keg file descriptor after the Ruby leader exits. Source
//! builds therefore fail before downloads, staging, or Cellar mutation there.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use eyre::{WrapErr, bail};

use super::api::Formula;
use super::pour;
use super::prefix;
use super::resolve::ResolvedFormula;
use super::tag;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::{ExtractOptions, ExtractionFormat};
use crate::http::{HTTP, HTTP_FETCH};
use crate::result::Result;
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::progress_report::SingleReport;

const SHIM_RB: &str = include_str!("shim.rb");
const HOMEBREW_CORE_RAW: &str = "https://raw.githubusercontent.com/Homebrew/homebrew-core";
const SOURCE_SYSTEM_PATH: &[&str] = &["/usr/bin", "/bin", "/usr/sbin", "/sbin"];
const SOURCE_INSTALL_HELPER_RB: &str = r####"
# frozen_string_literal: true

# A deliberately small replacement for the GNU install behavior exercised by
# the supported source-build boundary. Strict seccomp denies every chmod,
# chown, xattr, and timestamp mutation, including descriptor variants. This
# helper instead creates each replacement with its final mode, fsyncs it, and
# atomically renames it inside one of the two Rust-pinned writable roots.

File.umask(0)

def helper_error(message)
  raise ArgumentError, "mise source install helper: #{message}"
end

def lstat_if_exists(path)
  File.lstat(path)
rescue Errno::ENOENT
  nil
end

def allowed_root(path)
  expanded = File.expand_path(path)
  root = MISE_INSTALL_ALLOWED_ROOTS.find do |candidate|
    expanded == candidate || expanded.start_with?("#{candidate}/")
  end
  helper_error("destination escapes confined writable roots: #{path}") if root.nil?
  [expanded, root]
end

def real_directory!(path, context)
  metadata = lstat_if_exists(path)
  helper_error("#{context} does not exist: #{path}") if metadata.nil?
  helper_error("#{context} is a symlink: #{path}") if metadata.symlink?
  helper_error("#{context} is not a directory: #{path}") unless metadata.directory?
  metadata
end

def preflight_directory_chain(path, create_missing:)
  expanded, root = allowed_root(path)
  real_directory!(root, "writable root")
  relative = expanded.delete_prefix(root).sub(%r{\A/}, "")
  current = root
  missing = false
  relative.split("/").reject(&:empty?).each do |component|
    helper_error("invalid destination component") if component == "." || component == ".."
    current = File.join(current, component)
    metadata = lstat_if_exists(current)
    if metadata.nil?
      helper_error("destination parent does not exist: #{current}") unless create_missing
      missing = true
      next
    end
    helper_error("destination ancestor is a symlink: #{current}") if metadata.symlink?
    helper_error("destination ancestor is not a directory: #{current}") unless metadata.directory?
    helper_error("destination ancestor appeared below a missing parent: #{current}") if missing
  end
  expanded
end

def parse_mode(value)
  text = value.to_s
  helper_error("unsupported mode #{text.inspect}") unless text.match?(/\A[0-7]{3,4}\z/)
  mode = text.to_i(8)
  helper_error("special permission bits are unsupported") if mode > 0o777
  mode
end

def parse_install_arguments(arguments)
  directory = false
  create_parents = false
  no_target_directory = false
  target_directory = nil
  mode = nil
  operands = []
  index = 0
  options = true
  while index < arguments.length
    argument = arguments[index]
    if options && argument == "--"
      options = false
    elsif options && ["-c", "--compare"].include?(argument)
      helper_error("--compare is unsupported") if argument == "--compare"
    elsif options && ["-d", "--directory"].include?(argument)
      directory = true
    elsif options && argument == "-D"
      create_parents = true
    elsif options && ["-T", "--no-target-directory"].include?(argument)
      no_target_directory = true
    elsif options && ["-m", "--mode"].include?(argument)
      index += 1
      helper_error("#{argument} requires a mode") if index >= arguments.length
      mode = parse_mode(arguments[index])
    elsif options && argument.start_with?("--mode=")
      mode = parse_mode(argument.delete_prefix("--mode="))
    elsif options && ["-t", "--target-directory"].include?(argument)
      index += 1
      helper_error("#{argument} requires a directory") if index >= arguments.length
      target_directory = arguments[index]
    elsif options && argument.start_with?("--target-directory=")
      target_directory = argument.delete_prefix("--target-directory=")
    elsif options && argument.start_with?("-")
      helper_error("unsupported option #{argument.inspect}")
    else
      operands << argument
    end
    index += 1
  end

  if directory
    helper_error("-d cannot be combined with -D, -T, or -t") if create_parents || no_target_directory || target_directory
    helper_error("-d requires at least one directory") if operands.empty?
    return { kind: :directories, mode: mode || 0o755, paths: operands }
  end

  helper_error("-D and -T cannot be combined with -t") if target_directory && (create_parents || no_target_directory)
  if target_directory
    helper_error("-t requires at least one source") if operands.empty?
    return {
      kind: :files,
      mode: mode || 0o755,
      create_parents: false,
      target_directory: target_directory,
      sources: operands,
    }
  end

  helper_error("install requires a source and destination") if operands.length < 2
  destination = operands.pop
  helper_error("-D or -T supports exactly one source") if (create_parents || no_target_directory) && operands.length != 1
  {
    kind: :files,
    mode: mode || 0o755,
    create_parents: create_parents,
    no_target_directory: no_target_directory,
    destination: destination,
    sources: operands,
  }
end

def source_file!(path)
  expanded = File.expand_path(path)
  metadata = lstat_if_exists(expanded)
  helper_error("source does not exist: #{path}") if metadata.nil?
  helper_error("source symlinks are unsupported: #{path}") if metadata.symlink?
  helper_error("source is not a regular file: #{path}") unless metadata.file?
  expanded
end

def destination_file!(path, create_parents:)
  expanded, = allowed_root(path)
  helper_error("destination cannot be a writable root") if MISE_INSTALL_ALLOWED_ROOTS.include?(expanded)
  preflight_directory_chain(File.dirname(expanded), create_missing: create_parents)
  metadata = lstat_if_exists(expanded)
  if metadata
    helper_error("destination is a symlink: #{expanded}") if metadata.symlink?
    helper_error("destination is not a regular file: #{expanded}") unless metadata.file?
  end
  expanded
end

def preflight_install(plan)
  if plan[:kind] == :directories
    return plan[:paths].map do |path|
      expanded = preflight_directory_chain(path, create_missing: true)
      metadata = lstat_if_exists(expanded)
      if metadata && (metadata.mode & 0o7777) != plan[:mode]
        helper_error("existing directory has mode #{format('%04o', metadata.mode & 0o7777)}, expected #{format('%04o', plan[:mode])}: #{expanded}")
      end
      expanded
    end
  end

  target_directory = plan[:target_directory]
  if target_directory
    target_directory, = allowed_root(target_directory)
    real_directory!(target_directory, "target directory")
  end
  if !target_directory && plan[:sources].length > 1
    target_directory, = allowed_root(plan[:destination])
    real_directory!(target_directory, "multi-source destination")
  end

  plan[:sources].map do |source|
    source = source_file!(source)
    destination = if target_directory
      File.join(target_directory, File.basename(source))
    elsif plan[:sources].length > 1
      File.join(target_directory, File.basename(source))
    elsif !plan[:no_target_directory] && lstat_if_exists(File.expand_path(plan[:destination]))&.directory?
      directory, = allowed_root(plan[:destination])
      real_directory!(directory, "destination directory")
      File.join(directory, File.basename(source))
    else
      plan[:destination]
    end
    destination = destination_file!(destination, create_parents: plan[:create_parents])
    helper_error("source and destination are identical: #{source}") if source == destination
    if File.exist?(destination) && File.identical?(source, destination)
      helper_error("source and destination identify the same file: #{source}")
    end
    [source, destination]
  end
end

def sync_directory(path)
  flags = File::RDONLY
  flags |= File::DIRECTORY if defined?(File::DIRECTORY)
  File.open(path, flags) { |directory| directory.fsync }
end

def create_directory_chain(path, final_mode)
  expanded, root = allowed_root(path)
  current = root
  components = expanded.delete_prefix(root).sub(%r{\A/}, "").split("/").reject(&:empty?)
  components.each_with_index do |component, index|
    current = File.join(current, component)
    metadata = lstat_if_exists(current)
    if metadata
      real_directory!(current, "destination directory")
      next
    end
    mode = index == components.length - 1 ? final_mode : 0o755
    Dir.mkdir(current, mode)
    real_directory!(current, "created destination directory")
    helper_error("created directory has wrong mode: #{current}") unless (File.lstat(current).mode & 0o7777) == mode
    sync_directory(File.dirname(current))
  end
end

def atomic_install(source, destination, mode, create_parents:)
  create_directory_chain(File.dirname(destination), 0o755) if create_parents
  preflight_directory_chain(File.dirname(destination), create_missing: false)
  temporary = nil
  32.times do
    candidate = File.join(File.dirname(destination), ".mise-install.#{Process.pid}.#{Random.bytes(16).unpack1('H*')}")
    begin
      flags = File::WRONLY | File::CREAT | File::EXCL
      flags |= File::NOFOLLOW if defined?(File::NOFOLLOW)
      File.open(candidate, flags, mode) do |output|
        File.open(source, "rb") { |input| IO.copy_stream(input, output) }
        output.flush
        output.fsync
      end
      temporary = candidate
      break
    rescue Errno::EEXIST
      next
    end
  end
  helper_error("could not allocate adjacent install temporary") if temporary.nil?
  begin
    metadata = File.lstat(temporary)
    helper_error("temporary is not a regular file") unless metadata.file? && !metadata.symlink?
    helper_error("temporary has wrong mode") unless (metadata.mode & 0o7777) == mode
    File.rename(temporary, destination)
    temporary = nil
    sync_directory(File.dirname(destination))
    installed = File.lstat(destination)
    helper_error("installed destination is not a regular file") unless installed.file? && !installed.symlink?
    helper_error("installed destination has wrong mode") unless (installed.mode & 0o7777) == mode
  ensure
    File.unlink(temporary) if temporary && lstat_if_exists(temporary)&.file?
  end
end

def main
  plan = parse_install_arguments(ARGV)
  operations = preflight_install(plan)
  if plan[:kind] == :directories
    operations.each { |path| create_directory_chain(path, plan[:mode]) }
  else
    operations.each do |source, destination|
      atomic_install(source, destination, plan[:mode], create_parents: plan[:create_parents])
    end
  end
rescue StandardError => error
  warn error.message
  exit 1
end

main
"####;

struct SourceInstallHelper {
    file: std::fs::File,
    _directory: tempfile::TempDir,
    executable: PathBuf,
    identity: SourceInstallHelperIdentity,
}

struct MaterializedFormulaSource {
    #[cfg(target_os = "linux")]
    file: std::fs::File,
    #[cfg(not(target_os = "linux"))]
    _directory: tempfile::TempDir,
    path: PathBuf,
    identity: SourceReadOnlyIdentity,
}

struct MaterializedShim {
    _directory: tempfile::TempDir,
    file: std::fs::File,
    path: PathBuf,
    identity: SourceReadOnlyIdentity,
}

#[derive(Clone)]
struct SourceReadOnlyIdentity {
    device: u64,
    inode: u64,
    length: u64,
    mode: u32,
    sha256: String,
}

impl MaterializedFormulaSource {
    fn new(artifact: &super::fetch::VerifiedArtifact) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;

            let file = artifact.reader()?;
            if unsafe { nix::libc::fchmod(file.as_raw_fd(), 0o400) } == -1 {
                return Err(std::io::Error::last_os_error().into());
            }
            make_inherited(&file)?;
            let path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
            let identity = SourceReadOnlyIdentity::capture_file(&file, 0o400)?;
            return Ok(Self {
                file,
                path,
                identity,
            });
        }

        #[cfg(not(target_os = "linux"))]
        {
            use std::os::unix::fs::OpenOptionsExt;

            let directory = tempfile::Builder::new()
                .prefix("mise-brew-formula-source-")
                .tempdir()?;
            let path = directory.path().join("formula.rb");
            let mut source = artifact.reader()?;
            let mut destination = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o400)
                .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
                .open(&path)?;
            std::io::copy(&mut source, &mut destination)?;
            std::io::Write::flush(&mut destination)?;
            destination.sync_all()?;
            drop(destination);
            let identity = SourceReadOnlyIdentity::capture(&path)?;
            Ok(Self {
                _directory: directory,
                path,
                identity,
            })
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn validate(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        return self.identity.validate_file(&self.file, 0o400);

        #[cfg(not(target_os = "linux"))]
        self.identity.validate(&self.path)
    }
}

impl MaterializedShim {
    fn new(contents: &[u8]) -> Result<Self> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let directory = tempfile::Builder::new()
            .prefix("mise-brew-shim-")
            .tempdir()?;
        let path = directory.path().join("shim.rb");
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o400)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(&path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        let identity = SourceReadOnlyIdentity::capture_file(&file, 0o400)?;
        Ok(Self {
            _directory: directory,
            file,
            path,
            identity,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn validate(&self) -> Result<()> {
        self.identity.validate_file(&self.file, 0o400)?;
        let path_identity = SourceReadOnlyIdentity::capture(&self.path)?;
        if path_identity.device != self.identity.device
            || path_identity.inode != self.identity.inode
            || path_identity.sha256 != self.identity.sha256
        {
            bail!("source-build shim path changed before source build completed")
        }
        Ok(())
    }
}

impl SourceReadOnlyIdentity {
    fn capture_file(file: &std::fs::File, expected_mode: u32) -> Result<Self> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let metadata = file.metadata()?;
        if !metadata.is_file() {
            bail!("retained source input is not a regular file");
        }
        let mode = metadata.permissions().mode() & 0o7777;
        if mode != expected_mode {
            bail!("retained source input has unsafe mode {mode:04o}");
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            mode,
            sha256: hash_open_file(file)?,
        })
    }

    fn capture(path: &Path) -> Result<Self> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let metadata = path.symlink_metadata()?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("materialized formula source is not a real regular file");
        }
        let mode = metadata.permissions().mode() & 0o7777;
        if mode != 0o400 {
            bail!("materialized formula source has unsafe mode {mode:04o}");
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            mode,
            sha256: crate::hash::file_hash_sha256(path, None)?,
        })
    }

    #[cfg(not(target_os = "linux"))]
    fn validate(&self, path: &Path) -> Result<()> {
        let current = Self::capture(path)?;
        if current.device != self.device
            || current.inode != self.inode
            || current.length != self.length
            || current.mode != self.mode
            || current.sha256 != self.sha256
        {
            bail!("materialized formula source identity changed before source build completed");
        }
        Ok(())
    }

    fn validate_file(&self, file: &std::fs::File, expected_mode: u32) -> Result<()> {
        let current = Self::capture_file(file, expected_mode)?;
        if current.device != self.device
            || current.inode != self.inode
            || current.length != self.length
            || current.mode != self.mode
            || current.sha256 != self.sha256
        {
            bail!("retained source input identity changed before source build completed");
        }
        Ok(())
    }
}

fn hash_open_file(file: &std::fs::File) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::FileExt;

    let mut hasher = Sha256::new();
    let mut offset = 0_u64;
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let count = file.read_at(&mut buffer, offset)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        offset += u64::try_from(count)?;
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(target_os = "linux")]
fn make_inherited(file: &std::fs::File) -> Result<()> {
    use std::os::fd::AsRawFd;

    let current = nix::fcntl::fcntl(file, nix::fcntl::FcntlArg::F_GETFD)?;
    let flags = nix::fcntl::FdFlag::from_bits_truncate(current);
    nix::fcntl::fcntl(
        file,
        nix::fcntl::FcntlArg::F_SETFD(flags & !nix::fcntl::FdFlag::FD_CLOEXEC),
    )?;
    if unsafe { nix::libc::fcntl(file.as_raw_fd(), nix::libc::F_GETFD) } & nix::libc::FD_CLOEXEC
        != 0
    {
        bail!("retained source descriptor could not be inherited")
    }
    Ok(())
}

#[derive(Clone)]
struct SourceInstallHelperIdentity {
    device: u64,
    inode: u64,
    length: u64,
    mode: u32,
    sha256: String,
}

impl SourceInstallHelper {
    fn new(ruby: &Path, build_root: &Path, keg: &Path) -> Result<Self> {
        let ruby = ruby
            .to_str()
            .ok_or_else(|| eyre::eyre!("source-build Ruby path is not UTF-8"))?;
        if ruby.chars().any(char::is_whitespace) || !Path::new(ruby).is_absolute() {
            bail!("source-build Ruby path cannot be represented by a safe shebang");
        }
        let build_root = build_root
            .to_str()
            .ok_or_else(|| eyre::eyre!("source-build root is not UTF-8"))?;
        let keg = keg
            .to_str()
            .ok_or_else(|| eyre::eyre!("source-build keg is not UTF-8"))?;
        let script = format!(
            "#!{ruby}\nMISE_INSTALL_ALLOWED_ROOTS = [{build_root}, {keg}].map {{ |path| File.expand_path(path) }}.sort_by {{ |path| -path.length }}.freeze\n{SOURCE_INSTALL_HELPER_RB}",
            build_root = serde_json::to_string(build_root)?,
            keg = serde_json::to_string(keg)?,
        );

        use std::io::Write;
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;

        let directory = tempfile::Builder::new()
            .prefix("mise-brew-install-helper-")
            .tempdir()?;
        let executable = directory.path().join("install");
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o555)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(&executable)?;
        file.write_all(script.as_bytes())?;
        if unsafe { nix::libc::fchmod(file.as_raw_fd(), 0o555) } == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
        file.sync_all()?;
        let identity = SourceInstallHelperIdentity::capture_file(&file)?;
        Ok(Self {
            file,
            _directory: directory,
            executable,
            identity,
        })
    }

    fn add_to_env(&self, env: &mut HashMap<String, String>) {
        let executable = self.executable.display().to_string();
        let path = env.get("PATH").cloned().unwrap_or_default();
        env.insert(
            "PATH".to_string(),
            format!("{}:{path}", self.directory().display()),
        );
        env.insert("INSTALL".to_string(), executable.clone());
        env.insert("INSTALL_PROGRAM".to_string(), executable.clone());
        env.insert("INSTALL_SCRIPT".to_string(), executable.clone());
        env.insert("INSTALL_DATA".to_string(), format!("{executable} -m 644"));
        env.insert("MISE_BREW_INSTALL_HELPER".to_string(), executable);
    }

    fn directory(&self) -> &Path {
        self.executable
            .parent()
            .expect("source install helper always has a parent")
    }

    fn validate(&self) -> Result<()> {
        self.identity.validate_file(&self.file)?;
        self.identity.validate_path(&self.executable)
    }

    #[cfg(target_os = "linux")]
    fn retained_fd(&self) -> Result<OwnedFd> {
        Ok(self.file.try_clone()?.into())
    }
}

impl SourceInstallHelperIdentity {
    fn capture_file(file: &std::fs::File) -> Result<Self> {
        let identity = SourceReadOnlyIdentity::capture_file(file, 0o555)?;
        Ok(Self {
            device: identity.device,
            inode: identity.inode,
            length: identity.length,
            mode: identity.mode,
            sha256: identity.sha256,
        })
    }

    fn capture_path(path: &Path) -> Result<Self> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let metadata = path.symlink_metadata()?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            bail!("source install helper is not a real regular file");
        }
        let mode = metadata.permissions().mode() & 0o7777;
        if mode != 0o555 {
            bail!("source install helper has unsafe mode {mode:04o}");
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            length: metadata.len(),
            mode,
            sha256: crate::hash::file_hash_sha256(path, None)?,
        })
    }

    fn validate_path(&self, path: &Path) -> Result<()> {
        let current = Self::capture_path(path)?;
        if current.device != self.device
            || current.inode != self.inode
            || current.length != self.length
            || current.mode != self.mode
            || current.sha256 != self.sha256
        {
            bail!("source install helper identity changed before source build completed");
        }
        Ok(())
    }

    fn validate_file(&self, file: &std::fs::File) -> Result<()> {
        let current = Self::capture_file(file)?;
        if current.device != self.device
            || current.inode != self.inode
            || current.length != self.length
            || current.mode != self.mode
            || current.sha256 != self.sha256
        {
            bail!("source install helper identity changed before source build completed");
        }
        Ok(())
    }
}

/// does this formula have a bottle that can be poured on this machine?
pub fn has_bottle(formula: &Formula) -> bool {
    // undocumented override for testing the source-build pipeline with
    // formulae that do have bottles (comma-separated names)
    if let Ok(force) = crate::env::var("MISE_SYSTEM_BREW_FORCE_SOURCE")
        && force.split(',').any(|f| f.trim() == formula.name)
    {
        return false;
    }
    formula
        .bottle_files()
        .and_then(|files| tag::select(files))
        .is_some()
}

/// why `has_bottle` is false, for log/dry-run output
pub fn missing_bottle_reason(formula: &Formula) -> String {
    match formula.bottle_files() {
        Some(files) if !files.is_empty() => {
            let mut tags: Vec<String> = files.keys().cloned().collect();
            tags.sort();
            format!("bottles exist only for: {}", tags.join(", "))
        }
        _ => "source-only formula, no bottles".to_string(),
    }
}

/// Reject early what the source builder cannot handle, with the reason —
/// checked before any work happens so dry-run and real runs fail alike.
pub fn check_buildable(formula: &Formula) -> Result<()> {
    validate_source_build_platform(&formula.name)?;
    let Some(src) = formula.stable_url() else {
        bail!("{}: formula has no stable source URL", formula.name);
    };
    if let Some(using) = &src.using {
        bail!(
            "{}: source uses the {using:?} download strategy, which mise cannot build from \
             (and no bottle exists for this machine)",
            formula.name,
        );
    }
    let Some(source_checksum) = src.checksum.as_deref() else {
        bail!("{}: source archive has no sha256 in the API", formula.name);
    };
    if !valid_sha256(source_checksum) {
        bail!("{}: source archive has an invalid sha256", formula.name);
    }
    validate_source_format(formula)?;
    // the formula .rb must be pinned to the API snapshot's commit and
    // verifiable — evaluating a newer/unverified formula against older
    // source metadata would build the wrong thing
    if formula.ruby_source_path.is_none() {
        bail!("{}: API metadata has no ruby_source_path", formula.name);
    }
    if formula.tap_git_head.is_none() {
        bail!("{}: API metadata has no tap_git_head", formula.name);
    }
    let Some(formula_checksum) = formula
        .ruby_source_checksum
        .as_ref()
        .and_then(|c| c.sha256.as_deref())
    else {
        bail!("{}: API metadata has no formula checksum", formula.name);
    };
    if !valid_sha256(formula_checksum) {
        bail!(
            "{}: API metadata has an invalid formula checksum",
            formula.name
        );
    }
    Ok(())
}

fn validate_source_format(formula: &Formula) -> Result<()> {
    let source_format = ExtractionFormat::from_file_name(&source_basename(formula));
    if !matches!(
        source_format,
        ExtractionFormat::Raw
            | ExtractionFormat::Tar
            | ExtractionFormat::TarGz
            | ExtractionFormat::TarXz
            | ExtractionFormat::TarBz2
            | ExtractionFormat::TarZst
    ) {
        bail!(
            "{}: source format {source_format} cannot be extracted from an identity-bound descriptor",
            formula.name
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_source_build_platform(name: &str) -> Result<()> {
    bail!(
        "brew:{name}: source builds are unsupported on macOS because sandbox-exec cannot contain detached descendants; install a compatible bottle"
    )
}

#[cfg(target_os = "linux")]
fn validate_source_build_platform(name: &str) -> Result<()> {
    crate::sandbox::ensure_strict_formula_execution_available(&format!("brew:{name}: source build"))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn validate_source_build_platform(name: &str) -> Result<()> {
    bail!(
        "brew:{name}: source builds are supported only on Linux with fully enforced Landlock confinement; install a compatible bottle"
    )
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Build a formula from source into its keg and link it.
pub async fn build(
    rf: &ResolvedFormula,
    closure: &[ResolvedFormula],
    lifecycle: &super::lifecycle::PreparedFormulaLifecycle,
    pr: &dyn SingleReport,
) -> Result<()> {
    let formula = &rf.formula;
    let name = &formula.name;
    pour::validate_formula_install_policy(formula)?;
    let pkg_version = formula.pkg_version()?;
    check_buildable(formula)?;
    let keg = pour::keg_path(name, &pkg_version);
    pour::prepare_formula_rack(&keg)?;
    if pour::complete_interrupted_finalization(&keg)? {
        return Ok(());
    }
    if pour::resume_source_finalization(&keg, formula.keg_only, lifecycle, pr).await? {
        return Ok(());
    }
    pr.set_message("resolve ruby".to_string());
    let ruby = ruby_bin().await?;
    let formula_artifact = fetch_formula_rb(rf, pr).await?;
    let formula_source = MaterializedFormulaSource::new(&formula_artifact)?;
    let formula_rb = formula_source.path().to_path_buf();
    let archive = fetch_source(formula, pr).await?;

    let build_root = crate::dirs::CACHE
        .join("system-brew")
        .join("build")
        .join(format!(
            "{name}-{pkg_version}-{}",
            crate::rand::random_string(32)
        ));
    match build_root.symlink_metadata() {
        Ok(_) => bail!(
            "source-build staging path already exists: {}",
            build_root.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    crate::file::create_dir_all(&build_root)?;
    let mut build_root_cleanup = OwnedBuildRoot::new(&build_root)?;
    pr.set_message("extract source".to_string());
    let buildpath = stage_source(&archive, &build_root, &source_basename(formula))?;
    let buildpath_fd = open_real_directory(&buildpath.canonicalize()?)?;
    let shim = MaterializedShim::new(SHIM_RB.as_bytes())?;
    let shim_path = shim.path().to_path_buf();
    let sandbox_home = build_root.join("home");
    let sandbox_tmp = build_root.join("tmp");
    crate::file::create_dir_all(&sandbox_home)?;
    crate::file::create_dir_all(&sandbox_tmp)?;
    let install_helper = SourceInstallHelper::new(&ruby, &build_root, &keg)?;

    let mut env = build_env(rf, closure, &pkg_version, &buildpath, &formula_rb)?;
    install_helper.add_to_env(&mut env);
    env.insert("HOME".to_string(), sandbox_home.display().to_string());
    env.insert("TMPDIR".to_string(), sandbox_tmp.display().to_string());
    env.insert("TMP".to_string(), sandbox_tmp.display().to_string());
    env.insert("TEMP".to_string(), sandbox_tmp.display().to_string());
    let sandbox_paths = SourceSandboxPaths {
        ruby: &ruby,
        formula_rb: &formula_rb,
        build_root: &build_root,
        home: &sandbox_home,
        private_tmp: &sandbox_tmp,
        env: &env,
        install_helper: &install_helper,
        shim_path: &shim_path,
        #[cfg(target_os = "linux")]
        build_root_fd: build_root_cleanup.directory_fd(),
        #[cfg(target_os = "linux")]
        formula_fd: &formula_source.file,
        #[cfg(target_os = "linux")]
        shim_fd: Some(&shim.file),
    };
    let inspection_sandbox = source_sandbox_config(&sandbox_paths, None, None)?;
    let mut inspection_env = env.clone();
    inspection_env.insert("MISE_BREW_INSPECT_ONLY".to_string(), "1".to_string());
    let mut inspection = CmdLineRunner::new(&ruby)
        .arg(&shim_path)
        .current_dir_fd(buildpath_fd.try_clone()?)
        .env_clear()
        .envs(inspection_env)
        .with_pr(pr)
        .with_sandbox(inspection_sandbox)
        .with_process_group_cleanup();
    inspection
        .apply_sandbox()
        .await
        .wrap_err_with(|| format!("failed to confine source formula inspection for {name}"))?;
    install_helper.validate()?;
    formula_source.validate()?;
    shim.validate()?;
    let inspected = inspection.execute_async().await;
    install_helper.validate()?;
    formula_source.validate()?;
    shim.validate()?;
    inspected.wrap_err_with(|| {
        format!("brew:{name}: formula uses unsupported or unsafe source-build declarations")
    })?;

    // Formulae bake the final keg path into binaries, so the build installs
    // straight into the Cellar. Authority is durable before Ruby can write it.
    let transaction = pour::begin_source_build_transaction(
        name,
        &pkg_version,
        &keg,
        pour::active_keg(name),
        super::lifecycle::prepared_identity_sha256(lifecycle)?,
    )?;
    let predecessor_keg = transaction.predecessor_keg;
    let existing_backup = transaction.existing_backup;
    // From this point until the finalizer takes ownership, every failure must
    // restore the predecessor. Keep one error boundary so newly-added fallible
    // preparation cannot accidentally strand a Building transaction.
    let prepared = async {
        let keg_fd = open_real_directory(&keg.canonicalize()?)?;
        let sandbox = source_sandbox_config(&sandbox_paths, Some(&keg), Some(&keg_fd))?;

        pr.set_message("build from source".to_string());
        let mut cmd = CmdLineRunner::new(&ruby)
            .arg(&shim_path)
            .current_dir_fd(buildpath_fd.try_clone()?)
            .env_clear()
            .envs(env.clone())
            .with_pr(pr)
            .with_sandbox(sandbox)
            .with_process_group_cleanup();
        cmd.apply_sandbox()
            .await
            .wrap_err_with(|| format!("failed to confine source build for {name}"))?;
        install_helper.validate()?;
        formula_source.validate()?;
        shim.validate()?;
        cmd.execute_async()
            .await
            .wrap_err(format!("failed to build {name} {pkg_version} from source"))?;
        install_helper.validate()?;
        formula_source.validate()?;
        shim.validate()?;
        pour::validate_source_build_transaction(&keg)?;
        match keg.symlink_metadata() {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => bail!(
                "build of {name} finished but produced no keg at {}",
                keg.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => bail!(
                "build of {name} finished but produced no keg at {}",
                keg.display()
            ),
            Err(error) => return Err(error.into()),
        }
        pour::prepare_source_build_metadata(&keg)?;

        let formula_snapshot = keg.join(".brew").join(format!("{name}.rb"));
        pour::validate_source_build_transaction(&keg)?;
        formula_artifact.publish_cache(&formula_snapshot)?;
        formula_source.validate()?;
        Ok(pour::FormulaInstallProvenance::SourceBuild {
            formula_snapshot,
            compiler: source_compiler()?,
            built_on: native_build_system_info()?,
        })
    }
    .await;
    let provenance = match prepared {
        Ok(provenance) => provenance,
        Err(error) => {
            pour::rollback_source_build_transaction(&keg)?;
            return Err(error);
        }
    };
    let host_tag = tag::host_tag();
    let report = Default::default();
    let finalized = pour::finalize_formula(pour::FormulaFinalizer {
        rf,
        tag: &host_tag,
        staged_keg: &keg,
        keg: &keg,
        report: &report,
        closure,
        provenance,
        lifecycle,
        pr,
        existing_backup,
        predecessor_keg,
    })
    .await;
    if finalized.is_ok() {
        build_root_cleanup.remove()?;
    }
    finalized
}

struct SourceSandboxPaths<'a> {
    ruby: &'a Path,
    formula_rb: &'a Path,
    build_root: &'a Path,
    home: &'a Path,
    private_tmp: &'a Path,
    env: &'a HashMap<String, String>,
    install_helper: &'a SourceInstallHelper,
    shim_path: &'a Path,
    #[cfg(target_os = "linux")]
    build_root_fd: &'a OwnedFd,
    #[cfg(target_os = "linux")]
    formula_fd: &'a std::fs::File,
    #[cfg(target_os = "linux")]
    shim_fd: Option<&'a std::fs::File>,
}

fn source_sandbox_config(
    paths: &SourceSandboxPaths<'_>,
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] writable_keg: Option<&Path>,
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] writable_keg_fd: Option<
        &OwnedFd,
    >,
) -> Result<crate::sandbox::SandboxConfig> {
    let ruby_root = paths
        .ruby
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| eyre::eyre!("source-build Ruby has no installation root"))?;
    let brew_prefix = prefix::prefix();
    let mut allow_read = vec![
        paths.build_root.to_path_buf(),
        paths.formula_rb.to_path_buf(),
        paths.shim_path.to_path_buf(),
        ruby_root.to_path_buf(),
        paths.install_helper.executable.clone(),
    ];
    if let Some(cmake_paths) = paths.env.get("CMAKE_PREFIX_PATH") {
        allow_read.extend(
            std::env::split_paths(cmake_paths)
                .filter(|path| path != &brew_prefix)
                .filter(|path| path.is_dir()),
        );
    }
    allow_read.extend(source_platform_read_paths()?);
    let mut allow_write = if writable_keg.is_some() {
        vec![paths.build_root.to_path_buf()]
    } else {
        vec![paths.home.to_path_buf(), paths.private_tmp.to_path_buf()]
    };
    allow_write.extend(writable_keg.map(Path::to_path_buf));
    let mut sandbox = crate::sandbox::SandboxConfig {
        deny_read: true,
        deny_write: true,
        deny_net: true,
        deny_local_sockets: true,
        deny_env: true,
        allow_read,
        allow_write,
        deny_system_temp_write: true,
        deny_mise_data_read: true,
        require_full_filesystem_confinement: true,
        system_access_profile: crate::sandbox::SystemAccessProfile::FormulaExecution,
        ..Default::default()
    };
    sandbox.resolve_paths();
    #[cfg(target_os = "linux")]
    {
        sandbox
            .prebind_formula_execution_read(paths.build_root, paths.build_root_fd.try_clone()?)?;
        sandbox.prebind_formula_execution_read(
            paths.formula_rb,
            paths.formula_fd.try_clone()?.into(),
        )?;
        if let Some(shim_fd) = paths.shim_fd {
            sandbox.prebind_formula_execution_read(paths.shim_path, shim_fd.try_clone()?.into())?;
        }
        sandbox.prebind_formula_execution_read(
            &paths.install_helper.executable,
            paths.install_helper.retained_fd()?,
        )?;
        if writable_keg.is_some() {
            sandbox.prebind_formula_execution_write(
                paths.build_root,
                paths.build_root_fd.try_clone()?,
            )?;
        } else {
            sandbox.prebind_formula_execution_write(
                paths.home,
                open_real_directory(&paths.home.canonicalize()?)?,
            )?;
            sandbox.prebind_formula_execution_write(
                paths.private_tmp,
                open_real_directory(&paths.private_tmp.canonicalize()?)?,
            )?;
        }
        if let (Some(keg), Some(keg_fd)) = (writable_keg, writable_keg_fd) {
            sandbox.prebind_formula_execution_write(keg, keg_fd.try_clone()?)?;
        }
        sandbox.bind_formula_execution_paths()?;
    }
    Ok(sandbox)
}

#[cfg(target_os = "macos")]
fn source_platform_read_paths() -> Result<Vec<PathBuf>> {
    let output = std::process::Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .output()
        .wrap_err("could not locate active Xcode developer directory")?;
    if !output.status.success() {
        bail!("could not locate active Xcode developer directory");
    }
    let path = PathBuf::from(String::from_utf8(output.stdout)?.trim());
    let metadata = path.symlink_metadata()?;
    if !path.is_absolute() || !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("active Xcode developer directory is not a real absolute directory");
    }
    Ok(vec![path.canonicalize()?])
}

#[cfg(not(target_os = "macos"))]
fn source_platform_read_paths() -> Result<Vec<PathBuf>> {
    Ok(vec![])
}

struct OwnedBuildRoot {
    parent: OwnedFd,
    directory: OwnedFd,
    name: OsString,
    device: nix::libc::dev_t,
    inode: nix::libc::ino_t,
    removed: bool,
}

impl OwnedBuildRoot {
    fn new(path: &Path) -> Result<Self> {
        let name = path
            .file_name()
            .filter(|name| *name != OsStr::new(".") && *name != OsStr::new(".."))
            .ok_or_else(|| eyre::eyre!("source-build staging root has no safe name"))?
            .to_owned();
        let parent_path = path
            .parent()
            .ok_or_else(|| eyre::eyre!("source-build staging root has no parent"))?
            .canonicalize()?;
        let parent = open_real_directory(&parent_path)?;
        let directory = nix::fcntl::openat(
            &parent,
            name.as_os_str(),
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )?;
        let stat = nix::sys::stat::fstat(&directory)?;
        Ok(Self {
            parent,
            directory,
            name,
            device: stat.st_dev,
            inode: stat.st_ino,
            removed: false,
        })
    }

    fn remove(&mut self) -> Result<()> {
        if self.removed {
            return Ok(());
        }
        let bound = nix::sys::stat::fstat(&self.directory)?;
        if bound.st_dev != self.device || bound.st_ino != self.inode {
            bail!("source-build staging root descriptor identity changed")
        }
        super::lifecycle::remove_run_tree_contents(&self.directory)?;
        let linked = nix::sys::stat::fstatat(
            &self.parent,
            self.name.as_os_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )?;
        let kind = nix::sys::stat::SFlag::from_bits_truncate(linked.st_mode);
        if !kind.contains(nix::sys::stat::SFlag::S_IFDIR)
            || linked.st_dev != self.device
            || linked.st_ino != self.inode
        {
            bail!("source-build staging root changed before cleanup")
        }
        nix::unistd::unlinkat(
            &self.parent,
            self.name.as_os_str(),
            nix::unistd::UnlinkatFlags::RemoveDir,
        )?;
        nix::unistd::fsync(&self.parent)?;
        self.removed = true;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn directory_fd(&self) -> &OwnedFd {
        &self.directory
    }
}

impl Drop for OwnedBuildRoot {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

fn open_real_directory(path: &Path) -> Result<OwnedFd> {
    if !path.is_absolute() {
        bail!("source-build staging parent is not absolute")
    }
    let mut directory = nix::fcntl::open(
        "/",
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )?;
    for component in path.components() {
        use std::path::Component;
        let Component::Normal(name) = component else {
            continue;
        };
        directory = nix::fcntl::openat(
            &directory,
            name,
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )?;
    }
    Ok(directory)
}

fn source_compiler() -> Result<String> {
    let output = std::process::Command::new("cc").arg("--version").output()?;
    if !output.status.success() {
        bail!("cannot determine source-build compiler")
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let version = command_output("cc", &["-dumpfullversion", "-dumpversion"]);
    parse_source_compiler(&text, version.as_deref())
}

fn parse_source_compiler(version_output: &str, dumped_version: Option<&str>) -> Result<String> {
    let text = version_output.to_lowercase();
    if text.contains("clang") {
        return Ok("clang".to_string());
    }
    if text.contains("gcc")
        || text.contains("free software foundation")
        || text.contains("gnu compiler collection")
    {
        let major = dumped_version
            .and_then(|version| version.split('.').next())
            .filter(|major| !major.is_empty() && major.chars().all(|c| c.is_ascii_digit()))
            .ok_or_else(|| eyre::eyre!("cannot determine source-build GCC major version"))?;
        return Ok(format!("gcc-{major}"));
    }
    bail!("unrecognized source-build compiler")
}

fn native_build_system_info() -> Result<serde_json::Value> {
    let os = if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    };
    let os_version = if cfg!(target_os = "macos") {
        command_output("/usr/bin/sw_vers", &["-productVersion"])
    } else {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|contents| {
                contents.lines().find_map(|line| {
                    line.strip_prefix("PRETTY_NAME=")
                        .map(|value| value.trim_matches('"').to_string())
                })
            })
    }
    .ok_or_else(|| eyre::eyre!("cannot determine source-build operating system version"))?;
    let cpu_family = command_output("uname", &["-m"])
        .ok_or_else(|| eyre::eyre!("cannot determine source-build CPU family"))?;
    Ok(serde_json::json!({
        "os": os,
        "os_version": os_version,
        "cpu_family": cpu_family,
    }))
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Ensure a mise-managed ruby is installed (precompiled by default) and
/// return the path to its `ruby` executable.
pub(crate) async fn ruby_bin() -> Result<PathBuf> {
    let mut config = Config::get().await?;
    let tool: crate::cli::args::ToolArg = "ruby".parse()?;
    let mut ts = ToolsetBuilder::new()
        .with_args(&[tool])
        .with_default_to_latest(true)
        .build(&config)
        .await?;
    ts.install_missing_versions(
        &mut config,
        &InstallOptions {
            // only ruby — never drag the rest of the config's toolset in
            missing_args_only: true,
            reason: "brew source build".to_string(),
            ..Default::default()
        },
    )
    .await?;
    for (backend, tv) in ts.list_current_versions() {
        if tv.ba().short != "ruby" {
            continue;
        }
        for bin_dir in backend.list_bin_paths(&config, &tv).await? {
            let ruby = bin_dir.join("ruby");
            if ruby.is_file() {
                return Ok(ruby);
            }
        }
    }
    bail!("failed to provision ruby for building from source (try `mise install ruby`)");
}

/// Download the formula's .rb from homebrew/core, pinned to the commit the
/// API metadata was generated from and verified against its sha256.
async fn fetch_formula_rb(
    rf: &ResolvedFormula,
    pr: &dyn SingleReport,
) -> Result<super::fetch::VerifiedArtifact> {
    let formula = &rf.formula;
    // all guaranteed present by check_buildable
    let rb_path = formula.ruby_source_path.as_ref().unwrap();
    let sha256 = formula
        .ruby_source_checksum
        .as_ref()
        .and_then(|c| c.sha256.as_deref())
        .unwrap();
    let commit = formula.tap_git_head.as_deref().unwrap();
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("formula");
    let dest = cache_dir.join(format!("{}-{}.rb", formula.name, &sha256[..12]));
    match dest.symlink_metadata() {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if let Some(verified) =
                super::fetch::VerifiedArtifact::from_path(&dest, sha256, Some(pr))?
            {
                return Ok(verified);
            }
        }
        Ok(_) => bail!(
            "formula cache entry is not a real regular file: {}",
            dest.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let raw_base = rf
        .tap_raw_base
        .as_deref()
        .map(|base| base.trim_end_matches("/HEAD"))
        .unwrap_or(HOMEBREW_CORE_RAW);
    let url = format!("{raw_base}/{commit}/{rb_path}");
    pr.set_message(format!("download {rb_path}"));
    let response = HTTP_FETCH.get_async(&url).await?;
    let verified =
        super::fetch::VerifiedArtifact::from_response(response, &dest, sha256, Some(pr)).await?;
    verified.publish_cache(&dest)?;
    Ok(verified)
}

/// Download the stable source archive, verified against the API's sha256.
/// the source archive's upstream file name
fn source_basename(formula: &Formula) -> String {
    formula
        .stable_url()
        .map(|src| src.url.as_str())
        .and_then(|url| url.rsplit('/').next())
        .filter(|b| !b.is_empty())
        .unwrap_or("source")
        .to_string()
}

async fn fetch_source(
    formula: &Formula,
    pr: &dyn SingleReport,
) -> Result<super::fetch::VerifiedArtifact> {
    let src = formula.stable_url().unwrap(); // check_buildable
    let sha256 = src.checksum.as_deref().unwrap(); // check_buildable
    let basename = source_basename(formula);
    let cache_dir = crate::dirs::CACHE.join("system-brew").join("sources");
    let dest = cache_dir.join(format!("{}-{basename}", &sha256[..12]));
    match dest.symlink_metadata() {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if let Some(verified) =
                super::fetch::VerifiedArtifact::from_path(&dest, sha256, Some(pr))?
            {
                debug!("source cache hit: {}", dest.display());
                return Ok(verified);
            }
        }
        Ok(_) => bail!(
            "source cache entry is not a real regular file: {}",
            dest.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    pr.set_message(format!("download {basename}"));
    let response = HTTP.get_async(&src.url).await?;
    let verified =
        super::fetch::VerifiedArtifact::from_response(response, &dest, sha256, Some(pr)).await?;
    verified.publish_cache(&dest)?;
    Ok(verified)
}

/// Unpack the source archive the way brew stages it: when the archive holds
/// a single top-level directory, that directory is the buildpath.
fn stage_source(
    archive: &super::fetch::VerifiedArtifact,
    build_root: &Path,
    basename: &str,
) -> Result<PathBuf> {
    let stage = build_root.join("src");
    crate::file::create_dir_all(&stage)?;
    // `basename` is the upstream file name — the cache entry's own name
    // carries a checksum prefix that must not leak into the build tree
    let format = ExtractionFormat::from_file_name(basename);
    if format.is_tar_archive() {
        crate::file::untar_file(
            archive.reader()?,
            archive.label(),
            &stage,
            format,
            &ExtractOptions::default(),
        )
        .wrap_err_with(|| format!("failed to extract {}", archive.label().display()))?;
    } else {
        // a bare file (script, single binary): stage it as-is
        let destination = stage.join(basename);
        let mut input = archive.reader()?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        std::io::copy(&mut input, &mut output)?;
        std::io::Write::flush(&mut output)?;
        output.sync_all()?;
    }
    let entries: Vec<PathBuf> = crate::file::ls(&stage)?.into_iter().collect();
    match entries.as_slice() {
        [single] => {
            let metadata = single.symlink_metadata()?;
            if metadata.file_type().is_symlink() {
                bail!(
                    "source archive top-level entry is a symlink: {}",
                    single.display()
                );
            }
            if metadata.is_dir() {
                let canonical_stage = stage.canonicalize()?;
                let canonical_single = single.canonicalize()?;
                if !canonical_single.starts_with(&canonical_stage) {
                    bail!("source archive escaped staging root: {}", single.display());
                }
                Ok(single.clone())
            } else {
                Ok(stage)
            }
        }
        _ => Ok(stage),
    }
}

/// The environment the formula builds in: dependency kegs first on PATH,
/// pkg-config/include/lib flags pointing into the prefix, and the shim's
/// own variables. Mirrors the spirit of brew's superenv without the
/// compiler shims.
fn build_env(
    rf: &ResolvedFormula,
    closure: &[ResolvedFormula],
    pkg_version: &str,
    buildpath: &Path,
    formula_rb: &Path,
) -> Result<HashMap<String, String>> {
    let prefix = prefix::prefix();
    // only this formula's transitive dependencies — unrelated formulae from
    // the same install batch must not leak into the build environment
    let by_name: HashMap<&str, &ResolvedFormula> = closure
        .iter()
        .flat_map(|other| {
            std::iter::once((other.formula.name.as_str(), other)).chain(
                other
                    .formula
                    .aliases
                    .iter()
                    .map(move |a| (a.as_str(), other)),
            )
        })
        .collect();
    // walk each formula's deps under the same variations tag the closure
    // resolution used (the dep's selected bottle tag, not the host's)
    let host_tag = tag::host_tag();
    let rf_tag = super::resolve::dep_tag(&rf.formula, &host_tag);
    let mut deps: Vec<&ResolvedFormula> = vec![];
    let mut seen: std::collections::HashSet<&str> =
        std::iter::once(rf.formula.name.as_str()).collect();
    let mut queue: Vec<(&ResolvedFormula, &String)> = rf
        .formula
        .dependencies_for(&rf_tag)
        .iter()
        .chain(rf.formula.build_dependencies_for(&rf_tag))
        .map(|dependency| (rf, dependency))
        .collect();
    while let Some((declaring_formula, dep)) = queue.pop() {
        let other = *by_name
            .get(super::resolve::formula_reference_name(dep))
            .ok_or_else(|| {
                eyre::eyre!(
                    "source-build dependency closure for {} is missing declared dependency {dep}",
                    declaring_formula.formula.name
                )
            })?;
        if !seen.insert(other.formula.name.as_str()) {
            continue;
        }
        deps.push(other);
        let other_tag = super::resolve::dep_tag(&other.formula, &host_tag);
        queue.extend(
            other
                .formula
                .dependencies_for(&other_tag)
                .iter()
                .map(|dependency| (other, dependency)),
        );
    }
    let dep_kegs: Vec<PathBuf> = deps
        .iter()
        .map(|other| {
            let pkg_version = other.formula.pkg_version()?;
            let keg = pour::keg_path(&other.formula.name, &pkg_version);
            super::lifecycle::validate_lifecycle_keg_ancestry(&keg).wrap_err_with(|| {
                format!(
                    "source-build dependency keg is not a complete real keg: {}",
                    keg.display()
                )
            })?;
            let health = pour::installed_formula_health(&other.formula.name, &pkg_version);
            if health.kind != pour::FormulaHealthKind::Healthy {
                bail!(
                    "source-build dependency {}/{} is not healthy: {}",
                    other.formula.name,
                    pkg_version,
                    health.reasons.join("; ")
                )
            }
            Ok(keg)
        })
        .collect::<Result<_>>()?;

    let mut path: Vec<String> = dep_kegs
        .iter()
        .map(|p| p.join("bin"))
        .filter(|p| p.is_dir())
        .map(|p| p.display().to_string())
        .collect();
    for dir in SOURCE_SYSTEM_PATH {
        path.push(dir.to_string());
    }

    let pkg_config_path: Vec<String> = dep_kegs
        .iter()
        .flat_map(|p| [p.join("lib/pkgconfig"), p.join("share/pkgconfig")])
        .filter(|p| p.is_dir())
        .map(|p| p.display().to_string())
        .collect();

    let mut cppflags: Vec<String> = vec![];
    let mut ldflags: Vec<String> = vec![];
    for dir in &dep_kegs {
        let include = dir.join("include");
        if include.is_dir() {
            cppflags.push(format!("-I{}", include.display()));
        }
        let lib = dir.join("lib");
        if lib.is_dir() {
            ldflags.push(format!("-L{}", lib.display()));
        }
    }
    if cfg!(target_os = "linux") {
        // binaries must find brewed libraries at runtime without ldconfig
        ldflags.extend(
            dep_kegs
                .iter()
                .map(|dependency| format!("-Wl,-rpath,{}", dependency.join("lib").display())),
        );
    }

    let jobs = crate::jobs::normalize(Settings::get().jobs);
    let stable_version = rf.formula.versions.stable.clone().unwrap_or_default();
    let mut env = HashMap::from(
        [
            ("MISE_BREW_PREFIX", prefix.display().to_string()),
            ("MISE_BREW_CELLAR", prefix::cellar().display().to_string()),
            ("MISE_BREW_FORMULA_FILE", formula_rb.display().to_string()),
            ("MISE_BREW_NAME", rf.formula.name.clone()),
            ("MISE_BREW_VERSION", stable_version),
            ("MISE_BREW_PKG_VERSION", pkg_version.to_string()),
            ("MISE_BREW_BUILDPATH", buildpath.display().to_string()),
            (
                "MISE_BREW_CACHE",
                crate::dirs::CACHE
                    .join("system-brew")
                    .join("downloads")
                    .display()
                    .to_string(),
            ),
            ("MISE_BREW_MAKE_JOBS", jobs.to_string()),
            ("PATH", path.join(":")),
            ("MAKEFLAGS", format!("-j{jobs}")),
            ("HOMEBREW_PREFIX", prefix.display().to_string()),
            ("HOMEBREW_CELLAR", prefix::cellar().display().to_string()),
            (
                "CMAKE_PREFIX_PATH",
                dep_kegs
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(":"),
            ),
        ]
        .map(|(k, v)| (k.to_string(), v)),
    );
    if !pkg_config_path.is_empty() {
        env.insert("PKG_CONFIG_PATH".into(), pkg_config_path.join(":"));
    }
    if !cppflags.is_empty() {
        env.insert("CPPFLAGS".into(), cppflags.join(" "));
        env.insert("CFLAGS".into(), cppflags.join(" "));
        env.insert("CXXFLAGS".into(), cppflags.join(" "));
    }
    if !ldflags.is_empty() {
        env.insert("LDFLAGS".into(), ldflags.join(" "));
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::process::Output;

    use super::super::api::{BottleFile, BottleSpec, RubySourceChecksum, SourceUrl, Versions};
    use super::*;

    fn formula(tags: &[&str]) -> Formula {
        let files: HashMap<String, BottleFile> = tags
            .iter()
            .map(|tag| {
                (
                    tag.to_string(),
                    BottleFile {
                        cellar: ":any".to_string(),
                        url: "https://example.com/bottle.tar.gz".to_string(),
                        sha256: "0".repeat(64),
                    },
                )
            })
            .collect();
        let mut bottle = HashMap::new();
        if !tags.is_empty() {
            bottle.insert("stable".to_string(), BottleSpec { files });
        }
        Formula {
            name: "test".to_string(),
            tap: None,
            aliases: vec![],
            versions: Versions {
                stable: Some("1.0.0".to_string()),
            },
            revision: 0,
            keg_only: false,
            dependencies: vec![],
            build_dependencies: vec![],
            bottle,
            variations: HashMap::new(),
            urls: HashMap::from([(
                "stable".to_string(),
                SourceUrl {
                    url: "https://example.com/test-1.0.0.tar.gz".to_string(),
                    checksum: Some("0".repeat(64)),
                    using: None,
                },
            )]),
            ruby_source_path: Some("Formula/t/test.rb".to_string()),
            ruby_source_checksum: Some(RubySourceChecksum {
                sha256: Some("1".repeat(64)),
            }),
            tap_git_head: Some("abc123".to_string()),
            post_install_steps: vec![],
            post_install_defined: false,
            install_policy: Default::default(),
        }
    }

    fn run_shim_formula(
        source: &str,
        inspect_only: bool,
    ) -> Result<Option<(tempfile::TempDir, Output, PathBuf)>> {
        let mut ruby_candidates = crate::file::ls(&crate::dirs::INSTALLS.join("ruby"))
            .unwrap_or_default()
            .into_iter()
            .map(|install| install.join("bin/ruby"))
            .collect::<Vec<_>>();
        ruby_candidates.extend(crate::file::which("ruby"));
        let Some(ruby) = ruby_candidates.into_iter().find(|ruby| {
            std::process::Command::new(ruby)
                .args([
                    "--disable-gems",
                    "-e",
                    "major, minor = RUBY_VERSION.split('.').map(&:to_i); exit((major > 3 || (major == 3 && minor >= 1)) ? 0 : 1)",
                ])
                .status()
                .is_ok_and(|status| status.success())
        }) else {
            return Ok(None);
        };
        let tmp = tempfile::tempdir()?;
        let prefix = tmp.path().join("prefix");
        let build = tmp.path().join("build");
        let cache = tmp.path().join("cache");
        crate::file::create_dir_all(&build)?;
        crate::file::create_dir_all(&cache)?;
        let shim = tmp.path().join("shim.rb");
        let formula = tmp.path().join("test.rb");
        let keg = prefix.join("Cellar/test/1.0");
        let install_helper = SourceInstallHelper::new(&ruby, &build, &keg)?;
        crate::file::write(&shim, SHIM_RB)?;
        crate::file::write(&formula, source)?;
        let mut command = std::process::Command::new(&ruby);
        command
            .arg(&shim)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
            .env("INSTALL", &install_helper.executable)
            .env("INSTALL_PROGRAM", &install_helper.executable)
            .env("INSTALL_SCRIPT", &install_helper.executable)
            .env(
                "INSTALL_DATA",
                format!("{} -m 644", install_helper.executable.display()),
            )
            .env("MISE_BREW_INSTALL_HELPER", &install_helper.executable)
            .env("MISE_BREW_PREFIX", &prefix)
            .env("MISE_BREW_CELLAR", prefix.join("Cellar"))
            .env("MISE_BREW_FORMULA_FILE", &formula)
            .env("MISE_BREW_NAME", "test")
            .env("MISE_BREW_VERSION", "1.0")
            .env("MISE_BREW_PKG_VERSION", "1.0")
            .env("MISE_BREW_BUILDPATH", &build)
            .env("MISE_BREW_CACHE", &cache)
            .env("MISE_BREW_MAKE_JOBS", "2");
        if inspect_only {
            command.env("MISE_BREW_INSPECT_ONLY", "1");
        }
        let output = command.output()?;
        install_helper.validate()?;
        Ok(Some((tmp, output, keg)))
    }

    fn shim_failure_text(output: &Output) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    #[test]
    fn test_has_bottle() {
        // the version-independent "all" tag matches every machine
        assert!(has_bottle(&formula(&["all"])));
        assert!(!has_bottle(&formula(&[])));
    }

    #[test]
    fn test_missing_bottle_reason() {
        assert_eq!(
            missing_bottle_reason(&formula(&[])),
            "source-only formula, no bottles"
        );
        assert_eq!(
            missing_bottle_reason(&formula(&["x86_64_linux", "arm64_sonoma"])),
            "bottles exist only for: arm64_sonoma, x86_64_linux"
        );
    }

    #[test]
    fn test_check_buildable() {
        let buildable = formula(&[]);
        if cfg!(target_os = "macos") {
            let error = check_buildable(&buildable).unwrap_err().to_string();
            assert!(error.contains("source builds are unsupported on macOS"));
            assert!(error.contains("install a compatible bottle"));
        } else {
            assert_eq!(
                check_buildable(&buildable).is_ok(),
                validate_source_build_platform(&buildable.name).is_ok()
            );
        }

        let mut git_source = formula(&[]);
        git_source.urls.get_mut("stable").unwrap().using = Some("git".to_string());
        assert!(check_buildable(&git_source).is_err());

        let mut no_checksum = formula(&[]);
        no_checksum.urls.get_mut("stable").unwrap().checksum = None;
        assert!(check_buildable(&no_checksum).is_err());

        let mut no_url = formula(&[]);
        no_url.urls.clear();
        assert!(check_buildable(&no_url).is_err());

        let mut short_source_checksum = formula(&[]);
        short_source_checksum
            .urls
            .get_mut("stable")
            .unwrap()
            .checksum = Some("abc".to_string());
        assert!(check_buildable(&short_source_checksum).is_err());

        let mut non_ascii_formula_checksum = formula(&[]);
        non_ascii_formula_checksum
            .ruby_source_checksum
            .as_mut()
            .unwrap()
            .sha256 = Some("é".repeat(32));
        assert!(check_buildable(&non_ascii_formula_checksum).is_err());
    }

    #[test]
    fn source_format_must_support_identity_bound_extraction() {
        let mut unsupported = formula(&[]);
        unsupported.urls.get_mut("stable").unwrap().url =
            "https://example.com/test-1.0.0.zip".to_string();
        let error = validate_source_format(&unsupported)
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot be extracted from an identity-bound descriptor"));

        let supported = formula(&[]);
        assert!(validate_source_format(&supported).is_ok());
    }

    #[test]
    fn source_platform_gate_does_not_disable_bottles() {
        let bottled = formula(&["all"]);
        assert!(has_bottle(&bottled));
        assert_eq!(
            check_buildable(&bottled).is_ok(),
            validate_source_build_platform(&bottled.name).is_ok()
        );
    }

    #[test]
    fn source_shim_stages_shared_defaults_and_defers_post_install() {
        assert!(SHIM_RB.contains("def etc = prefix + \".bottle/etc\""));
        assert!(SHIM_RB.contains("def var = prefix + \".bottle/var\""));
        assert!(!SHIM_RB.contains("formula.post_install"));
    }

    #[test]
    fn source_path_excludes_unrelated_local_tools() {
        assert!(!SOURCE_SYSTEM_PATH.contains(&"/usr/local/bin"));
        assert_eq!(SOURCE_SYSTEM_PATH[0], "/usr/bin");
    }

    #[cfg(unix)]
    fn write_healthy_native_dependency_keg(
        prefix: &Path,
        name: &str,
        version: &str,
    ) -> Result<PathBuf> {
        let keg = prefix.join("Cellar").join(name).join(version);
        crate::file::create_dir_all(keg.join(".brew"))?;
        crate::file::create_dir_all(keg.join("bin"))?;
        crate::file::create_dir_all(keg.join("lib/pkgconfig"))?;
        crate::file::write(
            keg.join(".brew").join(format!("{name}.rb")),
            "class Test < Formula\n  keg_only :versioned_formula\nend\n",
        )?;
        crate::file::write(keg.join("INSTALL_RECEIPT.json"), "{}")?;
        crate::file::write(keg.join("sbom.spdx.json"), "{}")?;
        let opt = prefix.join("opt").join(name);
        crate::file::create_dir_all(opt.parent().unwrap())?;
        crate::file::make_symlink(&PathBuf::from(format!("../Cellar/{name}/{version}")), &opt)?;
        Ok(keg)
    }

    #[cfg(unix)]
    #[test]
    fn source_dependency_environment_uses_validated_cellar_kegs_not_opt_symlinks() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut guard = crate::test::EnvVarGuard::new();
        guard.set("MISE_SYSTEM_BREW_PREFIX", &prefix);

        let mut root_formula = formula(&[]);
        root_formula.name = "root".to_string();
        root_formula.dependencies = vec!["owner/tools/dependency-alias".to_string()];
        let root = ResolvedFormula {
            formula: root_formula,
            tap_raw_base: None,
            on_request: true,
        };
        let mut dependency_formula = formula(&[]);
        dependency_formula.name = "dependency".to_string();
        dependency_formula.aliases = vec!["dependency-alias".to_string()];
        dependency_formula.versions.stable = Some("2.0".to_string());
        let dependency = ResolvedFormula {
            formula: dependency_formula,
            tap_raw_base: None,
            on_request: false,
        };
        let dependency_keg = write_healthy_native_dependency_keg(&prefix, "dependency", "2.0")?;

        let env = build_env(
            &root,
            &[root.clone(), dependency],
            "1.0.0",
            tmp.path(),
            &tmp.path().join("root.rb"),
        )?;

        assert_eq!(
            env["CMAKE_PREFIX_PATH"],
            dependency_keg.display().to_string()
        );
        let dependency_bin = dependency_keg.join("bin").display().to_string();
        assert_eq!(env["PATH"].split(':').next(), Some(dependency_bin.as_str()));
        assert!(!env["PATH"].contains("/opt/"));
        Ok(())
    }

    #[test]
    fn source_dependency_environment_rejects_missing_direct_and_transitive_closure_nodes() {
        let mut root_formula = formula(&[]);
        root_formula.name = "root".to_string();
        root_formula.dependencies = vec!["owner/tools/missing-direct".to_string()];
        let root = ResolvedFormula {
            formula: root_formula,
            tap_raw_base: None,
            on_request: true,
        };
        let direct_error = build_env(
            &root,
            std::slice::from_ref(&root),
            "1.0.0",
            Path::new("/build"),
            Path::new("/formula.rb"),
        )
        .unwrap_err()
        .to_string();
        assert!(direct_error.contains("root"));
        assert!(direct_error.contains("owner/tools/missing-direct"));

        let mut root_formula = formula(&[]);
        root_formula.name = "root".to_string();
        root_formula.dependencies = vec!["dependency".to_string()];
        let root = ResolvedFormula {
            formula: root_formula,
            tap_raw_base: None,
            on_request: true,
        };
        let mut dependency_formula = formula(&[]);
        dependency_formula.name = "dependency".to_string();
        dependency_formula.dependencies = vec!["other/tap/missing-transitive".to_string()];
        let dependency = ResolvedFormula {
            formula: dependency_formula,
            tap_raw_base: None,
            on_request: false,
        };
        let transitive_error = build_env(
            &root,
            &[root.clone(), dependency],
            "1.0.0",
            Path::new("/build"),
            Path::new("/formula.rb"),
        )
        .unwrap_err()
        .to_string();
        assert!(transitive_error.contains("dependency"));
        assert!(transitive_error.contains("other/tap/missing-transitive"));
    }

    #[cfg(unix)]
    #[test]
    fn source_dependency_environment_rejects_malformed_installed_provenance() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let prefix = crate::file::desymlink_path(&tmp.path().join("prefix"));
        let mut guard = crate::test::EnvVarGuard::new();
        guard.set("MISE_SYSTEM_BREW_PREFIX", &prefix);

        let mut root_formula = formula(&[]);
        root_formula.name = "root".to_string();
        root_formula.dependencies = vec!["dependency".to_string()];
        let root = ResolvedFormula {
            formula: root_formula,
            tap_raw_base: None,
            on_request: true,
        };
        let mut dependency_formula = formula(&[]);
        dependency_formula.name = "dependency".to_string();
        dependency_formula.versions.stable = Some("2.0".to_string());
        let dependency = ResolvedFormula {
            formula: dependency_formula,
            tap_raw_base: None,
            on_request: false,
        };
        let dependency_keg = write_healthy_native_dependency_keg(&prefix, "dependency", "2.0")?;
        crate::file::write(dependency_keg.join("INSTALL_RECEIPT.json"), "not-json")?;

        let error = build_env(
            &root,
            &[root.clone(), dependency],
            "1.0.0",
            tmp.path(),
            &tmp.path().join("root.rb"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("dependency/2.0 is not healthy"));
        assert!(error.contains("receipt/provenance is missing or malformed"));
        Ok(())
    }

    #[test]
    fn source_compiler_matches_homebrew_receipt_names() {
        assert_eq!(
            parse_source_compiler(
                "cc (Ubuntu 13.3.0-6ubuntu2~24.04) 13.3.0\nCopyright (C) Free Software Foundation, Inc.",
                Some("13.3.0")
            )
            .unwrap(),
            "gcc-13"
        );
        assert_eq!(
            parse_source_compiler("Apple clang version 21.0.0", Some("21.0.0")).unwrap(),
            "clang"
        );
        assert!(parse_source_compiler("Tiny C Compiler", Some("0.9.27")).is_err());
        assert!(parse_source_compiler("gcc", Some("unknown")).is_err());
    }

    #[test]
    fn source_shim_preserves_exact_inreplace_and_append_semantics() -> Result<()> {
        let Some((_tmp, output, keg)) = run_shim_formula(
            r###"
class Test < Formula
  def install
    value = buildpath + "value"
    value.write("x x")
    inreplace(value, "x", "y", global: false)
    (prefix + "result").write(value.read)
  end
end
"###,
            false,
        )?
        else {
            return Ok(());
        };
        assert!(output.status.success(), "{}", shim_failure_text(&output));
        assert_eq!(crate::file::read_to_string(keg.join("result"))?, "y x");

        let Some((_tmp, output, _)) = run_shim_formula(
            r#"
class Test < Formula
  def install
    (buildpath + "missing").append_lines("value")
  end
end
"#,
            false,
        )?
        else {
            return Ok(());
        };
        assert!(!output.status.success());
        assert!(shim_failure_text(&output).contains("Cannot append file that doesn't exist"));
        Ok(())
    }

    #[test]
    fn source_shim_fails_closed_on_ambiguous_formula_behavior() -> Result<()> {
        let cases = [
            (
                r#"class Test < Formula
  mystery_install_policy true
end
"#,
                true,
                "unknown formula DSL",
            ),
            (
                r#"class Test < Formula
  def install
    ENV.mystery_build_environment
  end
end
"#,
                false,
                "exact Homebrew build-environment semantics are not implemented",
            ),
            (
                r#"class Test < Formula
  def install
    deps.each { |dep| puts dep }
  end
end
"#,
                false,
                "typed Dependency objects are not implemented",
            ),
            (
                r#"class Test < Formula
  def install
    Version.new("1.0-alpha") < Version.new("1.0")
  end
end
"#,
                false,
                "opaque version comparison",
            ),
            (
                r#"class Test < Formula
  MacOS::Xcode.installed?
end
"#,
                true,
                "exact Xcode detection is not implemented",
            ),
            (
                r#"class Test < Formula
  disable! date: "2020-01-01", because: :unmaintained
end
"#,
                true,
                "disabled formula (unmaintained)",
            ),
        ];
        for (source, inspect_only, expected) in cases {
            let Some((_tmp, output, _)) = run_shim_formula(source, inspect_only)? else {
                return Ok(());
            };
            assert!(
                !output.status.success(),
                "case unexpectedly succeeded: {source}"
            );
            assert!(
                shim_failure_text(&output).contains(expected),
                "missing {expected:?}: {}",
                shim_failure_text(&output)
            );
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn source_install_helper_supports_audited_gnu_install_subset() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let Some((_tmp, output, keg)) = run_shim_formula(
            r###"
class Test < Formula
  def install
    program = buildpath + "hello"
    program.write("#!/bin/sh\necho hello\n")
    system "install", "-d", "-m", "755", bin
    system "install", "-c", "-m", "755", program, bin

    data = buildpath + "hello.txt"
    data.write("hello data\n")
    system "install", "-D", "-m", "644", data, share + "hello/hello.txt"
  end
end
"###,
            false,
        )?
        else {
            return Ok(());
        };
        assert!(output.status.success(), "{}", shim_failure_text(&output));
        assert_eq!(
            crate::file::read_to_string(keg.join("bin/hello"))?,
            "#!/bin/sh\necho hello\n"
        );
        assert_eq!(
            keg.join("bin/hello").metadata()?.permissions().mode() & 0o7777,
            0o755
        );
        assert_eq!(
            crate::file::read_to_string(keg.join("share/hello/hello.txt"))?,
            "hello data\n"
        );
        assert_eq!(
            keg.join("share/hello/hello.txt")
                .metadata()?
                .permissions()
                .mode()
                & 0o7777,
            0o644
        );
        Ok(())
    }

    #[test]
    fn source_shim_rejects_ensure_executable_before_inode_replacement() -> Result<()> {
        let Some((tmp, output, _keg)) = run_shim_formula(
            r#"
class Test < Formula
  def install
    target = buildpath + "mode-target"
    target.write("mode\n")
    target.ensure_executable!
  end
end
"#,
            false,
        )?
        else {
            return Ok(());
        };
        assert!(!output.status.success());
        assert!(
            shim_failure_text(&output)
                .contains("Pathname#ensure_executable! requires inode-preserving chmod")
        );
        let target = tmp.path().join("build/mode-target");
        assert_eq!(crate::file::read_to_string(target)?, "mode\n");
        Ok(())
    }

    #[test]
    fn source_install_helper_rejects_unsupported_options_before_mutation() -> Result<()> {
        let Some((_tmp, output, keg)) = run_shim_formula(
            r#"
class Test < Formula
  def install
    source = buildpath + "payload"
    source.write("payload")
    system "install", "--owner=root", "-D", source, prefix + "new/deep/payload"
  end
end
"#,
            false,
        )?
        else {
            return Ok(());
        };
        assert!(!output.status.success());
        assert!(shim_failure_text(&output).contains("unsupported option"));
        assert!(keg.join("new").symlink_metadata().is_err());
        Ok(())
    }

    #[test]
    fn source_install_helper_rejects_destinations_outside_writable_roots() -> Result<()> {
        let Some((tmp, output, _keg)) = run_shim_formula(
            r#"
class Test < Formula
  def install
    source = buildpath + "payload"
    source.write("payload")
    system "install", "-D", source, buildpath.dirname + "outside/payload"
  end
end
"#,
            false,
        )?
        else {
            return Ok(());
        };
        assert!(!output.status.success());
        assert!(shim_failure_text(&output).contains("escapes confined writable roots"));
        assert!(tmp.path().join("outside").symlink_metadata().is_err());
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn sealed_formula_descriptor_runs_inside_strict_formula_sandbox() -> Result<()> {
        let Some(ruby) = crate::file::which("ruby").into_iter().next() else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let cached_formula = tmp.path().join("formula-cache.rb");
        crate::file::write(
            &cached_formula,
            "class Test < Formula\n  def install; end\nend\n",
        )?;
        let checksum = crate::hash::file_hash_sha256(&cached_formula, None)?;
        let artifact =
            super::super::fetch::VerifiedArtifact::from_path(&cached_formula, &checksum, None)?
                .expect("verified formula artifact");
        let formula_source = MaterializedFormulaSource::new(&artifact)?;
        let shim = MaterializedShim::new(SHIM_RB.as_bytes())?;
        let build = tmp.path().join("build");
        let home = build.join("home");
        let private_tmp = build.join("tmp");
        let keg = tmp.path().join("keg");
        for directory in [&build, &home, &private_tmp, &keg] {
            crate::file::create_dir_all(directory)?;
        }
        let build_fd = open_real_directory(&build.canonicalize()?)?;
        let helper = SourceInstallHelper::new(&ruby, &build, &keg)?;
        let mut env = HashMap::from([
            ("PATH".to_string(), SOURCE_SYSTEM_PATH.join(":")),
            ("CMAKE_PREFIX_PATH".to_string(), String::new()),
            (
                "MISE_BREW_PREFIX".to_string(),
                prefix::prefix().display().to_string(),
            ),
            (
                "MISE_BREW_CELLAR".to_string(),
                prefix::cellar().display().to_string(),
            ),
            (
                "MISE_BREW_FORMULA_FILE".to_string(),
                formula_source.path().display().to_string(),
            ),
            ("MISE_BREW_NAME".to_string(), "test".to_string()),
            ("MISE_BREW_VERSION".to_string(), "1.0".to_string()),
            ("MISE_BREW_PKG_VERSION".to_string(), "1.0".to_string()),
            (
                "MISE_BREW_BUILDPATH".to_string(),
                build.display().to_string(),
            ),
            (
                "MISE_BREW_CACHE".to_string(),
                tmp.path().join("cache").display().to_string(),
            ),
            ("MISE_BREW_MAKE_JOBS".to_string(), "1".to_string()),
            ("MISE_BREW_INSPECT_ONLY".to_string(), "1".to_string()),
            ("HOME".to_string(), home.display().to_string()),
            ("TMPDIR".to_string(), private_tmp.display().to_string()),
            ("TMP".to_string(), private_tmp.display().to_string()),
            ("TEMP".to_string(), private_tmp.display().to_string()),
        ]);
        helper.add_to_env(&mut env);
        let sandbox_paths = SourceSandboxPaths {
            ruby: &ruby,
            formula_rb: formula_source.path(),
            build_root: &build,
            home: &home,
            private_tmp: &private_tmp,
            env: &env,
            install_helper: &helper,
            shim_path: shim.path(),
            build_root_fd: &build_fd,
            formula_fd: &formula_source.file,
            shim_fd: Some(&shim.file),
        };
        let sandbox = source_sandbox_config(&sandbox_paths, None, None)?;
        let mut runner = CmdLineRunner::new(&ruby)
            .arg(shim.path())
            .current_dir_fd(build_fd.try_clone()?)
            .env_clear()
            .envs(env)
            .with_sandbox(sandbox)
            .with_process_group_cleanup();

        let expected =
            crate::sandbox::ensure_strict_formula_execution_available("formula-execution sandbox")
                .map_err(|error| error.to_string());
        if let Err(error) = runner.apply_sandbox().await {
            assert_eq!(Err(error.to_string()), expected);
            return Ok(());
        }
        expected.map_err(eyre::Report::msg)?;
        runner.execute_async().await?;
        formula_source.validate()?;
        shim.validate()?;
        helper.validate()?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn source_install_helper_runs_inside_strict_formula_sandbox() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let Some(ruby) = crate::file::which("ruby").into_iter().next() else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let build = tmp.path().join("build");
        let keg = tmp.path().join("keg");
        let home = build.join("home");
        let private_tmp = build.join("tmp");
        for directory in [&build, &keg, &home, &private_tmp] {
            crate::file::create_dir_all(directory)?;
        }
        let source = build.join("hello");
        let destination = keg.join("bin/hello");
        let formula_rb = build.join("test.rb");
        crate::file::write(&source, "#!/bin/sh\necho hello\n")?;
        crate::file::write(&formula_rb, "class Test; end\n")?;
        let helper = SourceInstallHelper::new(&ruby, &build, &keg)?;
        let formula_file = std::fs::File::open(&formula_rb)?;
        let build_fd = open_real_directory(&build.canonicalize()?)?;
        let keg_fd = open_real_directory(&keg.canonicalize()?)?;
        let mut env = HashMap::from([
            ("PATH".to_string(), SOURCE_SYSTEM_PATH.join(":")),
            ("CMAKE_PREFIX_PATH".to_string(), String::new()),
        ]);
        helper.add_to_env(&mut env);
        let sandbox_paths = SourceSandboxPaths {
            ruby: &ruby,
            formula_rb: &formula_rb,
            build_root: &build,
            home: &home,
            private_tmp: &private_tmp,
            env: &env,
            install_helper: &helper,
            shim_path: &helper.executable,
            build_root_fd: &build_fd,
            formula_fd: &formula_file,
            shim_fd: None,
        };
        let sandbox = source_sandbox_config(&sandbox_paths, Some(&keg), Some(&keg_fd))?;
        assert!(sandbox.allow_read.contains(&helper.executable));
        let driver = build.join("driver.rb");
        crate::file::write(
            &driver,
            r#"
begin
  File.open(ENV.fetch("INSTALL"), "wb") { |file| file.write("foreign") }
  abort "formula mutated immutable install helper"
rescue Errno::EACCES, Errno::EPERM
end
exit(system(ENV.fetch("INSTALL"), "-D", "-m", "755", ARGV.fetch(0), ARGV.fetch(1)) ? 0 : 1)
"#,
        )?;
        let mut runner = CmdLineRunner::new(&ruby)
            .arg(&driver)
            .args([source.to_str().unwrap(), destination.to_str().unwrap()])
            .current_dir_fd(build_fd.try_clone()?)
            .env_clear()
            .envs(env)
            .with_sandbox(sandbox)
            .with_process_group_cleanup();

        let expected =
            crate::sandbox::ensure_strict_formula_execution_available("formula-execution sandbox")
                .map_err(|error| error.to_string());
        if let Err(error) = runner.apply_sandbox().await {
            assert_eq!(Err(error.to_string()), expected);
            return Ok(());
        }
        expected.map_err(eyre::Report::msg)?;
        runner.execute_async().await?;
        helper.validate()?;
        assert_eq!(
            crate::file::read_to_string(&destination)?,
            "#!/bin/sh\necho hello\n"
        );
        assert_eq!(destination.metadata()?.permissions().mode() & 0o7777, 0o755);
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn materialized_formula_source_rejects_replacement() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let cached = tmp.path().join("formula-cache.rb");
        crate::file::write(&cached, "class Test < Formula; end\n")?;
        let sha256 = crate::hash::file_hash_sha256(&cached, None)?;
        let artifact = super::super::fetch::VerifiedArtifact::from_path(&cached, &sha256, None)?
            .expect("verified test artifact");
        let source = MaterializedFormulaSource::new(&artifact)?;
        std::fs::remove_file(source.path())?;
        crate::file::write(source.path(), "class Foreign; end\n")?;
        assert!(source.validate().is_err());
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn materialized_formula_source_uses_inherited_sealed_descriptor() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let cached = tmp.path().join("formula-cache.rb");
        crate::file::write(&cached, "class Test < Formula; end\n")?;
        let sha256 = crate::hash::file_hash_sha256(&cached, None)?;
        let artifact = super::super::fetch::VerifiedArtifact::from_path(&cached, &sha256, None)?
            .expect("verified test artifact");
        let source = MaterializedFormulaSource::new(&artifact)?;
        assert_eq!(
            crate::file::read_to_string(source.path())?,
            "class Test < Formula; end\n"
        );
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(source.path())
                .is_err()
        );
        source.validate()?;
        Ok(())
    }

    #[test]
    fn raw_source_staging_reads_retained_verified_descriptor() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let cached = tmp.path().join("source.sh");
        crate::file::write(&cached, "original\n")?;
        let sha256 = crate::hash::file_hash_sha256(&cached, None)?;
        let artifact = super::super::fetch::VerifiedArtifact::from_path(&cached, &sha256, None)?
            .expect("verified test artifact");
        crate::file::write(&cached, "replacement\n")?;
        let build = tmp.path().join("build");
        crate::file::create_dir_all(&build)?;
        stage_source(&artifact, &build, "source.sh")?;
        assert_eq!(
            crate::file::read_to_string(build.join("src/source.sh"))?,
            "original\n"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn source_install_helper_identity_rejects_replacement() -> Result<()> {
        let Some(ruby) = crate::file::which("ruby").into_iter().next() else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let build = tmp.path().join("build");
        let keg = tmp.path().join("keg");
        crate::file::create_dir_all(&build)?;
        let helper = SourceInstallHelper::new(&ruby, &build, &keg)?;
        std::fs::remove_file(&helper.executable)?;
        crate::file::write(&helper.executable, "#!/bin/sh\nexit 0\n")?;
        assert!(helper.validate().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn source_install_helper_has_exact_mode_under_restrictive_umask() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        struct UmaskGuard(nix::libc::mode_t);
        impl Drop for UmaskGuard {
            fn drop(&mut self) {
                unsafe { nix::libc::umask(self.0) };
            }
        }

        let Some(ruby) = crate::file::which("ruby").into_iter().next() else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let build = tmp.path().join("build");
        let keg = tmp.path().join("keg");
        crate::file::create_dir_all(&build)?;
        let _umask = UmaskGuard(unsafe { nix::libc::umask(0o077) });

        let helper = SourceInstallHelper::new(&ruby, &build, &keg)?;

        assert_eq!(
            helper.executable.metadata()?.permissions().mode() & 0o7777,
            0o555
        );
        helper.validate()?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_install_helper_is_read_only_and_identity_bound() -> Result<()> {
        let Some(ruby) = crate::file::which("ruby").into_iter().next() else {
            return Ok(());
        };
        let tmp = tempfile::tempdir()?;
        let build = tmp.path().join("build");
        let keg = tmp.path().join("keg");
        crate::file::create_dir_all(&build)?;
        let helper = SourceInstallHelper::new(&ruby, &build, &keg)?;
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&helper.executable)
                .is_err()
        );
        helper.validate()?;
        Ok(())
    }

    #[test]
    fn owned_build_root_is_cleaned_after_failure_scope() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let build_root = tmp.path().join("build");
        crate::file::create_dir_all(&build_root)?;
        {
            let _owned = OwnedBuildRoot::new(&build_root)?;
            crate::file::write(build_root.join("partial"), "partial")?;
        }
        assert!(build_root.symlink_metadata().is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn owned_build_root_never_removes_replacement_directory() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let build_root = tmp.path().join("build");
        crate::file::create_dir_all(&build_root)?;
        crate::file::write(build_root.join("partial"), "partial")?;
        let owned = OwnedBuildRoot::new(&build_root)?;
        let old_build = tmp.path().join("old-build");
        crate::file::rename(&build_root, &old_build)?;
        crate::file::create_dir_all(&build_root)?;
        crate::file::write(build_root.join("foreign"), "foreign")?;
        drop(owned);
        assert_eq!(
            crate::file::read_to_string(build_root.join("foreign"))?,
            "foreign"
        );
        assert!(old_build.join("partial").symlink_metadata().is_err());
        assert!(old_build.is_dir());
        Ok(())
    }

    #[test]
    fn owned_build_root_uses_retained_parent_after_ancestor_swap() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let parent = tmp.path().join("parent");
        let build_root = parent.join("build");
        crate::file::create_dir_all(&build_root)?;
        crate::file::write(build_root.join("partial"), "partial")?;
        let owned = OwnedBuildRoot::new(&build_root)?;

        let old_parent = tmp.path().join("old-parent");
        crate::file::rename(&parent, &old_parent)?;
        crate::file::create_dir_all(&build_root)?;
        crate::file::write(build_root.join("foreign"), "foreign")?;
        drop(owned);

        assert_eq!(
            crate::file::read_to_string(build_root.join("foreign"))?,
            "foreign"
        );
        assert!(old_parent.join("build").symlink_metadata().is_err());
        Ok(())
    }
}
