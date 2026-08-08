#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_normalize_os() {
        assert_eq!(normalize_os("macos"), "macos");
        assert_eq!(normalize_os("darwin"), "macos");
        assert_eq!(normalize_os("linux"), "linux");
        assert_eq!(normalize_os("windows"), "windows");
        assert_eq!(normalize_os("win"), "windows");
        assert_eq!(normalize_os("freebsd"), "freebsd");
    }

    #[test]
    fn test_normalize_arch() {
        assert_eq!(normalize_arch("arm64"), "arm64");
        assert_eq!(normalize_arch("aarch64"), "arm64");
        assert_eq!(normalize_arch("x64"), "x64");
        assert_eq!(normalize_arch("x86_64"), "x64");
        assert_eq!(normalize_arch("amd64"), "x64");
        assert_eq!(normalize_arch("riscv64"), "riscv64");
    }

    #[test]
    fn test_bare_os_matches_any_arch() {
        assert!(os_list_matches(&list(&["linux"]), "linux", "x64"));
        assert!(os_list_matches(&list(&["linux"]), "linux", "arm64"));
        assert!(os_list_matches(&list(&["macos"]), "macos", "arm64"));
        assert!(!os_list_matches(&list(&["linux"]), "macos", "arm64"));
        assert!(!os_list_matches(&list(&["macos"]), "linux", "x64"));
    }

    #[test]
    fn test_os_alias_entries_match_canonical_platform() {
        assert!(os_list_matches(&list(&["darwin"]), "macos", "arm64"));
        assert!(os_list_matches(&list(&["darwin"]), "macos", "x64"));
        assert!(os_list_matches(&list(&["win"]), "windows", "x64"));
        assert!(!os_list_matches(&list(&["darwin"]), "linux", "arm64"));
    }

    #[test]
    fn test_target_platform_aliases_are_normalized() {
        assert!(os_list_matches(&list(&["macos"]), "darwin", "arm64"));
        assert!(os_list_matches(&list(&["windows"]), "win", "x64"));
        assert!(os_list_matches(&list(&["linux/arm64"]), "linux", "aarch64"));
        assert!(os_list_matches(&list(&["linux/x64"]), "linux", "amd64"));
        assert!(os_list_matches(&list(&["linux/x64"]), "linux", "x86_64"));
    }

    #[test]
    fn test_os_arch_form_requires_both_to_match() {
        assert!(os_list_matches(&list(&["macos/arm64"]), "macos", "arm64"));
        assert!(!os_list_matches(&list(&["macos/arm64"]), "macos", "x64"));
        assert!(!os_list_matches(&list(&["macos/arm64"]), "linux", "arm64"));
        assert!(os_list_matches(&list(&["linux/x86_64"]), "linux", "x64"));
        assert!(os_list_matches(&list(&["darwin/aarch64"]), "macos", "arm64"));
        assert!(os_list_matches(&list(&["linux/amd64"]), "linux", "x64"));
    }

    #[test]
    fn test_any_entry_in_list_matching_is_enough() {
        let entries = list(&["linux", "macos/arm64"]);
        assert!(os_list_matches(&entries, "linux", "x64"));
        assert!(os_list_matches(&entries, "linux", "arm64"));
        assert!(os_list_matches(&entries, "macos", "arm64"));
        assert!(!os_list_matches(&entries, "macos", "x64"));
        assert!(!os_list_matches(&entries, "windows", "x64"));
    }

    #[test]
    fn test_empty_list_matches_nothing() {
        assert!(!os_list_matches(&[], "linux", "x64"));
        assert!(!os_list_matches(&[], "macos", "arm64"));
        assert!(!os_list_matches(&[], "windows", "x64"));
    }

    #[test]
    fn test_unknown_names_never_match() {
        assert!(!os_list_matches(&list(&["notanos"]), "linux", "x64"));
        assert!(!os_list_matches(&list(&["solaris"]), "macos", "arm64"));
        assert!(!os_list_matches(&list(&["linux/notanarch"]), "linux", "x64"));
        assert!(!os_list_matches(&list(&["notanos/x64"]), "linux", "x64"));
        assert!(os_list_matches(&list(&["notanos", "linux"]), "linux", "x64"));
    }

    #[test]
    fn test_malformed_os_arch_entries_never_match() {
        assert!(!os_list_matches(&list(&["linux/"]), "linux", "x64"));
        assert!(!os_list_matches(&list(&["/x64"]), "linux", "x64"));
        assert!(!os_list_matches(&list(&["linux/arm64/extra"]), "linux", "arm64"));
        assert!(!os_list_matches(&list(&[""]), "linux", "x64"));
    }

    #[test]
    fn test_os_list_matches_current_with_current_platform() {
        assert!(os_list_matches_current(&list(&[std::env::consts::OS])));
        let current_os_arch = format!(
            "{}/{}",
            crate::cli::version::OS.as_str(),
            crate::cli::version::ARCH.as_str()
        );
        assert!(os_list_matches_current(&[current_os_arch]));
    }

    #[test]
    fn test_os_list_matches_current_mismatch() {
        assert!(!os_list_matches_current(&[]));
        assert!(!os_list_matches_current(&list(&["notanos"])));
        let other_os = if std::env::consts::OS == "linux" {
            "macos"
        } else {
            "linux"
        };
        assert!(!os_list_matches_current(&list(&[other_os])));
    }
}
