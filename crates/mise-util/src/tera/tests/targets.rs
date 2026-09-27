use super::*;

#[tokio::test]
async fn test_os_arch_for_target() {
    // os()/arch() resolve to the requested target, not the host.
    assert_eq!(
        render_for_target("{{os()}}-{{arch()}}", "windows", "arm64"),
        "windows-arm64"
    );
    assert_eq!(render_for_target("{{os()}}", "macos", "x64"), "macos");
}
#[tokio::test]
async fn test_os_arch_remap_for_target() {
    // Remap arguments keep host semantics but apply to the target value.
    assert_eq!(
        render_for_target(
            r#"{{os(macos="darwin")}}_{{arch(x64="amd64")}}"#,
            "macos",
            "x64"
        ),
        "darwin_amd64"
    );
    // A remap that does not match the target value is ignored.
    assert_eq!(
        render_for_target(r#"{{arch(x64="amd64")}}"#, "linux", "arm64"),
        "arm64"
    );
}
#[tokio::test]
async fn test_os_family_for_target() {
    // os_family() follows the target, not the host.
    assert_eq!(
        render_for_target("{{os_family()}}", "windows", "x64"),
        "windows"
    );
    assert_eq!(render_for_target("{{os_family()}}", "linux", "x64"), "unix");
    assert_eq!(
        render_for_target("{{os_family()}}", "macos", "arm64"),
        "unix"
    );
}
#[tokio::test]
async fn test_preserving_os_arch_round_trips_through_target() {
    // A deferred os(...) remap survives config-load preservation and then
    // re-renders correctly for a target platform.
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("cwd", "/");
    let mut deferred = {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        get_tera_preserving_os_arch(None)
    };
    let preserved = render_str(&mut deferred, r#"{{ os(macos="darwin") }}"#, &ctx).unwrap();
    assert_eq!(preserved, r#"{{ os(macos="darwin") }}"#);
    let mut tera = locked_tera_for_target(None, "macos", "arm64");
    assert_eq!(render_str(&mut tera, &preserved, &ctx).unwrap(), "darwin");
}
#[tokio::test]
async fn test_preserving_os_family_round_trips_through_target() {
    // os_family() must be deferred at config-load time and resolve against
    // the lock target, not the host — otherwise a windows target locked from
    // a unix host would get "unix" baked in.
    let mut ctx = BASE_CONTEXT.clone();
    ctx.insert("cwd", "/");
    let mut deferred = {
        let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        get_tera_preserving_os_arch(None)
    };
    let preserved = render_str(&mut deferred, r#"{{ os_family() }}"#, &ctx).unwrap();
    assert_eq!(preserved, r#"{{ os_family() }}"#);
    let mut tera = locked_tera_for_target(None, "windows", "x64");
    assert_eq!(render_str(&mut tera, &preserved, &ctx).unwrap(), "windows");
}
