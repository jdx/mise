use super::*;

/// Signer constraints for keyless cosign verification: the `--certificate-*`
/// flags of `cosign verify-blob`. A bundle only verifies when its (already
/// chain-validated) Fulcio certificate satisfies every constraint that is set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CosignIdentity {
    pub identity: Option<String>,
    pub identity_regexp: Option<String>,
    pub oidc_issuer: Option<String>,
    pub oidc_issuer_regexp: Option<String>,
    pub github_workflow_trigger: Option<String>,
    pub github_workflow_sha: Option<String>,
    pub github_workflow_name: Option<String>,
    pub github_workflow_repository: Option<String>,
    pub github_workflow_ref: Option<String>,
}

impl CosignIdentity {
    /// Parse the `--certificate-*` flags out of a cosign option list
    /// (`--flag value` or `--flag=value`). Other flags are ignored, but a
    /// `--certificate-*` flag that is unknown or lacks a value is an error:
    /// silently dropping a constraint would weaken verification.
    pub fn from_opts(opts: &[String]) -> Result<Self> {
        let mut identity = Self::default();
        let mut iter = opts.iter().peekable();
        while let Some(opt) = iter.next() {
            if !opt.starts_with("--certificate") {
                continue;
            }
            let (flag, inline) = match opt.split_once('=') {
                Some((flag, value)) => (flag, Some(value.to_string())),
                None => (opt.as_str(), None),
            };
            let value = match inline {
                Some(value) => value,
                // A following option is not this flag's value.
                None => iter
                    .next_if(|next| !next.starts_with("--"))
                    .cloned()
                    .ok_or_else(|| {
                        AttestationError::Verification(format!(
                            "cosign option {flag} requires a value"
                        ))
                    })?,
            };
            let slot = match flag {
                "--certificate-identity" => &mut identity.identity,
                "--certificate-identity-regexp" => &mut identity.identity_regexp,
                "--certificate-oidc-issuer" => &mut identity.oidc_issuer,
                "--certificate-oidc-issuer-regexp" => &mut identity.oidc_issuer_regexp,
                "--certificate-github-workflow-trigger" => &mut identity.github_workflow_trigger,
                "--certificate-github-workflow-sha" => &mut identity.github_workflow_sha,
                "--certificate-github-workflow-name" => &mut identity.github_workflow_name,
                "--certificate-github-workflow-repository" => {
                    &mut identity.github_workflow_repository
                }
                "--certificate-github-workflow-ref" => &mut identity.github_workflow_ref,
                _ => {
                    return Err(AttestationError::Verification(format!(
                        "unsupported cosign option {flag}; cannot enforce the signer constraint"
                    )));
                }
            };
            *slot = Some(value);
        }
        Ok(identity)
    }

    /// Whether the signer's identity is pinned. Without it, any Fulcio
    /// certificate — which any GitHub Actions workflow can obtain — would pass.
    /// An empty value, or a regexp that matches the empty string (`""`, `.*`),
    /// constrains nothing and does not count.
    pub fn pins_signer(&self) -> bool {
        self.require_pinned_signer().is_ok()
    }

    pub(crate) fn require_pinned_signer(&self) -> Result<()> {
        if self.identity.as_deref().is_some_and(|id| !id.is_empty()) {
            return Ok(());
        }
        if let Some(pattern) = self.identity_regexp.as_deref().filter(|p| !p.is_empty())
            && !compile_constraint(pattern)?.is_match("")
        {
            return Ok(());
        }
        Err(AttestationError::Verification(
            "keyless cosign verification requires a certificate identity \
             (--certificate-identity or a non-trivial --certificate-identity-regexp)"
                .to_string(),
        ))
    }
}

