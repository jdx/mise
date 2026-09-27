use demand::Theme;

use crate::config::Settings;

/// Returns the demand theme based on the `color_theme` setting and the current
/// terminal color state.
///
/// Available themes:
/// - "auto" (or "default") - Auto-detect: use a light-friendly theme (base16) on
///   light terminals, otherwise charm
/// - "charm" - The charm theme (good for dark terminals, no auto-detection)
/// - "base16" - Base16 theme (good for light terminals)
/// - "catppuccin" - Catppuccin theme
/// - "dracula" - Dracula theme
///
/// When colors are disabled (e.g. `color=false`, `NO_COLOR`, `CLICOLOR=0`) an
/// unstyled theme is returned, mirroring `demand::Theme::default()`'s own
/// behavior so that interactive prompts honor the color settings.
pub fn get_theme() -> Theme {
    let settings = Settings::get();
    select_theme(
        console::colors_enabled_stderr(),
        &settings.color_theme,
        detect_light_background(),
    )
}

/// Pure theme-selection logic, separated from global state so it can be tested
/// deterministically (the test harness forces colors off globally, so the
/// branches cannot be exercised through `get_theme()` directly).
fn select_theme(colors_enabled: bool, color_theme: &str, light_background: bool) -> Theme {
    // Honor disabled colors regardless of the requested theme. This matches
    // `demand::Theme::default()`, which returns `Theme::new()` (no colors) when
    // `console::colors_enabled_stderr()` is false.
    if !colors_enabled {
        return Theme::new();
    }
    let auto = || {
        if light_background {
            Theme::base16()
        } else {
            Theme::charm()
        }
    };
    match color_theme.to_lowercase().as_str() {
        "auto" | "default" | "" => auto(),
        "charm" => Theme::charm(),
        "base16" => Theme::base16(),
        "catppuccin" => Theme::catppuccin(),
        "dracula" => Theme::dracula(),
        other => {
            warn!("Unknown color theme '{}', using default", other);
            auto()
        }
    }
}

/// Best-effort detection of a light terminal background via the `COLORFGBG`
/// environment variable (set by many terminals, e.g. iTerm2, konsole, rxvt).
/// We intentionally avoid OSC 11 terminal queries, which can hang or corrupt
/// output. Returns false (assume dark) when detection is not possible.
fn detect_light_background() -> bool {
    std::env::var("COLORFGBG")
        .ok()
        .as_deref()
        .map(colorfgbg_is_light)
        .unwrap_or(false)
}

/// Parse a `COLORFGBG` value ("foreground;background", sometimes with a middle
/// field) and decide whether the background indicates a light terminal.
/// The background is the last `;`-separated field; ANSI codes 7 (white) and
/// 10..=15 (bright colors / bright white) are treated as light.
fn colorfgbg_is_light(value: &str) -> bool {
    value
        .rsplit(';')
        .next()
        .and_then(|bg| bg.trim().parse::<u8>().ok())
        .map(|bg| bg == 7 || (10..=15).contains(&bg))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_disabled_returns_unstyled_theme() {
        // Regardless of the requested theme, disabled colors -> no styling.
        for theme in [
            "default",
            "charm",
            "base16",
            "catppuccin",
            "dracula",
            "auto",
            "bogus",
        ] {
            let t = select_theme(false, theme, false);
            let plain = Theme::new();
            assert_eq!(t.title.fg(), plain.title.fg(), "theme={theme}");
            assert_eq!(
                t.selected_option.fg(),
                plain.selected_option.fg(),
                "theme={theme}"
            );
            assert_eq!(
                t.unselected_option.fg(),
                plain.unselected_option.fg(),
                "theme={theme}"
            );
        }
    }

    #[test]
    fn explicit_theme_is_applied_when_colors_enabled() {
        // base16's title color differs from charm's; confirm the explicit
        // request wins and is actually colored.
        let base16 = select_theme(true, "base16", false);
        assert_eq!(base16.title.fg(), Theme::base16().title.fg());
        assert!(base16.title.fg().is_some());

        let charm = select_theme(true, "charm", true /* ignored for explicit */);
        assert_eq!(charm.title.fg(), Theme::charm().title.fg());
    }

    #[test]
    fn auto_picks_base16_on_light_and_charm_on_dark() {
        for theme in ["auto", "default", ""] {
            let light = select_theme(true, theme, true);
            assert_eq!(
                light.title.fg(),
                Theme::base16().title.fg(),
                "theme={theme}"
            );

            let dark = select_theme(true, theme, false);
            assert_eq!(dark.title.fg(), Theme::charm().title.fg(), "theme={theme}");
        }
    }

    #[test]
    fn unknown_theme_falls_back_to_auto() {
        assert_eq!(
            select_theme(true, "bogus", true).title.fg(),
            Theme::base16().title.fg()
        );
        assert_eq!(
            select_theme(true, "bogus", false).title.fg(),
            Theme::charm().title.fg()
        );
    }

    #[test]
    fn colorfgbg_light_detection() {
        // Light backgrounds (white / bright)
        assert!(colorfgbg_is_light("0;15"));
        assert!(colorfgbg_is_light("0;7"));
        assert!(colorfgbg_is_light("0;default;15"));
        assert!(colorfgbg_is_light(" 0 ; 15 "));
        // Dark backgrounds
        assert!(!colorfgbg_is_light("15;0"));
        assert!(!colorfgbg_is_light("7;0"));
        assert!(!colorfgbg_is_light("15;8"));
        // Unparseable -> assume dark
        assert!(!colorfgbg_is_light(""));
        assert!(!colorfgbg_is_light("foo"));
        assert!(!colorfgbg_is_light("0;default"));
    }
}
