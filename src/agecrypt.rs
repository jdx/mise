//! age encryption shared by encrypted tracked files and the
//! experimental `[env]` age directives (`mise set --age-encrypt`, decrypted
//! when the environment is resolved).
//!
//! This module is the stable core: identity discovery (`MISE_AGE_KEY`, the
//! `age.*` settings, `~/.config/mise/age.txt`, the default SSH keys),
//! recipient parsing and byte-level encryption.
//! Nothing here is gated behind `settings.experimental`. The directive layer
//! in `directive` keeps its own gate and its own wording.

use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use age::ssh;
use age::{Decryptor, Encryptor, Identity, IdentityFile, Recipient};
use eyre::{Result, bail, eyre};
use indexmap::IndexSet;

use crate::config::Settings;
use crate::file::{self, display_path, replace_path};
use crate::{dirs, env};

mod directive;
pub(crate) use directive::{
    create_age_directive, decrypt_age_directive, load_recipients_from_defaults,
    load_recipients_from_key_file, load_ssh_recipient_from_path,
};

pub(crate) const ZSTD_COMPRESSION_LEVEL: i32 = 3;

/// Why an SSH identity mise loaded cannot actually decrypt anything.
///
/// `ssh::Identity::from_buffer` succeeds for these keys, so they look loaded,
/// but age maps them to `None` when matching stanzas. The decryptor then
/// reports "No matching keys found", which blames the recipient when the real
/// problem is that the identity could not be read at all.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UnusableSshIdentity {
    Passphrase,
    EncryptedPem,
    EncryptedCipher(String),
    Hardware(String),
    KeyType(String),
}

impl UnusableSshIdentity {
    fn classify(identity: &ssh::Identity) -> Option<Self> {
        match identity {
            ssh::Identity::Unencrypted(_) => None,
            ssh::Identity::Encrypted(_) => Some(Self::Passphrase),
            ssh::Identity::Unsupported(key) => Some(match key {
                ssh::UnsupportedKey::EncryptedPem => Self::EncryptedPem,
                ssh::UnsupportedKey::EncryptedSsh(cipher) => Self::EncryptedCipher(cipher.clone()),
                ssh::UnsupportedKey::Hardware(kind) => Self::Hardware(kind.clone()),
                ssh::UnsupportedKey::Type(kind) => Self::KeyType(kind.clone()),
            }),
        }
    }

    fn reason(&self) -> String {
        match self {
            Self::Passphrase => {
                "is protected by a passphrase, which mise does not prompt for".to_string()
            }
            Self::EncryptedPem => "is an encrypted PEM key, a format age cannot read".to_string(),
            Self::EncryptedCipher(cipher) => {
                format!("is encrypted with {cipher}, a cipher age does not support")
            }
            Self::Hardware(kind) => {
                format!("is a {kind} key held on a hardware security key")
            }
            Self::KeyType(kind) => format!("is a {kind} key, a type age does not support"),
        }
    }
}

/// Explain identities that were loaded but could not participate in decryption.
///
/// Only worth saying once decryption has already failed: an unreadable key
/// sitting beside a working one costs the user nothing.
pub(crate) fn unusable_identity_hint(unusable: &[(PathBuf, UnusableSshIdentity)]) -> String {
    if unusable.is_empty() {
        return String::new();
    }
    let mut hint = String::new();
    for (path, reason) in unusable {
        hint.push_str(&format!(
            "\nhint: {} {}, so it could not be used",
            display_path(path),
            reason.reason()
        ));
    }
    hint.push_str(
        "\nhint: use an identity mise can read, or set settings.age.key_file to an age key",
    );
    hint
}

/// Every identity this machine can decrypt with, in the order they were
/// found, plus what is known about the ones that cannot.
#[derive(Default)]
pub(crate) struct LoadedIdentities {
    pub identities: Vec<Box<dyn Identity + Send + Sync>>,
    pub unusable: Vec<(PathBuf, UnusableSshIdentity)>,
    /// The public keys of the x25519 identities among them (`age1…`), so a
    /// caller can tell whether this machine is among a set of recipients.
    pub x25519_public: Vec<String>,
    pub plugins: usize,
}

