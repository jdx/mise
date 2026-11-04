pub use mise_config::doctor::{DoctorCheck, DoctorConfig};

/// Whether `check` runs on this host: it names no `os`, or one of its selectors matches.
pub fn applies(check: &DoctorCheck) -> bool {
    check.os.is_empty()
        || check
            .os
            .iter()
            .any(|os| crate::platform::os_selector_matches(os))
}
