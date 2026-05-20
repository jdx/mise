use demand::Theme;

use crate::config::Settings;

/// Returns the demand theme based on the `color_theme` setting.
///
/// Available themes:
/// - "default" or "charm" - Default charm theme (good for dark terminals)
/// - "base16" - Base16 theme (good for light terminals)
/// - "catppuccin" - Catppuccin theme
/// - "dracula" - Dracula theme
pub fn get_theme() -> Theme {
    let settings = Settings::get();
    if !console::colors_enabled_stderr() {
        return no_color_theme();
    }
    match settings.color_theme.to_lowercase().as_str() {
        "base16" => Theme::base16(),
        "catppuccin" => Theme::catppuccin(),
        "dracula" => Theme::dracula(),
        "charm" | "default" | "" => Theme::charm(),
        other => {
            warn!("Unknown color theme '{}', using default", other);
            Theme::charm()
        }
    }
}

fn no_color_theme() -> Theme {
    let mut theme = Theme::new();
    theme.title = Default::default();
    theme.description = Default::default();
    theme.cursor = Default::default();
    theme.selected_option = Default::default();
    theme.selected_prefix_fg = Default::default();
    theme.unselected_option = Default::default();
    theme.unselected_prefix_fg = Default::default();
    theme.error_indicator = Default::default();
    theme.input_cursor = Default::default();
    theme.input_placeholder = Default::default();
    theme.input_prompt = Default::default();
    theme.help_key = Default::default();
    theme.help_desc = Default::default();
    theme.help_sep = Default::default();
    theme.focused_button = Default::default();
    theme.blurred_button = Default::default();
    theme.cursor_style = Default::default();
    theme.force_style = false;
    theme.breadcrumb_active = Default::default();
    theme.breadcrumb_clickable = Default::default();
    theme.breadcrumb_future = Default::default();
    theme
}

#[cfg(test)]
mod tests {
    use confique::Layer;

    use crate::config::Settings;
    use crate::config::settings::SettingsPartial;

    use super::*;

    #[test]
    fn get_theme_returns_no_color_theme_when_stderr_colors_are_disabled() {
        let mut partial = SettingsPartial::empty();
        partial.color = Some(false);
        partial.color_theme = Some("base16".to_string());
        Settings::reset(Some(partial));

        let theme = get_theme();
        let cursor_color = theme.real_cursor_color(None);

        for color in [
            &theme.title,
            &theme.description,
            &theme.cursor,
            &theme.selected_option,
            &theme.selected_prefix_fg,
            &theme.unselected_option,
            &theme.unselected_prefix_fg,
            &theme.error_indicator,
            &theme.input_cursor,
            &theme.input_placeholder,
            &theme.input_prompt,
            &theme.help_key,
            &theme.help_desc,
            &theme.help_sep,
            &theme.focused_button,
            &theme.blurred_button,
            &theme.cursor_style,
            &theme.breadcrumb_active,
            &theme.breadcrumb_clickable,
            &theme.breadcrumb_future,
        ] {
            assert!(color.fg().is_none());
            assert!(color.bg().is_none());
        }
        assert!(cursor_color.fg().is_none());
        assert!(cursor_color.bg().is_none());

        Settings::reset(None);
    }
}