/// Why `decrypt_bytes` could not produce the plaintext.
#[derive(Debug)]
pub(crate) enum DecryptError {
    /// No identity at all: nothing to try.
    NoIdentities,
    /// Identities were tried and none matched.
    Failed { error: String, hint: String },
    /// Not an age file, or a damaged one.
    Corrupt(String),
}

impl std::fmt::Display for DecryptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoIdentities => write!(f, "no age identity is available on this machine"),
            Self::Failed { error, hint } => write!(f, "{error}{hint}"),
            Self::Corrupt(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for DecryptError {}

pub(crate) const MAX_ENCRYPTED_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_PLAINTEXT_BYTES: u64 = 1024 * 1024 * 1024;

pub(crate) fn read_bounded(reader: impl Read, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other(
            "encrypted content exceeds the size limit",
        ));
    }
    Ok(bytes)
}

/// zstd-compressed, then age-encrypted for `recipients`.
pub(crate) fn encrypt_bytes(
    plaintext: &[u8],
    recipients: &[Box<dyn Recipient + Send>],
) -> Result<Vec<u8>> {
    if recipients.is_empty() {
        bail!("no age recipients to encrypt for");
    }
    if plaintext.len() as u64 > MAX_PLAINTEXT_BYTES {
        bail!("plaintext exceeds the size limit");
    }
    let compressed = zstd::encode_all(plaintext, ZSTD_COMPRESSION_LEVEL)?;
    if compressed.len() as u64 > MAX_ENCRYPTED_BYTES {
        bail!("compressed payload exceeds the size limit");
    }
    let encryptor =
        Encryptor::with_recipients(recipients.iter().map(|r| r.as_ref() as &dyn Recipient))
            .map_err(|e| eyre!("creating the age encryptor: {e}"))?;
    let mut out = Vec::new();
    let mut writer = encryptor.wrap_output(&mut out)?;
    writer.write_all(&compressed)?;
    writer.finish()?;
    if out.len() as u64 > MAX_ENCRYPTED_BYTES {
        bail!("encrypted payload exceeds the size limit");
    }
    Ok(out)
}

pub(crate) async fn decrypt_bytes_mode(
    ciphertext: &[u8],
    interactive: bool,
) -> Result<Vec<u8>, DecryptError> {
    if ciphertext.len() as u64 > MAX_ENCRYPTED_BYTES {
        return Err(DecryptError::Corrupt(
            "encrypted payload exceeds the size limit".into(),
        ));
    }
    let loaded = load_identities(interactive).await;
    if loaded.identities.is_empty() {
        if loaded.plugins > 0 {
            return Err(DecryptError::Failed { error: "hardware identity requires an interactive restore with its age plugin installed".into(), hint: String::new() });
        }
        return Err(DecryptError::NoIdentities);
    }
    let decryptor = Decryptor::new(ciphertext)
        .map_err(|e| DecryptError::Corrupt(format!("not an age file: {e}")))?;
    let refs: Vec<&dyn Identity> = loaded
        .identities
        .iter()
        .map(|i| i.as_ref() as &dyn Identity)
        .collect();
    let mut reader = decryptor
        .decrypt(refs.into_iter())
        .map_err(|e| DecryptError::Failed {
            error: e.to_string(),
            hint: unusable_identity_hint(&loaded.unusable),
        })?;
    let compressed = read_bounded(&mut reader, MAX_ENCRYPTED_BYTES)
        .map_err(|e| DecryptError::Corrupt(format!("reading the age payload: {e}")))?;
    let decoder = zstd::stream::read::Decoder::new(&compressed[..])
        .map_err(|e| DecryptError::Corrupt(format!("decompressing the payload: {e}")))?;
    read_bounded(decoder, MAX_PLAINTEXT_BYTES)
        .map_err(|e| DecryptError::Corrupt(format!("decompressing the payload: {e}")))
}

