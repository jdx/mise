//! macOS user defaults (preferences) for the `[bootstrap.macos.defaults]` config section.
//!
//! Entries are read and written with Core Foundation's preferences API so
//! nested property-list values retain their types. Like `[bootstrap.packages]`
//! they are machine-global, declarative, and only ever applied when explicitly
//! requested with `mise bootstrap macos defaults apply` or `mise bootstrap`.

use indexmap::IndexMap;

mod dock;

use crate::result::Result;

/// The host scope is part of a preference's identity.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HostScope {
    #[default]
    Any,
    Current,
}

pub(super) fn canonical_domain(domain: &str) -> &str {
    match domain {
        "-g" | "-globalDomain" => "NSGlobalDomain",
        domain => domain,
    }
}

/// A typed preference and its host scope.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DefaultsRequest {
    /// preferences domain, e.g. "com.apple.dock" or "NSGlobalDomain"
    pub domain: String,
    pub key: String,
    /// Use the current host instead of the any-host preference scope.
    pub host: HostScope,
    /// A nonempty dictionary path, or None to replace the whole preference.
    pub path: Option<Vec<String>>,
    pub value: DefaultsValue,
    /// The winning friendly Dock declaration uses application identity, not raw plist equality.
    pub dock_apps: bool,
}

impl DefaultsRequest {
    pub(crate) fn display_key(&self) -> String {
        let mut key = self.key.clone();
        if let Some(path) = &self.path {
            for component in path {
                key.push('[');
                key.push_str(&serde_json::to_string(component).expect("string serialization"));
                key.push(']');
            }
        }
        key
    }

    pub(crate) fn display_domain(&self) -> String {
        if self.host == HostScope::Current {
            format!("{} (current host)", self.domain)
        } else {
            self.domain.clone()
        }
    }
}

impl std::fmt::Display for DefaultsRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} = {}",
            self.display_domain(),
            self.display_key(),
            self.value
        )
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
    validate_requests(requests)?;
    let requests = requests.to_vec();
    tokio::task::spawn_blocking(move || status_sync(&requests)).await?
}

