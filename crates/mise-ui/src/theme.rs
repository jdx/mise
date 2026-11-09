use demand::Theme;
use termcolor::{Color, ColorSpec};

use mise_settings::Settings;

/// Returns the demand theme based on the `color_theme` setting and the current
/// terminal color state.
///
/// Available themes:
/// - "auto" (or "default") - The base16 theme
/// - "base16" - Base16 theme: the terminal's own 16 named colors, so it follows
///   the terminal's palette on light and dark backgrounds
/// - "charm" - The charm theme (fixed 256-color greys, suits dark terminals)
/// - "catppuccin" - Catppuccin theme
/// - "dracula" - Dracula theme
///
/// When colors are disabled (e.g. `color=false`, `NO_COLOR`, `CLICOLOR=0`) an
/// unstyled theme is returned, mirroring `demand::Theme::default()`'s own
/// behavior so that interactive prompts honor the color settings.
pub fn get_theme() -> Theme {
    let settings = Settings::get();
    select_theme(console::colors_enabled_stderr(), &settings.color_theme)
}

/// Pure theme-selection logic, separated from global state so it can be tested
/// deterministically (the test harness forces colors off globally, so the
/// branches cannot be exercised through `get_theme()` directly).
fn select_theme(colors_enabled: bool, color_theme: &str) -> Theme {
    // Honor disabled colors regardless of the requested theme. This matches
    // `demand::Theme::default()`, which returns `Theme::new()` (no colors) when
    // `console::colors_enabled_stderr()` is false.
    if !colors_enabled {
        return Theme::new();
    }
    match color_theme.to_lowercase().as_str() {
        "auto" | "default" | "" => base16(),
        "charm" => Theme::charm(),
        "base16" => base16(),
        "catppuccin" => Theme::catppuccin(),
        "dracula" => Theme::dracula(),
        other => {
            log::warn!("Unknown color theme '{}', using default", other);
            base16()
        }
    }
}

/// demand's base16 theme with its `Ansi256(0..=15)` colors swapped for the
/// terminal's named colors (`ESC[3Xm`, and `ESC[9Xm` for the bright half), and
/// unselected options left in the terminal's default text color. demand's
/// unselected color, ANSI white, shows as pale grey on light themes.
fn base16() -> Theme {
    let mut theme = Theme::base16();
    for spec in [
        &mut theme.title,
        &mut theme.description,
        &mut theme.cursor,
        &mut theme.selected_prefix_fg,
        &mut theme.selected_option,
        &mut theme.input_cursor,
        &mut theme.input_placeholder,
        &mut theme.input_prompt,
        &mut theme.focused_button,
        &mut theme.blurred_button,
        &mut theme.error_indicator,
        &mut theme.breadcrumb_active,
        &mut theme.breadcrumb_clickable,
        &mut theme.breadcrumb_future,
    ] {
        name_ansi_colors(spec);
    }
    theme.unselected_prefix_fg = ColorSpec::new();
    theme.unselected_option = ColorSpec::new();
    theme
}

fn name_ansi_colors(spec: &mut ColorSpec) {
    let fg = spec.fg().copied();
    let bg = spec.bg().copied();
    // Intense is one flag for the whole spec, so a spec mixing a normal and a
    // bright color would lose one; base16 has none.
    let intense = [fg, bg]
        .into_iter()
        .flatten()
        .any(|c| matches!(c, Color::Ansi256(8..=15)));
    spec.set_fg(fg.map(named_color));
    spec.set_bg(bg.map(named_color));
    spec.set_intense(intense);
}

fn named_color(color: Color) -> Color {
    match color {
        Color::Ansi256(0 | 8) => Color::Black,
        Color::Ansi256(1 | 9) => Color::Red,
        Color::Ansi256(2 | 10) => Color::Green,
        Color::Ansi256(3 | 11) => Color::Yellow,
        Color::Ansi256(4 | 12) => Color::Blue,
        Color::Ansi256(5 | 13) => Color::Magenta,
        Color::Ansi256(6 | 14) => Color::Cyan,
        Color::Ansi256(7 | 15) => Color::White,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs(theme: &Theme) -> Vec<&ColorSpec> {
        vec![
            &theme.title,
            &theme.description,
            &theme.cursor,
            &theme.selected_prefix_fg,
            &theme.selected_option,
            &theme.unselected_prefix_fg,
            &theme.unselected_option,
            &theme.input_cursor,
            &theme.input_placeholder,
            &theme.input_prompt,
            &theme.focused_button,
            &theme.blurred_button,
            &theme.error_indicator,
            &theme.breadcrumb_active,
            &theme.breadcrumb_clickable,
            &theme.breadcrumb_future,
        ]
    }

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
            let t = select_theme(false, theme);
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
        let charm = select_theme(true, "charm");
        assert_eq!(charm.title.fg(), Theme::charm().title.fg());
        assert_eq!(
            charm.unselected_option.fg(),
            Theme::charm().unselected_option.fg()
        );
    }

    #[test]
    fn auto_and_unknown_themes_are_base16() {
        for theme in ["auto", "default", "", "base16", "bogus"] {
            let t = select_theme(true, theme);
            assert_eq!(t.title.fg(), Some(&Color::Cyan), "theme={theme}");
            assert!(t.title.bold(), "theme={theme}");
        }
    }

    #[test]
    fn base16_uses_named_colors_not_ansi256() {
        let theme = base16();
        for spec in specs(&theme) {
            for color in [spec.fg(), spec.bg()].into_iter().flatten() {
                assert!(
                    !matches!(color, Color::Ansi256(_)),
                    "base16 still has {color:?}"
                );
            }
        }
        assert_eq!(theme.selected_option.fg(), Some(&Color::Green));
        assert_eq!(theme.cursor.fg(), Some(&Color::Yellow));
        // bright black, as demand's Ansi256(8)
        assert_eq!(theme.description.fg(), Some(&Color::Black));
        assert!(theme.description.intense());
    }

    #[test]
    fn base16_leaves_unselected_options_in_the_terminal_text_color() {
        let theme = base16();
        for spec in [&theme.unselected_option, &theme.unselected_prefix_fg] {
            assert_eq!(spec.fg(), None);
            assert_eq!(spec.bg(), None);
        }
    }
}