/// The recipients this machine's own identities imply: the public key of
/// every x25519 identity in the key file, and the `.pub` of every default
/// SSH key. Empty when there is none; not an error.
pub(crate) async fn default_recipient_strings() -> Result<Vec<String>> {
    let mut recipients: IndexSet<String> = IndexSet::new();
    if let Some(key_file) = get_default_key_file().await
        && key_file.exists()
    {
        let content = file::read_to_string(&key_file)?;
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("AGE-SECRET-KEY-")
                && let Ok(identity) = line.parse::<age::x25519::Identity>()
            {
                recipients.insert(identity.to_public().to_string());
            }
        }
    }
    for path in get_default_ssh_key_paths() {
        if path.exists()
            && let Ok(recipient) = ssh_public_key_for_private(&path).await
        {
            recipients.insert(recipient);
        }
    }
    Ok(recipients.into_iter().collect())
}

/// `age1…` or `ssh-…`; `None` for anything else.
pub(crate) fn parse_recipient(recipient_str: &str) -> Result<Option<Box<dyn Recipient + Send>>> {
    parse_recipient_mode(recipient_str, true)
}

pub(crate) fn parse_recipient_mode(
    recipient_str: &str,
    interactive: bool,
) -> Result<Option<Box<dyn Recipient + Send>>> {
    let trimmed = recipient_str.trim();
    if trimmed.starts_with("age1tag1") {
        return trimmed
            .parse::<age::tag::Recipient>()
            .map(|r| Some(Box::new(r) as Box<dyn Recipient + Send>))
            .map_err(|e| eyre!("invalid tagged age recipient: {e}"));
    }
    if trimmed.starts_with("age1") && trimmed.parse::<age::plugin::Recipient>().is_err() {
        match trimmed.parse::<age::x25519::Recipient>() {
            Ok(r) => Ok(Some(Box::new(r))),
            Err(e) => Err(eyre!("invalid age recipient {trimmed:?}: {e}")),
        }
    } else if trimmed.starts_with("ssh-") {
        // the age crate validates the key material
        match trimmed.parse::<ssh::Recipient>() {
            Ok(r) => Ok(Some(Box::new(r))),
            Err(e) => Err(eyre!("invalid SSH recipient: {e:?}")),
        }
    } else if let Ok(recipient) = trimmed.parse::<age::plugin::Recipient>() {
        if !interactive {
            return Err(eyre!(
                "plugin-dependent age recipients require interactive synchronization; use a native age, SSH, or tagged public recipient for background encryption"
            ));
        }
        let plugin = age::plugin::RecipientPluginV1::new(
            recipient.plugin(),
            std::slice::from_ref(&recipient),
            &[],
            age::NoCallbacks,
        )
        .map_err(|e| eyre!("age recipient plugin is unavailable: {e}"))?;
        Ok(Some(Box::new(plugin)))
    } else {
        Ok(None)
    }
}

/// The public key beside an SSH private key: age cannot derive it, so the
/// `.pub` file must exist.
pub(crate) async fn ssh_public_key_for_private(path: &Path) -> Result<String> {
    let pub_path = path.with_extension("pub");
    if pub_path.exists() {
        let content = file::read_to_string(&pub_path)?;
        let trimmed = content.trim();
        if trimmed.starts_with("ssh-") {
            return Ok(trimmed.to_string());
        }
    }
    bail!(
        "no public key for the SSH private key at {}; expected {}",
        display_path(path),
        display_path(&pub_path)
    )
}

/// Every identity this machine has: `MISE_AGE_KEY`, the identity files
/// named by the settings and the default `age.txt`, and the SSH keys named
/// by the settings and the default `~/.ssh/id_ed25519` / `id_rsa`.
pub(crate) async fn load_all_identities() -> LoadedIdentities {
    load_identities(false).await
}

