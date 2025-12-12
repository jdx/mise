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
