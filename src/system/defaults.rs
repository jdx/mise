//! macOS user defaults (preferences) for the `[bootstrap.macos.defaults]` config section.
//!
//! Entries are read and written with Core Foundation's preferences API so
//! nested property-list values retain their types. Like `[bootstrap.packages]`
//! they are machine-global, declarative, and only ever applied when explicitly
//! requested with `mise bootstrap macos defaults apply` or `mise bootstrap`.

use indexmap::IndexMap;

use crate::result::Result;

/// A single `[bootstrap.macos.defaults.<domain>]` entry: `key = value`
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DefaultsRequest {
    /// preferences domain, e.g. "com.apple.dock" or "NSGlobalDomain"
    pub domain: String,
    pub key: String,
    pub value: DefaultsValue,
}

impl std::fmt::Display for DefaultsRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} = {}", self.domain, self.key, self.value)
    }
}

/// Property-list values that mise can write and verify. TOML arrays and tables
/// are converted recursively, preserving the types of nested values.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DefaultsValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Self>),
    Dict(IndexMap<String, Self>),
}

impl DefaultsValue {
    pub(crate) fn from_toml(value: &toml::Value) -> Option<Self> {
        match value {
            toml::Value::Boolean(b) => Some(Self::Bool(*b)),
            toml::Value::Integer(i) => Some(Self::Int(*i)),
            toml::Value::Float(f) => Some(Self::Float(*f)),
            toml::Value::String(s) => Some(Self::Str(s.clone())),
            toml::Value::Array(values) => values
                .iter()
                .map(Self::from_toml)
                .collect::<Option<Vec<_>>>()
                .map(Self::Array),
            toml::Value::Table(values) => values
                .iter()
                .map(|(key, value)| Some((key.clone(), Self::from_toml(value)?)))
                .collect::<Option<IndexMap<_, _>>>()
                .map(Self::Dict),
            toml::Value::Datetime(_) => None,
        }
    }

    /// A copy-pasteable `defaults write` suffix for scalar values. The CLI
    /// cannot safely represent nested typed property-list values.
    fn write_args(&self) -> Option<Vec<String>> {
        match self {
            Self::Bool(b) => Some(vec!["-bool".into(), b.to_string()]),
            Self::Int(i) => Some(vec!["-int".into(), i.to_string()]),
            Self::Float(f) => Some(vec!["-float".into(), f.to_string()]),
            Self::Str(s) => Some(vec!["-string".into(), s.clone()]),
            Self::Array(_) | Self::Dict(_) => None,
        }
    }

    #[cfg(any(target_os = "macos", test))]
    fn to_plist(&self) -> plist::Value {
        match self {
            Self::Bool(value) => plist::Value::Boolean(*value),
            Self::Int(value) => plist::Value::Integer((*value).into()),
            Self::Float(value) => plist::Value::Real(*value),
            Self::Str(value) => plist::Value::String(value.clone()),
            Self::Array(values) => plist::Value::Array(values.iter().map(Self::to_plist).collect()),
            Self::Dict(values) => {
                let mut dict = plist::Dictionary::new();
                for (key, value) in values {
                    dict.insert(key.clone(), value.to_plist());
                }
                plist::Value::Dictionary(dict)
            }
        }
    }