const OID_ISSUER_V1: &str = "1.3.6.1.4.1.57264.1.1";
const OID_GITHUB_WORKFLOW_TRIGGER: &str = "1.3.6.1.4.1.57264.1.2";
const OID_GITHUB_WORKFLOW_SHA: &str = "1.3.6.1.4.1.57264.1.3";
const OID_GITHUB_WORKFLOW_NAME: &str = "1.3.6.1.4.1.57264.1.4";
const OID_GITHUB_WORKFLOW_REPOSITORY: &str = "1.3.6.1.4.1.57264.1.5";
const OID_GITHUB_WORKFLOW_REF: &str = "1.3.6.1.4.1.57264.1.6";
const OID_ISSUER_V2: &str = "1.3.6.1.4.1.57264.1.8";
const OID_SOURCE_REPOSITORY_DIGEST: &str = "1.3.6.1.4.1.57264.1.13";
const OID_SOURCE_REPOSITORY_REF: &str = "1.3.6.1.4.1.57264.1.14";
const OID_BUILD_TRIGGER: &str = "1.3.6.1.4.1.57264.1.20";

/// Check a chain-validated Fulcio certificate against the signer constraints.
pub(crate) fn verify_certificate_identity(
    cert_der: &[u8],
    expected: &CosignIdentity,
) -> Result<()> {
    use x509_cert::Certificate;
    use x509_cert::der::Decode;

    let cert = Certificate::from_der(cert_der).map_err(|e| {
        AttestationError::Verification(format!("failed to parse signer certificate: {e}"))
    })?;

    if expected.identity.is_some() || expected.identity_regexp.is_some() {
        let identities = certificate_identities(&cert);
        let exact = expected.identity.as_deref();
        let regexp = expected
            .identity_regexp
            .as_deref()
            .map(compile_constraint)
            .transpose()?;
        let matched = identities.iter().any(|identity| {
            exact.is_none_or(|exact| identity == exact)
                && regexp.as_ref().is_none_or(|re| re.is_match(identity))
        });
        if !matched {
            return Err(mismatch("certificate identity", &identities));
        }
    }

    let issuer = fulcio_string(&cert, OID_ISSUER_V1, Some(OID_ISSUER_V2));
    check_constraint(
        "OIDC issuer",
        issuer.as_deref(),
        expected.oidc_issuer.as_deref(),
        expected.oidc_issuer_regexp.as_deref(),
    )?;
    check_constraint(
        "GitHub workflow trigger",
        fulcio_string(&cert, OID_GITHUB_WORKFLOW_TRIGGER, Some(OID_BUILD_TRIGGER)).as_deref(),
        expected.github_workflow_trigger.as_deref(),
        None,
    )?;
    check_constraint(
        "GitHub workflow sha",
        fulcio_string(
            &cert,
            OID_GITHUB_WORKFLOW_SHA,
            Some(OID_SOURCE_REPOSITORY_DIGEST),
        )
        .as_deref(),
        expected.github_workflow_sha.as_deref(),
        None,
    )?;
    check_constraint(
        "GitHub workflow name",
        fulcio_string(&cert, OID_GITHUB_WORKFLOW_NAME, None).as_deref(),
        expected.github_workflow_name.as_deref(),
        None,
    )?;
    check_constraint(
        "GitHub workflow repository",
        // Like cosign, check the GitHub Workflow Repository claim (the repository
        // running the workflow), falling back to the source repository claim on
        // certificates that lack it. The signing workflow file itself, which may
        // live in another repository for reusable workflows, is the SAN identity.
        fulcio_string(&cert, OID_GITHUB_WORKFLOW_REPOSITORY, None)
            .or_else(|| certificate_source_repository(&cert))
            .as_deref(),
        expected.github_workflow_repository.as_deref(),
        None,
    )?;
    check_constraint(
        "GitHub workflow ref",
        fulcio_string(
            &cert,
            OID_GITHUB_WORKFLOW_REF,
            Some(OID_SOURCE_REPOSITORY_REF),
        )
        .as_deref(),
        expected.github_workflow_ref.as_deref(),
        None,
    )?;
    Ok(())
}