async fn load_identities(interactive: bool) -> LoadedIdentities {
    let identity_files = get_all_identity_files().await;
    let ssh_identity_files = get_all_ssh_identity_files();
    let mut loaded = LoadedIdentities::default();
    let mut plugin_sources = Vec::new();

    if let Ok(age_key) = env::var("MISE_AGE_KEY")
        && !age_key.is_empty()
    {
        plugin_sources.push(age_key.clone());
        let age_key = software_identity_text(&age_key);
        // raw secret keys first
        for line in age_key.lines() {
            let line = line.trim();
            if line.starts_with("AGE-SECRET-KEY-")
                && let Ok(identity) = line.parse::<age::x25519::Identity>()
            {
                loaded.x25519_public.push(identity.to_public().to_string());
                loaded.identities.push(Box::new(identity));
            }
        }
        // else the whole value as an identity file
        if loaded.identities.is_empty()
            && let Ok(identity_file) = IdentityFile::from_buffer(age_key.as_bytes())
            && let Ok(mut file_identities) = identity_file.into_identities()
        {
            loaded.identities.append(&mut file_identities);
        }
    }

    for path in identity_files {
        if !path.exists() {
            continue;
        }
        match file::read_to_string(&path) {
            Ok(content) => {
                plugin_sources.push(content.clone());
                let content = software_identity_text(&content);
                if let Ok(identity_file) = IdentityFile::from_buffer(content.as_bytes())
                    && let Ok(mut file_identities) = identity_file.into_identities()
                {
                    loaded.identities.append(&mut file_identities);
                }
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with("AGE-SECRET-KEY-")
                        && let Ok(identity) = line.parse::<age::x25519::Identity>()
                    {
                        loaded.x25519_public.push(identity.to_public().to_string());
                    }
                }
            }
            Err(e) => {
                debug!("age: failed to read identity file {:?}: {}", path, e);
            }
        }
    }

    for path in ssh_identity_files {
        if !path.exists() {
            continue;
        }
        match std::fs::File::open(&path) {
            Ok(file) => {
                let mut reader = BufReader::new(file);
                match ssh::Identity::from_buffer(&mut reader, Some(path.display().to_string())) {
                    Ok(identity) => {
                        if let Some(reason) = UnusableSshIdentity::classify(&identity) {
                            // Still keep it: dropping the only identity would
                            // report "no identities" instead, which is just
                            // as misleading as the message being fixed here.
                            loaded.unusable.push((path.clone(), reason));
                        }
                        loaded.identities.push(Box::new(identity));
                    }
                    Err(e) => {
                        debug!("age: failed to parse SSH identity from {:?}: {}", path, e);
                    }
                }
            }
            Err(e) => {
                debug!("age: failed to read SSH identity file {:?}: {}", path, e);
            }
        }
    }

    // Try software recovery identities before touching hardware.
    for text in plugin_sources {
        add_plugin_identities(&text, interactive, &mut loaded);
    }
    loaded
}