    pub(crate) fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Bool(b) => (*b).into(),
            Self::Int(i) => (*i).into(),
            Self::Float(f) => (*f).into(),
            Self::Str(s) => s.clone().into(),
            Self::Array(values) => values.iter().map(Self::to_json).collect::<Vec<_>>().into(),
            Self::Dict(values) => values
                .iter()
                .map(|(key, value)| (key.clone(), value.to_json()))
                .collect::<serde_json::Map<_, _>>()
                .into(),
        }
    }

    fn matches(&self, current: &plist::Value) -> bool {
        match (self, current) {
            (Self::Bool(expected), plist::Value::Boolean(current)) => expected == current,
            (Self::Int(expected), plist::Value::Integer(current)) => {
                current.as_signed() == Some(*expected)
            }
            (Self::Float(expected), plist::Value::Real(current)) => {
                (current - expected).abs() < 1e-9
            }
            (Self::Str(expected), plist::Value::String(current)) => expected == current,
            (Self::Array(expected), plist::Value::Array(current)) => {
                expected.len() == current.len()
                    && expected
                        .iter()
                        .zip(current)
                        .all(|(expected, current)| expected.matches(current))
            }
            (Self::Dict(expected), plist::Value::Dictionary(current)) => {
                expected.len() == current.len()
                    && expected.iter().all(|(key, expected)| {
                        current
                            .get(key)
                            .is_some_and(|current| expected.matches(current))
                    })
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for DefaultsValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Str(s) => write!(f, "{s}"),
            Self::Array(_) | Self::Dict(_) => write!(f, "{}", self.to_json()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DefaultsState {
    /// current value matches the config
    Set,
    /// a value exists but differs from the config (in value or type)
    Differs { current: String },
    /// the key is not set in this domain
    Unset,
}

#[derive(Debug, Clone)]
pub(crate) struct DefaultsStatus {
    pub request: DefaultsRequest,
    pub state: DefaultsState,
}

pub(crate) fn is_available() -> bool {
    cfg!(target_os = "macos")
}

pub(crate) fn unavailable_reason() -> String {
    "only available on macos".to_string()
}

/// Query the current state of each entry. Side-effect free.
pub(crate) async fn status(requests: &[DefaultsRequest]) -> Result<Vec<DefaultsStatus>> {
    let requests = requests.to_vec();
    tokio::task::spawn_blocking(move || status_sync(&requests)).await?
}

fn status_sync(requests: &[DefaultsRequest]) -> Result<Vec<DefaultsStatus>> {
    let mut out = vec![];
    for req in requests {
        let state = match read(&req.domain, &req.key)? {
            Some(current) => {
                if req.value.matches(&current) {
                    DefaultsState::Set
                } else {
                    DefaultsState::Differs {
                        current: display_plist(&current),
                    }
                }
            }
            None => DefaultsState::Unset,
        };
        out.push(DefaultsStatus {
            request: req.clone(),
            state,
        });
    }
    Ok(out)
}

/// Write the given entries (already filtered to unset/differing ones)
pub(crate) async fn apply(requests: &[DefaultsRequest], dry_run: bool) -> Result<()> {
    for req in requests {
        if dry_run {
            if let Some(write_args) = req.value.write_args() {
                let mut args = vec!["write".to_string(), req.domain.clone(), req.key.clone()];
                args.extend(write_args);
                miseprintln!("defaults {}", shell_words::join(&args));
            } else {
                miseprintln!("macOS preference {req}");
            }
            continue;
        }
        debug!("setting macOS preference {req}");
    }
    if dry_run {
        return Ok(());
    }
    let requests = requests.to_vec();
    tokio::task::spawn_blocking(move || write_all(&requests)).await?
}

fn display_plist(value: &plist::Value) -> String {
    match value {
        plist::Value::Boolean(value) => value.to_string(),
        plist::Value::Integer(value) => value
            .as_signed()
            .map(|value| value.to_string())
            .or_else(|| value.as_unsigned().map(|value| value.to_string()))
            .unwrap_or_else(|| format!("{value:?}")),
        plist::Value::Real(value) => value.to_string(),
        plist::Value::String(value) => value.clone(),
        plist::Value::Array(values) => format!("array ({} items)", values.len()),
        plist::Value::Dictionary(values) => format!("dictionary ({} entries)", values.len()),
        plist::Value::Data(value) => format!("data ({} bytes)", value.len()),
        plist::Value::Date(value) => format!("{value:?}"),
        plist::Value::Uid(value) => format!("{value:?}"),
        _ => format!("{value:?}"),
    }
}

#[cfg(target_os = "macos")]
fn read(domain: &str, key: &str) -> Result<Option<plist::Value>> {
    macos::read(domain, key)
}

#[cfg(not(target_os = "macos"))]
fn read(_domain: &str, _key: &str) -> Result<Option<plist::Value>> {
    Ok(None)
}

#[cfg(target_os = "macos")]
fn write_all(requests: &[DefaultsRequest]) -> Result<()> {
    macos::write_all(requests)
}

#[cfg(not(target_os = "macos"))]
fn write_all(_requests: &[DefaultsRequest]) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::base::TCFType;
    use core_foundation::data::CFData;
    use core_foundation::propertylist::{CFPropertyList, create_data, create_with_data};
    use core_foundation::string::CFString;
    use core_foundation_sys::preferences::{
        CFPreferencesCopyValue, CFPreferencesSetValue, CFPreferencesSynchronize,
        kCFPreferencesAnyApplication, kCFPreferencesAnyHost, kCFPreferencesCurrentUser,
    };
    use core_foundation_sys::propertylist::{
        kCFPropertyListImmutable, kCFPropertyListXMLFormat_v1_0,
    };
    use indexmap::IndexSet;

    use super::*;

    fn application_id(
        domain: &str,
    ) -> (Option<CFString>, core_foundation_sys::string::CFStringRef) {
        if matches!(domain, "NSGlobalDomain" | "-g" | "-globalDomain") {
            (None, unsafe { kCFPreferencesAnyApplication })
        } else {
            let domain = CFString::new(domain);
            let reference = domain.as_concrete_TypeRef();
            (Some(domain), reference)
        }
    }

    pub(super) fn read(domain: &str, key: &str) -> Result<Option<plist::Value>> {
        let key = CFString::new(key);
        let (_application, application_id) = application_id(domain);
        let value = unsafe {
            CFPreferencesCopyValue(
                key.as_concrete_TypeRef(),
                application_id,
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            )
        };
        if value.is_null() {
            return Ok(None);
        }
        let value = unsafe { CFPropertyList::wrap_under_create_rule(value) };
        let data = create_data(value.as_CFTypeRef(), kCFPropertyListXMLFormat_v1_0)
            .map_err(|err| eyre::eyre!("failed to serialize macOS preference: {err}"))?;
        Ok(Some(plist::Value::from_reader_xml(data.bytes())?))
    }

    fn set(domain: &str, key: &str, value: &DefaultsValue) -> Result<()> {
        let mut xml = Vec::new();
        plist::to_writer_xml(&mut xml, &value.to_plist())?;
        let data = CFData::from_buffer(&xml);
        let (value, _) = create_with_data(data, kCFPropertyListImmutable)
            .map_err(|err| eyre::eyre!("failed to parse macOS preference: {err}"))?;
        let value = unsafe { CFPropertyList::wrap_under_create_rule(value) };
        let key = CFString::new(key);
        let (_application, application_id) = application_id(domain);
        unsafe {
            CFPreferencesSetValue(
                key.as_concrete_TypeRef(),
                value.as_CFTypeRef(),
                application_id,
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            );
        }
        Ok(())
    }

    fn synchronize(domain: &str) -> Result<()> {
        let (_application, application_id) = application_id(domain);
        unsafe {
            if CFPreferencesSynchronize(
                application_id,
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            ) == 0
            {
                eyre::bail!("failed to synchronize macOS preference domain {domain}");
            }
        }
        Ok(())
    }

    pub(super) fn write_all(requests: &[DefaultsRequest]) -> Result<()> {
        let mut domains = IndexSet::new();
        for request in requests {
            set(&request.domain, &request.key, &request.value)?;
            domains.insert(request.domain.as_str());
        }
        for domain in domains {
            synchronize(domain)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn remove(domain: &str, key: &str) -> Result<()> {
        let key = CFString::new(key);
        let (_application, application_id) = application_id(domain);
        unsafe {
            CFPreferencesSetValue(
                key.as_concrete_TypeRef(),
                std::ptr::null(),
                application_id,
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            );
            if CFPreferencesSynchronize(
                application_id,
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            ) == 0
            {
                eyre::bail!("failed to synchronize macOS preference domain {domain}");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(s: &str) -> toml::Value {
        s.parse().unwrap()
    }

    #[test]
    fn test_from_toml() {
        assert_eq!(
            DefaultsValue::from_toml(&val("true")),
            Some(DefaultsValue::Bool(true))
        );
        assert_eq!(
            DefaultsValue::from_toml(&val("48")),
            Some(DefaultsValue::Int(48))
        );
        assert_eq!(
            DefaultsValue::from_toml(&val("1.5")),
            Some(DefaultsValue::Float(1.5))
        );
        assert_eq!(
            DefaultsValue::from_toml(&val(r#""right""#)),
            Some(DefaultsValue::Str("right".into()))
        );
        assert_eq!(
            DefaultsValue::from_toml(&val("[1, true]")),
            Some(DefaultsValue::Array(vec![
                DefaultsValue::Int(1),
                DefaultsValue::Bool(true),
            ]))
        );
        assert_eq!(
            DefaultsValue::from_toml(&val(r#"{ a = 1, nested = { enabled = true } }"#)),
            Some(DefaultsValue::Dict(IndexMap::from([
                ("a".into(), DefaultsValue::Int(1)),
                (
                    "nested".into(),
                    DefaultsValue::Dict(IndexMap::from([(
                        "enabled".into(),
                        DefaultsValue::Bool(true),
                    )])),
                ),
            ])))
        );

        // Dates and times remain unsupported; this change only adds arrays and tables.
        assert_eq!(DefaultsValue::from_toml(&val("1979-05-27T07:32:00Z")), None);
    }

    #[test]
    fn test_write_args() {
        assert_eq!(
            DefaultsValue::Bool(true).write_args().unwrap(),
            ["-bool", "true"]
        );
        assert_eq!(
            DefaultsValue::Bool(false).write_args().unwrap(),
            ["-bool", "false"]
        );
        assert_eq!(DefaultsValue::Int(2).write_args().unwrap(), ["-int", "2"]);
        assert_eq!(
            DefaultsValue::Float(0.5).write_args().unwrap(),
            ["-float", "0.5"]
        );
        assert_eq!(
            DefaultsValue::Str("left".into()).write_args().unwrap(),
            ["-string", "left"]
        );
        assert_eq!(DefaultsValue::Array(vec![]).write_args(), None);
    }

    #[test]
    fn test_matches() {
        assert!(DefaultsValue::Bool(true).matches(&plist::Value::Boolean(true)));
        assert!(DefaultsValue::Bool(false).matches(&plist::Value::Boolean(false)));
        assert!(!DefaultsValue::Bool(true).matches(&plist::Value::Boolean(false)));
        // strict typing: integer 1 does not satisfy `true`
        assert!(!DefaultsValue::Bool(true).matches(&plist::Value::Integer(1.into())));

        assert!(DefaultsValue::Int(2).matches(&plist::Value::Integer(2.into())));
        assert!(!DefaultsValue::Int(2).matches(&plist::Value::Integer(3.into())));
        assert!(!DefaultsValue::Int(2).matches(&plist::Value::Real(2.0)));

        assert!(DefaultsValue::Float(48.0).matches(&plist::Value::Real(48.0)));
        assert!(DefaultsValue::Float(0.5).matches(&plist::Value::Real(0.5)));
        assert!(!DefaultsValue::Float(0.5).matches(&plist::Value::Real(0.6)));

        assert!(DefaultsValue::Str("left".into()).matches(&plist::Value::String("left".into())));
        assert!(!DefaultsValue::Str("left".into()).matches(&plist::Value::String("right".into())));

        let nested = DefaultsValue::from_toml(&val("[{ enabled = true, count = 2 }]")).unwrap();
        assert!(nested.matches(&nested.to_plist()));
    }

    /// `status()` must not fail when keys don't exist yet — this is the
    /// expected path on a fresh macOS install.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_status_missing_keys_are_unset() {
        let reqs = vec![
            // key doesn't exist in a real domain
            DefaultsRequest {
                domain: "NSGlobalDomain".into(),
                key: "_mise_test_nonexistent_key_42".into(),
                value: DefaultsValue::Bool(true),
            },
            // domain doesn't exist at all
            DefaultsRequest {
                domain: "com.mise.nonexistent".into(),
                key: "TestKey".into(),
                value: DefaultsValue::Bool(true),
            },
        ];
        let statuses = status(&reqs).await.unwrap();
        assert_eq!(statuses.len(), 2);
        for s in &statuses {
            assert_eq!(
                s.state,
                DefaultsState::Unset,
                "expected Unset for {}, got {:?}",
                s.request,
                s.state
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_nested_value_round_trip() {
        let domain = "com.mise.defaults-test";
        let key = "NestedValue";
        let value = DefaultsValue::from_toml(&val(
            r#"[{ name = "Terminal", enabled = true, position = 1, scale = 1.5, metadata = { kind = "file-tile" } }]"#,
        ))
        .unwrap();

        write_all(&[DefaultsRequest {
            domain: domain.into(),
            key: key.into(),
            value: value.clone(),
        }])
        .unwrap();
        let current = read(domain, key);
        let cleanup = macos::remove(domain, key);

        assert_eq!(current.unwrap(), Some(value.to_plist()));
        cleanup.unwrap();
    }
}