fn status_sync(requests: &[DefaultsRequest]) -> Result<Vec<DefaultsStatus>> {
    let mut out = vec![];
    for req in requests {
        let current = read(&req.domain, &req.key, req.host)?;
        let current = selected_value(current.as_ref(), req)?;
        let state = match current {
            Some(current) => {
                let matches = if req.dock_apps {
                    dock::matches(&req.value, current)?
                } else {
                    req.value.matches(current)
                };
                if matches {
                    DefaultsState::Set
                } else {
                    DefaultsState::Differs {
                        current: display_difference(&req.value, current),
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
    validate_requests(requests)?;
    for req in requests {
        if dry_run {
            if let Some(write_args) = req.value.write_args().filter(|_| req.path.is_none()) {
                let mut args = vec![];
                if req.host == HostScope::Current {
                    args.push("-currentHost".to_string());
                }
                args.extend(["write".to_string(), req.domain.clone(), req.key.clone()]);
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

/// Reject conflicting ownership before inspecting or writing preferences.
fn validate_requests(requests: &[DefaultsRequest]) -> Result<()> {
    for (i, request) in requests.iter().enumerate() {
        if request.dock_apps {
            dock::paths(&request.value)?;
        }
        if let Some(path) = &request.path {
            eyre::ensure!(
                !path.is_empty(),
                "defaults patch path must not be empty: {request}"
            );
        }
        for other in &requests[..i] {
            if canonical_domain(&request.domain) != canonical_domain(&other.domain)
                || request.key != other.key
                || request.host != other.host
            {
                continue;
            }
            let overlaps = match (&request.path, &other.path) {
                (None, None) => false, // Preserve existing whole-value declarations.
                (Some(a), Some(b)) => a.starts_with(b) || b.starts_with(a),
                _ => true,
            };
            eyre::ensure!(
                !overlaps,
                "overlapping macOS defaults declarations: {other}; {request}"
            );
        }
    }
    Ok(())
}

fn selected_value<'a>(
    mut current: Option<&'a plist::Value>,
    request: &DefaultsRequest,
) -> Result<Option<&'a plist::Value>> {
    if let Some(path) = &request.path {
        for component in path {
            current = match current {
                Some(plist::Value::Dictionary(dict)) => dict.get(component),
                None => return Ok(None),
                Some(_) => eyre::bail!("expected dictionary along defaults patch path: {request}"),
            };
        }
    }
    Ok(current)
}

#[cfg(any(target_os = "macos", test))]
fn patch_value(current: &mut plist::Value, path: &[String], value: plist::Value) -> Result<()> {
    let Some((key, rest)) = path.split_first() else {
        eyre::bail!("defaults patch path must not be empty");
    };
    let dict = current
        .as_dictionary_mut()
        .ok_or_else(|| eyre::eyre!("expected dictionary along defaults patch path"))?;
    if rest.is_empty() {
        dict.insert(key.clone(), value);
    } else {
        if !dict.contains_key(key) {
            dict.insert(
                key.clone(),
                plist::Value::Dictionary(plist::Dictionary::new()),
            );
        }
        patch_value(
            dict.get_mut(key).expect("dictionary entry inserted"),
            rest,
            value,
        )?;
    }
    Ok(())
}

/// Prepare every value before writing, including all patches of the same key.
#[cfg(any(target_os = "macos", test))]
fn prepare_writes(
    requests: &[DefaultsRequest],
    mut read: impl FnMut(&str, &str, HostScope) -> Result<Option<plist::Value>>,
) -> Result<IndexMap<(String, String, HostScope), plist::Value>> {
    validate_requests(requests)?;
    let mut writes = IndexMap::new();
    for request in requests {
        let domain = canonical_domain(&request.domain);
        let key = (domain.to_string(), request.key.clone(), request.host);
        if request.dock_apps {
            let current = read(domain, &request.key, request.host)?;
            writes.insert(key, dock::reconcile(&request.value, current.as_ref())?);
        } else if let Some(path) = &request.path {
            if !writes.contains_key(&key) {
                let current = read(domain, &request.key, request.host)?
                    .unwrap_or_else(|| plist::Value::Dictionary(plist::Dictionary::new()));
                writes.insert(key.clone(), current);
            }
            patch_value(
                writes.get_mut(&key).expect("preference inserted"),
                path,
                request.value.to_plist(),
            )
            .map_err(|err| eyre::eyre!("{request}: {err}"))?;
        } else {
            writes.insert(key, request.value.to_plist());
        }
    }
    Ok(writes)
}

fn display_difference(expected: &DefaultsValue, current: &plist::Value) -> String {
    let expected_type = plist_type(&expected.to_plist());
    let current_type = plist_type(current);
    let value = display_plist(current);
    if expected_type == current_type {
        value
    } else {
        format!("{value} ({current_type}; expected {expected_type})")
    }
}

fn plist_type(value: &plist::Value) -> &'static str {
    match value {
        plist::Value::Boolean(_) => "boolean",
        plist::Value::Integer(_) => "integer",
        plist::Value::Real(_) => "real",
        plist::Value::String(_) => "string",
        plist::Value::Array(_) => "array",
        plist::Value::Dictionary(_) => "dictionary",
        plist::Value::Data(_) => "data",
        plist::Value::Date(_) => "date",
        plist::Value::Uid(_) => "uid",
        _ => "unknown",
    }
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
        plist::Value::Array(values) => plist_to_json(value)
            .map(|value| value.to_string())
            .unwrap_or_else(|| format!("array ({} items)", values.len())),
        plist::Value::Dictionary(values) => plist_to_json(value)
            .map(|value| value.to_string())
            .unwrap_or_else(|| format!("dictionary ({} entries)", values.len())),
        plist::Value::Data(value) => format!("data ({} bytes)", value.len()),
        plist::Value::Date(value) => format!("{value:?}"),
        plist::Value::Uid(value) => format!("{value:?}"),
        _ => format!("{value:?}"),
    }
}

fn plist_to_json(value: &plist::Value) -> Option<serde_json::Value> {
    match value {
        plist::Value::Boolean(value) => Some((*value).into()),
        plist::Value::Integer(value) => value
            .as_signed()
            .map(Into::into)
            .or_else(|| value.as_unsigned().map(Into::into)),
        plist::Value::Real(value) => Some((*value).into()),
        plist::Value::String(value) => Some(value.clone().into()),
        plist::Value::Array(values) => values
            .iter()
            .map(plist_to_json)
            .collect::<Option<Vec<_>>>()
            .map(Into::into),
        plist::Value::Dictionary(values) => values
            .iter()
            .map(|(key, value)| Some((key.clone(), plist_to_json(value)?)))
            .collect::<Option<serde_json::Map<_, _>>>()
            .map(Into::into),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn read(domain: &str, key: &str, host: HostScope) -> Result<Option<plist::Value>> {
    macos::read(domain, key, host)
}

#[cfg(not(target_os = "macos"))]
fn read(_domain: &str, _key: &str, _host: HostScope) -> Result<Option<plist::Value>> {
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
        kCFPreferencesAnyApplication, kCFPreferencesAnyHost, kCFPreferencesCurrentHost,
        kCFPreferencesCurrentUser,
    };
    use core_foundation_sys::propertylist::{
        kCFPropertyListImmutable, kCFPropertyListXMLFormat_v1_0,
    };
    use indexmap::IndexSet;

    use super::*;

    fn host_id(host: HostScope) -> core_foundation_sys::string::CFStringRef {
        unsafe {
            match host {
                HostScope::Any => kCFPreferencesAnyHost,
                HostScope::Current => kCFPreferencesCurrentHost,
            }
        }
    }

    fn application_id(
        domain: &str,
    ) -> (Option<CFString>, core_foundation_sys::string::CFStringRef) {
        if canonical_domain(domain) == "NSGlobalDomain" {
            (None, unsafe { kCFPreferencesAnyApplication })
        } else {
            let domain = CFString::new(domain);
            let reference = domain.as_concrete_TypeRef();
            (Some(domain), reference)
        }
    }

    pub(super) fn read(domain: &str, key: &str, host: HostScope) -> Result<Option<plist::Value>> {
        let key = CFString::new(key);
        let (_application, application_id) = application_id(domain);
        let value = unsafe {
            CFPreferencesCopyValue(
                key.as_concrete_TypeRef(),
                application_id,
                kCFPreferencesCurrentUser,
                host_id(host),
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

    fn set(domain: &str, key: &str, value: &plist::Value, host: HostScope) -> Result<()> {
        let mut xml = Vec::new();
        plist::to_writer_xml(&mut xml, value)?;
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
                host_id(host),
            );
        }
        Ok(())
    }

    fn synchronize(domain: &str, host: HostScope) -> Result<()> {
        let (_application, application_id) = application_id(domain);
        unsafe {
            if CFPreferencesSynchronize(application_id, kCFPreferencesCurrentUser, host_id(host))
                == 0
            {
                eyre::bail!("failed to synchronize macOS preference domain {domain}");
            }
        }
        Ok(())
    }

    pub(super) fn write_all(requests: &[DefaultsRequest]) -> Result<()> {
        let mut domains = IndexSet::new();
        let writes = prepare_writes(requests, read)?;
        for ((domain, key, host), value) in &writes {
            set(domain, key, value, *host)?;
            domains.insert((domain.as_str(), *host));
        }
        for (domain, host) in domains {
            synchronize(domain, host)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn remove(domain: &str, key: &str, host: HostScope) -> Result<()> {
        let key = CFString::new(key);
        let (_application, application_id) = application_id(domain);
        unsafe {
            CFPreferencesSetValue(
                key.as_concrete_TypeRef(),
                std::ptr::null(),
                application_id,
                kCFPreferencesCurrentUser,
                host_id(host),
            );
            if CFPreferencesSynchronize(application_id, kCFPreferencesCurrentUser, host_id(host))
                == 0
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
    #[cfg(target_os = "macos")]
    fn test_dock_apps_native_round_trip() {
        let domain = format!("com.mise.dock-test.{}", uuid::Uuid::now_v7());
        let temp = tempfile::tempdir().unwrap();
        let app = temp.path().join("Example App.app");
        std::fs::create_dir(&app).unwrap();
        let request = DefaultsRequest {
            domain: domain.clone(),
            key: "persistent-apps".into(),
            host: HostScope::Any,
            path: None,
            dock_apps: true,
            value: DefaultsValue::Array(vec![DefaultsValue::Str(app.to_str().unwrap().into())]),
        };
        let result = (|| -> Result<()> {
            write_all(std::slice::from_ref(&request))?;
            assert_eq!(
                status_sync(std::slice::from_ref(&request))?[0].state,
                DefaultsState::Set
            );
            let original = read(&domain, &request.key, HostScope::Any)?.unwrap();
            write_all(std::slice::from_ref(&request))?;
            assert_eq!(
                read(&domain, &request.key, HostScope::Any)?.unwrap(),
                original
            );
            let clear = DefaultsRequest {
                value: DefaultsValue::Array(vec![]),
                ..request.clone()
            };
            write_all(std::slice::from_ref(&clear))?;
            assert_eq!(status_sync(&[clear])?[0].state, DefaultsState::Set);
            Ok(())
        })();
        macos::remove(&domain, "persistent-apps", HostScope::Any).unwrap();
        result.unwrap();
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
    fn differing_types_are_visible_even_when_values_render_identically() {
        for (expected, current, display) in [
            (
                DefaultsValue::Int(2),
                plist::Value::Real(2.0),
                "2 (real; expected integer)",
            ),
            (
                DefaultsValue::Float(15.0),
                plist::Value::Integer(15.into()),
                "15 (integer; expected real)",
            ),
            (
                DefaultsValue::Int(2),
                plist::Value::String("2".into()),
                "2 (string; expected integer)",
            ),
            (
                DefaultsValue::Bool(true),
                plist::Value::String("true".into()),
                "true (string; expected boolean)",
            ),
            (DefaultsValue::Int(2), plist::Value::Integer(3.into()), "3"),
        ] {
            assert!(!expected.matches(&current));
            assert_eq!(display_difference(&expected, &current), display);
        }
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

    #[test]
    fn test_display_plist_collections() {
        let nested = DefaultsValue::from_toml(&val("[{ enabled = true, count = 2 }]")).unwrap();
        assert_eq!(
            display_plist(&nested.to_plist()),
            r#"[{"enabled":true,"count":2}]"#
        );
        assert_eq!(
            display_plist(&plist::Value::Array(vec![plist::Value::Data(vec![1])])),
            "array (1 items)"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_host_scopes_are_independent() {
        let domain = format!("com.mise.defaults-test.{}", uuid::Uuid::now_v7());
        let any = DefaultsRequest {
            dock_apps: false,
            domain: domain.clone(),
            key: "ScopeValue".into(),
            host: HostScope::Any,
            path: None,
            value: DefaultsValue::Bool(true),
        };
        let host = DefaultsRequest {
            host: HostScope::Current,
            value: DefaultsValue::from_toml(&val("{ nested = [1, false] }")).unwrap(),
            ..any.clone()
        };
        write_all(std::slice::from_ref(&any)).unwrap();
        let missing = status_sync(std::slice::from_ref(&host));
        let write = write_all(&[any.clone(), host.clone()]);
        let any_value = read(&domain, &any.key, HostScope::Any);
        let host_value = read(&domain, &host.key, HostScope::Current);
        let statuses = status_sync(&[any.clone(), host.clone()]);
        let cleanup_any = macos::remove(&domain, &any.key, HostScope::Any);
        let cleanup_host = macos::remove(&domain, &host.key, HostScope::Current);
        assert_eq!(missing.unwrap()[0].state, DefaultsState::Unset);
        write.unwrap();
        assert_eq!(any_value.unwrap(), Some(any.value.to_plist()));
        assert_eq!(host_value.unwrap(), Some(host.value.to_plist()));
        for status in statuses.unwrap() {
            assert_eq!(status.state, DefaultsState::Set);
        }
        cleanup_any.unwrap();
        cleanup_host.unwrap();
    }

    fn patch(path: &[&str], value: DefaultsValue) -> DefaultsRequest {
        DefaultsRequest {
            dock_apps: false,
            domain: "com.mise.patch-test".into(),
            key: "Shortcuts".into(),
            host: HostScope::Any,
            path: Some(path.iter().map(|s| s.to_string()).collect()),
            value,
        }
    }

    #[test]
    fn test_patches_preserve_siblings_and_plist_types() {
        let mut original = DefaultsValue::from_toml(&val(
            r#"{ "64" = { enabled = true, parameters = [32, 49, 1048576] }, "65" = { enabled = true } }"#,
        )).unwrap().to_plist();
        let dict = original.as_dictionary_mut().unwrap();
        dict.insert("data".into(), plist::Value::Data(vec![0, 255]));
        dict.insert(
            "date".into(),
            plist::Value::Date(std::time::UNIX_EPOCH.into()),
        );
        let requests = [
            patch(&["64", "enabled"], DefaultsValue::Bool(false)),
            patch(&["new.key", "enabled"], DefaultsValue::Bool(true)),
        ];
        let mut reads = 0;
        let writes = prepare_writes(&requests, |_, _, _| {
            reads += 1;
            Ok(Some(original.clone()))
        })
        .unwrap();
        assert_eq!(reads, 1);
        let updated = writes.values().next().unwrap();
        let mut expected = original.clone();
        let dict = expected.as_dictionary_mut().unwrap();
        dict.get_mut("64")
            .unwrap()
            .as_dictionary_mut()
            .unwrap()
            .insert("enabled".into(), plist::Value::Boolean(false));
        dict.insert(
            "new.key".into(),
            DefaultsValue::from_toml(&val("{ enabled = true }"))
                .unwrap()
                .to_plist(),
        );
        assert_eq!(updated, &expected);
        for request in &requests {
            assert!(
                request
                    .value
                    .matches(selected_value(Some(updated), request).unwrap().unwrap())
            );
        }
        let again = prepare_writes(&requests, |_, _, _| Ok(Some(updated.clone()))).unwrap();
        assert_eq!(again, writes);
    }

    #[test]
    fn test_patches_create_missing_dictionaries_but_reject_wrong_types() {
        let request = patch(&["64", "enabled"], DefaultsValue::Bool(false));
        let writes = prepare_writes(std::slice::from_ref(&request), |_, _, _| Ok(None)).unwrap();
        assert_eq!(
            writes.values().next().unwrap(),
            &DefaultsValue::from_toml(&val(r#"{ "64" = { enabled = false } }"#,))
                .unwrap()
                .to_plist()
        );
        assert_eq!(selected_value(None, &request).unwrap(), None);
        for value in [
            plist::Value::Boolean(true),
            DefaultsValue::from_toml(&val(r#"{ "64" = [true] }"#))
                .unwrap()
                .to_plist(),
        ] {
            assert!(selected_value(Some(&value), &request).is_err());
            assert!(
                prepare_writes(std::slice::from_ref(&request), |_, _, _| Ok(Some(
                    value.clone()
                )))
                .is_err()
            );
        }
    }

    #[test]
    fn test_patches_reject_overlapping_ownership() {
        let request = patch(&["64", "enabled"], DefaultsValue::Bool(false));
        for path in [
            None,
            Some(vec!["64".into()]),
            request.path.clone(),
            Some(vec!["64".into(), "enabled".into(), "child".into()]),
        ] {
            let other = DefaultsRequest {
                path,
                ..request.clone()
            };
            assert!(validate_requests(&[request.clone(), other]).is_err());
        }
        assert!(validate_requests(&[patch(&[], DefaultsValue::Bool(false))]).is_err());
        let any = DefaultsRequest {
            domain: "-g".into(),
            ..request.clone()
        };
        let global = DefaultsRequest {
            domain: "NSGlobalDomain".into(),
            path: None,
            ..request
        };
        assert!(validate_requests(&[any, global]).is_err());
    }

    #[test]
    fn test_patch_host_scopes_have_independent_ownership_and_reads() {
        let any = patch(&["enabled"], DefaultsValue::Bool(false));
        let current = DefaultsRequest {
            host: HostScope::Current,
            ..any.clone()
        };
        let whole_current = DefaultsRequest {
            path: None,
            ..current.clone()
        };
        assert!(validate_requests(&[any.clone(), whole_current]).is_ok());
        let mut scopes = vec![];
        let writes = prepare_writes(&[any, current], |_, _, host| {
            scopes.push(host);
            Ok(Some(
                DefaultsValue::from_toml(&val(if host == HostScope::Any {
                    "{ sibling = 1 }"
                } else {
                    "{ sibling = 2 }"
                }))
                .unwrap()
                .to_plist(),
            ))
        })
        .unwrap();
        assert_eq!(scopes, vec![HostScope::Any, HostScope::Current]);
        for ((_, _, host), value) in writes {
            let dict = value.as_dictionary().unwrap();
            assert_eq!(dict.get("enabled"), Some(&plist::Value::Boolean(false)));
            assert_eq!(
                dict.get("sibling").unwrap().as_signed_integer(),
                Some(if host == HostScope::Any { 1 } else { 2 })
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_patch_native_round_trip_and_preflight() {
        let domain = format!("com.mise.patch-test.{}", uuid::Uuid::now_v7());
        let original =
            DefaultsValue::from_toml(&val(r#"{ "64" = { enabled = true }, data = [1, 2] }"#))
                .unwrap();
        let whole = DefaultsRequest {
            domain: domain.clone(),
            path: None,
            value: original.clone(),
            ..patch(&["64"], DefaultsValue::Bool(false))
        };
        let request = DefaultsRequest {
            domain: domain.clone(),
            ..patch(&["64", "enabled"], DefaultsValue::Bool(false))
        };
        write_all(&[whole]).unwrap();
        let apply = write_all(std::slice::from_ref(&request));
        let current = read(&domain, &request.key, HostScope::Any);
        let status = status_sync(std::slice::from_ref(&request));
        // A later invalid patch must prevent an earlier valid write.
        let invalid = DefaultsRequest {
            path: Some(vec!["data".into(), "child".into()]),
            ..request.clone()
        };
        let valid = DefaultsRequest {
            value: DefaultsValue::Bool(true),
            ..request.clone()
        };
        let failed = write_all(&[valid, invalid]);
        let after_failed = read(&domain, &request.key, HostScope::Any);
        let cleanup = macos::remove(&domain, &request.key, HostScope::Any);
        apply.unwrap();
        let current = current.unwrap().unwrap();
        assert_eq!(
            current.as_dictionary().unwrap().get("data"),
            original.to_plist().as_dictionary().unwrap().get("data")
        );
        assert_eq!(status.unwrap()[0].state, DefaultsState::Set);
        assert!(failed.is_err());
        assert_eq!(after_failed.unwrap(), Some(current));
        cleanup.unwrap();
    }

    /// `status()` must not fail when keys don't exist yet — this is the
    /// expected path on a fresh macOS install.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_status_missing_keys_are_unset() {
        let reqs = vec![
            // key doesn't exist in a real domain
            DefaultsRequest {
                dock_apps: false,
                host: HostScope::Any,
                path: None,
                domain: "NSGlobalDomain".into(),
                key: "_mise_test_nonexistent_key_42".into(),
                value: DefaultsValue::Bool(true),
            },
            // domain doesn't exist at all
            DefaultsRequest {
                dock_apps: false,
                host: HostScope::Any,
                path: None,
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
            dock_apps: false,
            host: HostScope::Any,
            path: None,
            domain: domain.into(),
            key: key.into(),
            value: value.clone(),
        }])
        .unwrap();
        let current = read(domain, key, HostScope::Any);
        let cleanup = macos::remove(domain, key, HostScope::Any);

        assert_eq!(current.unwrap(), Some(value.to_plist()));
        cleanup.unwrap();
    }
}