fn software_identity_text(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim().starts_with("AGE-PLUGIN-"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn add_plugin_identities(text: &str, interactive: bool, loaded: &mut LoadedIdentities) {
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("AGE-PLUGIN-"))
    {
        loaded.plugins += 1;
        // Hardware plugins can display native dialogs without asking callbacks.
        // Never start them during background work or routine diagnostics.
        if !interactive {
            continue;
        }
        match line.parse::<age::plugin::Identity>() {
            Ok(identity) => match age::plugin::IdentityPluginV1::new(
                identity.plugin(),
                std::slice::from_ref(&identity),
                HardwareCallbacks,
            ) {
                Ok(plugin) => loaded.identities.push(Box::new(plugin)),
                Err(err) => warn!("age identity plugin unavailable: {err}"),
            },
            Err(_) => warn!("invalid age plugin identity"),
        }
    }
}

#[derive(Clone)]
struct HardwareCallbacks;
impl age::Callbacks for HardwareCallbacks {
    fn display_message(&self, message: &str) {
        eprintln!("{message}");
    }
    fn confirm(&self, message: &str, yes: &str, _no: Option<&str>) -> Option<bool> {
        let answer = demand::Input::new(format!("{message} ({yes}? y/N)"))
            .run()
            .ok()?;
        Some(answer.eq_ignore_ascii_case("y"))
    }
    fn request_public_string(&self, description: &str) -> Option<String> {
        demand::Input::new(description).run().ok()
    }
    fn request_passphrase(&self, description: &str) -> Option<age::secrecy::SecretString> {
        demand::Input::new(description)
            .password(true)
            .run()
            .ok()
            .map(Into::into)
    }
}

/// Sync's Git reconciliation is synchronous; isolate the identity loader's
/// runtime rather than nesting a runtime on a Tokio worker.
pub(crate) fn decrypt_sync(ciphertext: &[u8], interactive: bool) -> Result<Vec<u8>> {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(decrypt_bytes_mode(ciphertext, interactive))
                    .map_err(Into::into)
            })
            .join()
            .map_err(|_| eyre!("age decryption worker failed"))?
    })
}

async fn get_default_key_file() -> Option<PathBuf> {
    Settings::get()
        .age
        .key_file
        .clone()
        .map(replace_path)
        .or_else(|| {
            let default_path = dirs::CONFIG.join("age.txt");
            if default_path.exists() {
                Some(default_path)
            } else {
                None
            }
        })
}

async fn get_all_identity_files() -> Vec<PathBuf> {
    identity_paths_age()
}

pub(crate) fn identity_paths() -> Vec<PathBuf> {
    let mut paths = identity_paths_age();
    paths.extend(get_all_ssh_identity_files());
    paths
}

fn identity_paths_age() -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Some(ref identity_files) = Settings::get().age.identity_files {
        for path in identity_files {
            // Apply path expansion for tilde and environment variables
            files.push(replace_path(path.clone()));
        }
    }

    if let Some(key_file) = Settings::get().age.key_file.clone() {
        files.push(replace_path(key_file));
    }

    let default_age_txt = dirs::CONFIG.join("age.txt");
    if default_age_txt.exists() && !files.contains(&default_age_txt) {
        files.push(default_age_txt);
    }

    files
}

fn get_all_ssh_identity_files() -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Some(ref ssh_identity_files) = Settings::get().age.ssh_identity_files {
        for path in ssh_identity_files {
            // Apply path expansion for tilde and environment variables
            files.push(replace_path(path.clone()));
        }
    }

    files.extend(get_default_ssh_key_paths());
    files
}

