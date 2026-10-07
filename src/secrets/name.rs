/// Validated `[A-Za-z_][A-Za-z0-9_]*`. Names are not secret; `Display` is fine.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct SecretName(String);

impl SecretName {
    pub fn new(s: &str) -> Option<Self> {
        let mut chars = s.chars();
        let first = chars.next()?;
        if !(first.is_ascii_alphabetic() || first == '_') {
            return None;
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        Some(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SecretName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names() {
        for ok in ["A", "_a", "DEPLOY_KEY", "a1_B2"] {
            assert_eq!(SecretName::new(ok).unwrap().as_str(), ok);
        }
        for bad in ["", "1A", "A-B", "A B", "A.B", "é"] {
            assert!(SecretName::new(bad).is_none(), "{bad}");
        }
    }
}