fn check_constraint(
    what: &str,
    actual: Option<&str>,
    exact: Option<&str>,
    regexp: Option<&str>,
) -> Result<()> {
    if exact.is_none() && regexp.is_none() {
        return Ok(());
    }
    let found = actual
        .map(|actual| vec![actual.to_string()])
        .unwrap_or_default();
    let Some(actual) = actual else {
        return Err(mismatch(what, &found));
    };
    if exact.is_some_and(|exact| actual != exact) {
        return Err(mismatch(what, &found));
    }
    if let Some(regexp) = regexp
        && !compile_constraint(regexp)?.is_match(actual)
    {
        return Err(mismatch(what, &found));
    }
    Ok(())
}

fn compile_constraint(pattern: &str) -> Result<regex::Regex> {
    regex::Regex::new(&expand_quoted_literals(pattern)).map_err(|e| {
        AttestationError::Verification(format!(
            "invalid cosign regular expression {pattern:?}: {e}"
        ))
    })
}

/// Cosign compiles these patterns with Go's RE2, where `\Q...\E` matches the
/// text between them literally. The aqua registry relies on it to pin a tag
/// (`@refs/tags/\Q{{.Version}}\E$`), but Rust's `regex` rejects the escape, so
/// rewrite each quoted span into an escaped literal. Like RE2, an unterminated
/// `\Q` quotes through the end of the pattern.
fn expand_quoted_literals(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut rest = pattern;
    while let Some(start) = find_unescaped(rest, "\\Q") {
        out.push_str(&rest[..start]);
        let quoted = &rest[start + 2..];
        let (literal, tail) = quoted.split_once("\\E").unwrap_or((quoted, ""));
        out.push_str(&regex::escape(literal));
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// The byte offset of the first `needle` that is not itself escaped by an
/// odd run of backslashes, so `\\Q` (a literal backslash then `Q`) is left alone.
fn find_unescaped(haystack: &str, needle: &str) -> Option<usize> {
    let bytes = haystack.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if haystack[i..].starts_with(needle) {
                return Some(i);
            }
            // skip the escaped character
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

fn mismatch(what: &str, found: &[String]) -> AttestationError {
    AttestationError::WorkflowMismatch(format!(
        "signer {what} does not match the expected value; certificate has {found:?}"
    ))
}

/// Everything cosign treats as the certificate's identity: URI and email SANs.
fn certificate_identities(cert: &x509_cert::Certificate) -> Vec<String> {
    use x509_cert::ext::pkix::SubjectAltName;
    use x509_cert::ext::pkix::name::GeneralName;
    let Ok(Some(san)) = cert.tbs_certificate().get_extension::<SubjectAltName>() else {
        return Vec::new();
    };
    san.1
        .0
        .iter()
        .filter_map(|name| match name {
            GeneralName::UniformResourceIdentifier(uri) => Some(uri.to_string()),
            GeneralName::Rfc822Name(email) => Some(email.to_string()),
            _ => None,
        })
        .collect()
}

/// A Fulcio string extension: the deprecated raw-bytes OID, falling back to
/// the DER `UTF8String` OID that replaced it.
fn fulcio_string(
    cert: &x509_cert::Certificate,
    raw_oid: &str,
    der_oid: Option<&str>,
) -> Option<String> {
    use x509_cert::der::Decode;
    use x509_cert::der::asn1::Utf8StringRef;
    let extensions = cert.tbs_certificate().extensions()?;
    let extension = |oid: &str| {
        extensions
            .iter()
            .find(|ext| ext.extn_id.to_string() == oid)
            .map(|ext| ext.extn_value.as_bytes())
    };
    if let Some(value) = extension(raw_oid).and_then(|v| std::str::from_utf8(v).ok()) {
        return Some(value.to_string());
    }
    let value = extension(der_oid?)?;
    Utf8StringRef::from_der(value)
        .ok()
        .map(|s| s.as_str().to_string())
}