fn get_default_ssh_key_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = &*dirs::HOME;
    let ssh_dir = home.join(".ssh");
    paths.push(ssh_dir.join("id_ed25519"));
    paths.push(ssh_dir.join("id_rsa"));
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_unsupported_ssh_identities() {
        // `Encrypted` needs a real parsed key, so it is covered end-to-end by
        // e2e/env/test_env_age_ssh_passphrase instead. These variants carry
        // only a string, so the mapping can be checked directly.
        let cases = [
            (
                ssh::UnsupportedKey::EncryptedPem,
                UnusableSshIdentity::EncryptedPem,
            ),
            (
                ssh::UnsupportedKey::EncryptedSsh("aes256-cbc".into()),
                UnusableSshIdentity::EncryptedCipher("aes256-cbc".into()),
            ),
            (
                ssh::UnsupportedKey::Hardware("sk-ssh-ed25519".into()),
                UnusableSshIdentity::Hardware("sk-ssh-ed25519".into()),
            ),
            (
                ssh::UnsupportedKey::Type("ssh-dss".into()),
                UnusableSshIdentity::KeyType("ssh-dss".into()),
            ),
        ];
        for (key, expected) in cases {
            let identity = ssh::Identity::Unsupported(key);
            assert_eq!(UnusableSshIdentity::classify(&identity), Some(expected));
        }
    }

    #[test]
    fn test_unusable_identity_hint_stays_quiet_when_every_key_is_readable() {
        assert_eq!(unusable_identity_hint(&[]), "");
    }

    #[test]
    fn test_unusable_identity_hint_names_the_file_and_the_way_out() {
        let path = PathBuf::from("/keys/id_ed25519");
        let hint = unusable_identity_hint(&[(path.clone(), UnusableSshIdentity::Passphrase)]);
        // Compare against the rendered form: display_path uses the host
        // separator, so a literal "/keys/..." never matches on Windows.
        assert!(hint.contains(&display_path(&path)), "{hint}");
        assert!(hint.contains("passphrase"), "{hint}");
        assert!(hint.contains("settings.age.key_file"), "{hint}");
    }

    #[test]
    fn test_parse_recipient() -> Result<()> {
        let age_recipient = "age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p";
        assert!(parse_recipient(age_recipient)?.is_some());
        // Current age-plugin-tpm exports native tagged recipients. Encryption
        // must not look for an age-plugin-tag executable or access the TPM.
        let tpm_recipient = "age1tag1q096edfp3ty6n36fj5kyq0yuesp7rdcmm7sjswzdcrekh6ash8n3uys987t";
        let tagged = parse_recipient(tpm_recipient)?.unwrap();
        assert!(encrypt_bytes(b"TPM public recipient", &[tagged]).is_ok());
        let ssh_recipient =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJmkfJ8VZq4m5k7tJVts7+nR01fbRvLHLgeQCF6FWYr5";
        assert!(parse_recipient(ssh_recipient)?.is_some());
        assert!(parse_recipient("invalid_recipient")?.is_none());
        let err = match parse_recipient("age1notakey") {
            Err(err) => err.to_string(),
            Ok(_) => panic!("age1notakey was accepted"),
        };
        assert!(!err.contains("experimental"), "{err}");
        Ok(())
    }

    #[tokio::test]
    async fn test_encrypt_bytes_round_trips_and_compresses() -> Result<()> {
        use age::secrecy::ExposeSecret;
        let key = age::x25519::Identity::generate();
        let recipients: Vec<Box<dyn Recipient + Send>> = vec![Box::new(key.to_public())];

        let small = b"a few bytes".to_vec();
        let ciphertext = encrypt_bytes(&small, &recipients)?;
        assert_ne!(ciphertext, small);

        // compressible: the ciphertext ends up shorter than the plaintext
        let large = "the same line again\n".repeat(5000).into_bytes();
        let large_ciphertext = encrypt_bytes(&large, &recipients)?;
        assert!(large_ciphertext.len() < large.len());

        let mut vars = crate::test::EnvVarGuard::new();
        vars.set("MISE_AGE_KEY", key.to_string().expose_secret());
        let decrypted = decrypt_bytes_mode(&ciphertext, false).await;
        let decrypted_large = decrypt_bytes_mode(&large_ciphertext, false).await;
        vars.remove("MISE_AGE_KEY");
        assert_eq!(decrypted.unwrap(), small);
        assert_eq!(decrypted_large.unwrap(), large);
        Ok(())
    }

    #[tokio::test]
    async fn test_decrypt_bytes_reports_the_wrong_identity() -> Result<()> {
        use age::secrecy::ExposeSecret;
        let key = age::x25519::Identity::generate();
        let other = age::x25519::Identity::generate();
        let recipients: Vec<Box<dyn Recipient + Send>> = vec![Box::new(key.to_public())];
        let ciphertext = encrypt_bytes(b"payload", &recipients)?;

        let mut vars = crate::test::EnvVarGuard::new();
        vars.set("MISE_AGE_KEY", other.to_string().expose_secret());
        let result = decrypt_bytes_mode(&ciphertext, false).await;
        let corrupt = decrypt_bytes_mode(b"not an age file at all", false).await;
        vars.remove("MISE_AGE_KEY");
        assert!(
            matches!(result, Err(DecryptError::Failed { .. })),
            "{result:?}"
        );
        assert!(
            matches!(corrupt, Err(DecryptError::Corrupt(_))),
            "{corrupt:?}"
        );
        Ok(())
    }

    #[test]
    fn test_encrypt_bytes_needs_a_recipient() {
        let err = encrypt_bytes(b"x", &[]).unwrap_err().to_string();
        assert!(err.contains("no age recipients"), "{err}");
        assert!(!err.contains("experimental"), "{err}");
    }
}

