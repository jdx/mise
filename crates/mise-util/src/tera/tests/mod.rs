use super::*;
use confique::Layer;
use pretty_assertions::assert_str_eq;

mod basics;
mod compatibility;
mod functions;
mod targets;

struct SettingsGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl SettingsGuard {
    fn tera_v1() -> Self {
        let lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
        let mut settings = mise_settings::SettingsPartial::empty();
        settings.tera_v1 = Some(true);
        crate::testing::reset_settings(Some(settings));
        Self { _lock: lock }
    }
}

impl Drop for SettingsGuard {
    fn drop(&mut self) {
        crate::testing::reset_settings(None);
    }
}

/// `get_tera` under the settings lock. The engine is picked from the
/// `tera_v1` setting when the `Tera` is built, so holding the lock for
/// the build keeps a concurrent `SettingsGuard::tera_v1()` from handing a
/// v2 test the v1 engine.
fn locked_tera(dir: Option<&Path>) -> TeraEngine {
    let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
    get_tera(dir)
}

/// The `get_tera_for_target` counterpart of [`locked_tera`].
fn locked_tera_for_target(dir: Option<&Path>, os: &str, arch: &str) -> TeraEngine {
    let _lock = crate::testing::lock_ignoring_poison(&crate::testing::SETTINGS_LOCK);
    get_tera_for_target(dir, os, arch)
}

fn render(s: &str) -> String {
    let config_root = Path::new("/");
    let mut tera_ctx = BASE_CONTEXT.clone();
    tera_ctx.insert("config_root", &config_root);
    tera_ctx.insert("cwd", "/");
    let mut tera = locked_tera(Option::from(config_root));
    render_str(&mut tera, s, &tera_ctx).unwrap()
}

fn render_for_target(s: &str, os: &str, arch: &str) -> String {
    let mut tera_ctx = BASE_CONTEXT.clone();
    tera_ctx.insert("cwd", "/");
    let mut tera = locked_tera_for_target(None, os, arch);
    render_str(&mut tera, s, &tera_ctx).unwrap()
}

/// Render through the v1 engine explicitly. `render` goes through
/// `get_tera`, which picks the engine from settings; selecting v1 here
/// directly keeps these tests from mutating process-wide state.
fn render_v1(s: &str) -> String {
    let mut tera = TeraEngine::V1(Box::new(get_tera_v1(None)));
    let mut tera_ctx = BASE_CONTEXT.clone();
    tera_ctx.insert("cwd", "/");
    render_str(&mut tera, s, &tera_ctx).unwrap()
}

/// The v2 counterpart of [`render_v1`]. A cross-engine comparison must pin
/// both sides, or with `tera_v1` set it would compare v1 against itself.
fn render_v2(s: &str) -> String {
    let mut tera = TeraEngine::V2(Box::new(get_tera_v2(None)));
    let mut tera_ctx = BASE_CONTEXT.clone();
    tera_ctx.insert("cwd", "/");
    render_str(&mut tera, s, &tera_ctx).unwrap()
}