#[cfg(test)]
mod limits_tests {
    use super::*;

    #[test]
    fn bounded_reader_accepts_limit_and_refuses_next_byte() {
        assert_eq!(read_bounded(&b"1234"[..], 4).unwrap(), b"1234");
        assert!(read_bounded(&b"12345"[..], 4).is_err());
        let compressed = zstd::encode_all(&vec![0u8; 10000][..], 1).unwrap();
        let decoder = zstd::stream::read::Decoder::new(&compressed[..]).unwrap();
        assert!(read_bounded(decoder, 100).is_err());
    }
}

#[cfg(all(test, unix))]
mod plugin_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn plugin_protocol_roundtrip_and_software_recovery() {
        use age::secrecy::ExposeSecret;
        let tmp = tempfile::tempdir().unwrap();
        let executable = tmp.path().join("age-plugin-se");
        std::fs::write(
            &executable,
            include_str!("agecrypt/fixtures/age-plugin-se.py"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut vars = crate::test::EnvVarGuard::new();
        vars.remove("MISE_TEST_PLUGIN_MODE");
        let old_path = env::var("PATH").unwrap();
        vars.set("PATH", format!("{}:{old_path}", tmp.path().display()));
        let identity = age::plugin::Identity::default_for_plugin("se")
            .unwrap()
            .to_string();
        vars.set("MISE_AGE_KEY", &identity);
        let recipient = "age1se1qfn44rsw0xvmez3pky46nghmnd5up0jpj97nd39zptlh83a0nja6skde3ak";
        let refused = parse_recipient_mode(recipient, false).err().unwrap();
        assert!(
            refused
                .to_string()
                .contains("require interactive synchronization")
        );
        let software = age::x25519::Identity::generate();
        let recipients: Vec<Box<dyn Recipient + Send>> = vec![
            parse_recipient(recipient).unwrap().unwrap(),
            Box::new(software.to_public()),
        ];
        let ciphertext = encrypt_bytes(b"plugin protocol test", &recipients).unwrap();
        let locked = decrypt_bytes_mode(&ciphertext, false).await;
        assert!(locked.is_err());
        assert_eq!(
            decrypt_bytes_mode(&ciphertext, true).await.unwrap(),
            b"plugin protocol test"
        );
        vars.set("MISE_TEST_PLUGIN_MODE", "malformed");
        assert!(decrypt_bytes_mode(&ciphertext, true).await.is_err());
        vars.set("MISE_TEST_PLUGIN_MODE", "cancel");
        assert!(encrypt_bytes(b"cancelled request", &recipients).is_err());
        let plugin_identity = identity.parse::<age::plugin::Identity>().unwrap();
        let noninteractive =
            age::plugin::IdentityPluginV1::new("se", &[plugin_identity], age::NoCallbacks).unwrap();
        assert!(
            Decryptor::new(&ciphertext[..])
                .unwrap()
                .decrypt(std::iter::once(&noninteractive as &dyn Identity))
                .is_err()
        );
        vars.remove("MISE_TEST_PLUGIN_MODE");
        std::fs::remove_file(&executable).unwrap();
        assert!(parse_recipient(recipient).is_err());
        assert!(decrypt_bytes_mode(&ciphertext, true).await.is_err());
        vars.set("MISE_AGE_KEY", software.to_string().expose_secret());
        assert_eq!(
            decrypt_bytes_mode(&ciphertext, false).await.unwrap(),
            b"plugin protocol test"
        );
    }
}
