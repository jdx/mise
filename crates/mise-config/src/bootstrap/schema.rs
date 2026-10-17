//! The JSON schema for the `[bootstrap]` fields that stay raw TOML, written by hand.
//!
//! Hooks, shell activation, the macOS defaults domains and the apt and dnf repositories are kept
//! as TOML values so a shape from a newer mise warns instead of failing the whole file, so there
//! is no Rust type to derive them from. The derived `[bootstrap]` schema points at these through
//! [`refs`](crate::schema::refs). Change them together with the code that reads those values, then
//! run `mise run render:schema`.

use schemars::{Schema, json_schema};

/// Each definition, keyed by its name under `$defs` in `schema/mise.json`.
pub(crate) fn defs() -> Vec<(&'static str, Schema)> {
    vec![
        ("bootstrap_file_phase", bootstrap_file_phase()),
        ("bootstrap_hook", bootstrap_hook()),
        ("bootstrap_shell_activation", bootstrap_shell_activation()),
        ("bootstrap_apt_sources", bootstrap_apt_sources()),
        ("bootstrap_dnf_repos", bootstrap_dnf_repos()),
        ("bootstrap_hooks", bootstrap_hooks()),
        ("bootstrap_macos_defaults", bootstrap_macos_defaults()),
        ("bootstrap_macos_dock", bootstrap_macos_dock()),
        ("bootstrap_macos_finder", bootstrap_macos_finder()),
        ("bootstrap_macos_keyboard", bootstrap_macos_keyboard()),
        ("bootstrap_macos_trackpad", bootstrap_macos_trackpad()),
        (
            "bootstrap_mise_shell_activate",
            bootstrap_mise_shell_activate(),
        ),
        ("macos_defaults_value", macos_defaults_value()),
    ]
}

fn bootstrap_file_phase() -> Schema {
    json_schema!({
        "type": "string",
        "enum": ["pre-packages", "post-packages"],
        "default": "post-packages",
        "description": "When to apply the managed path during bootstrap; pre-packages runs before the pre-packages hook"
    })
}

fn bootstrap_hook() -> Schema {
    json_schema!({
        "description": "command or commands to run for a bootstrap phase",
        "oneOf": [
            {"type": "string"},
            {"type": "array", "items": {"type": "string"}},
            {
                "type": "object",
                "properties": {
                    "run": {
                        "oneOf": [
                            {"type": "string"},
                            {"type": "array", "items": {"type": "string"}}
                        ]
                    }
                },
                "required": ["run"],
                "additionalProperties": true
            }
        ]
    })
}

fn bootstrap_shell_activation() -> Schema {
    json_schema!({
        "oneOf": [
            {
                "type": "boolean",
                "description": "enable the target with its default mode, or disable it"
            },
            {
                "type": "string",
                "enum": ["activate", "shims", "auto"],
                "description": "enable the target with the selected activation mode"
            },
            {
                "type": "object",
                "description": "shell activation settings for the target",
                "properties": {
                    "enabled": {
                        "type": "boolean",
                        "description": "whether to manage the activation block"
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["activate", "shims", "auto"],
                        "description": "activation mode; defaults according to the target"
                    }
                },
                "required": ["enabled"],
                "additionalProperties": false
            }
        ]
    })
}

fn bootstrap_apt_sources() -> Schema {
    json_schema!({
        "type": "object",
        "description": "apt repositories added before apt packages are installed, keyed by a name used in the files mise writes",
        "propertyNames": {"pattern": "^[A-Za-z0-9_-][A-Za-z0-9_.-]*$"},
        "additionalProperties": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "ppa": {
                    "type": "string",
                    "description": "Launchpad PPA as owner/archive, such as git-core/ppa; added on Ubuntu and Ubuntu-based releases only"
                },
                "uris": {"type": "string", "description": "repository URL(s), space-separated"},
                "suites": {
                    "type": "string",
                    "description": "suite(s), space-separated; defaults to the release codename"
                },
                "components": {
                    "type": "string",
                    "description": "component(s), space-separated; defaults to main, or none when suites is a path ending in /"
                },
                "architectures": {
                    "type": "string",
                    "description": "architecture(s) to fetch, space-separated"
                },
                "key": {
                    "type": "string",
                    "description": "http(s) URL of the repository's signing key, armored or binary"
                },
                "fingerprint": {
                    "description": "full primary key fingerprint(s) the signing key must have; required with key",
                    "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
                }
            },
            "oneOf": [
                {
                    "required": ["ppa"],
                    "properties": {"fingerprint": {"type": "string"}},
                    "not": {
                        "anyOf": [
                            {"required": ["uris"]},
                            {"required": ["suites"]},
                            {"required": ["components"]},
                            {"required": ["architectures"]},
                            {"required": ["key"]}
                        ]
                    }
                },
                {"required": ["uris", "key", "fingerprint"], "not": {"required": ["ppa"]}}
            ]
        }
    })
}

fn bootstrap_dnf_repos() -> Schema {
    json_schema!({
        "type": "object",
        "description": "dnf repositories enabled before dnf packages are installed, keyed by a name for the entry",
        "propertyNames": {"pattern": "^[A-Za-z0-9_-][A-Za-z0-9_.-]*$"},
        "additionalProperties": {
            "type": "object",
            "additionalProperties": false,
            "required": ["copr"],
            "properties": {
                "copr": {
                    "type": "string",
                    "description": "COPR project as owner/project or @group/project, such as scottames/ghostty; enabled as dnf copr enable would"
                }
            }
        }
    })
}

fn bootstrap_hooks() -> Schema {
    json_schema!({
        "type": "object",
        "description": "commands to run before and after bootstrap phases",
        "properties": {
            "pre-packages": {"$ref": "#/$defs/bootstrap_hook"},
            "pre_packages": {"$ref": "#/$defs/bootstrap_hook"},
            "post-packages": {"$ref": "#/$defs/bootstrap_hook"},
            "post_packages": {"$ref": "#/$defs/bootstrap_hook"},
            "pre-repos": {"$ref": "#/$defs/bootstrap_hook"},
            "pre_repos": {"$ref": "#/$defs/bootstrap_hook"},
            "post-repos": {"$ref": "#/$defs/bootstrap_hook"},
            "post_repos": {"$ref": "#/$defs/bootstrap_hook"},
            "pre-dotfiles": {"$ref": "#/$defs/bootstrap_hook"},
            "pre_dotfiles": {"$ref": "#/$defs/bootstrap_hook"},
            "post-dotfiles": {"$ref": "#/$defs/bootstrap_hook"},
            "post_dotfiles": {"$ref": "#/$defs/bootstrap_hook"},
            "pre-defaults": {"$ref": "#/$defs/bootstrap_hook"},
            "pre_defaults": {"$ref": "#/$defs/bootstrap_hook"},
            "post-defaults": {"$ref": "#/$defs/bootstrap_hook"},
            "post_defaults": {"$ref": "#/$defs/bootstrap_hook"},
            "pre-user": {"$ref": "#/$defs/bootstrap_hook"},
            "pre_user": {"$ref": "#/$defs/bootstrap_hook"},
            "post-user": {"$ref": "#/$defs/bootstrap_hook"},
            "post_user": {"$ref": "#/$defs/bootstrap_hook"},
            "pre-tools": {"$ref": "#/$defs/bootstrap_hook"},
            "pre_tools": {"$ref": "#/$defs/bootstrap_hook"},
            "post-tools": {"$ref": "#/$defs/bootstrap_hook"},
            "post_tools": {"$ref": "#/$defs/bootstrap_hook"},
            "final": {"$ref": "#/$defs/bootstrap_hook"}
        },
        "additionalProperties": false
    })
}

fn bootstrap_macos_defaults() -> Schema {
    json_schema!({
        "type": "object",
        "description": "macOS user defaults to apply with `mise bootstrap macos defaults apply`, keyed by preferences domain (e.g. \"com.apple.dock\", \"NSGlobalDomain\")",
        "additionalProperties": {
            "type": "object",
            "description": "defaults keys and their desired values for this domain",
            "additionalProperties": {
                "$ref": "#/$defs/macos_defaults_value",
                "description": "desired value, written with the matching `defaults write` type; arrays and tables become property list arrays and dictionaries"
            }
        }
    })
}

fn bootstrap_macos_dock() -> Schema {
    json_schema!({
        "type": "object",
        "description": "curated Dock preferences that compile into macOS defaults",
        "properties": {
            "apps": {
                "type": "array",
                "items": {"type": "string", "pattern": "^(?:/|~/(?!/)).+\\.[aA][pP][pP]/*$"},
                "uniqueItems": true,
                "description": "Pinned application paths in order; absolute or ~/ paths ending in .app. An empty array removes application tiles."
            },
            "autohide_delay": {"type": "number", "description": "delay before showing the Dock"},
            "autohide_time_modifier": {
                "type": "number",
                "description": "Dock autohide animation time modifier"
            },
            "autohide": {"type": "boolean", "description": "hide and show the Dock automatically"},
            "orientation": {
                "type": "string",
                "enum": ["bottom", "left", "right"],
                "description": "Dock screen edge"
            },
            "tilesize": {"type": "integer", "description": "Dock icon size"},
            "magnification": {"type": "boolean", "description": "enable Dock magnification"},
            "largesize": {"type": "integer", "description": "Dock magnified icon size"},
            "show_recents": {
                "type": "boolean",
                "description": "show recent applications in the Dock"
            },
            "mru_spaces": {
                "type": "boolean",
                "description": "automatically rearrange Spaces based on most recent use"
            }
        },
        "additionalProperties": false
    })
}

fn bootstrap_macos_finder() -> Schema {
    json_schema!({
        "type": "object",
        "description": "curated Finder preferences that compile into macOS defaults",
        "properties": {
            "sort_folders_first": {
                "type": "boolean",
                "description": "keep folders first when sorting by name"
            },
            "save_new_documents_to_cloud": {
                "type": "boolean",
                "description": "save new documents to iCloud by default (applies globally to application save dialogs)"
            },
            "show_all_files": {"type": "boolean", "description": "show hidden files in Finder"},
            "show_pathbar": {"type": "boolean", "description": "show the Finder path bar"},
            "show_status_bar": {"type": "boolean", "description": "show the Finder status bar"},
            "show_extensions_warning": {
                "type": "boolean",
                "description": "show the warning when changing file extensions"
            },
            "preferred_view_style": {
                "type": "string",
                "enum": ["icon", "list", "column", "gallery"],
                "description": "Finder preferred view style"
            }
        },
        "additionalProperties": false
    })
}

fn bootstrap_macos_keyboard() -> Schema {
    json_schema!({
        "type": "object",
        "description": "curated keyboard preferences that compile into macOS defaults",
        "properties": {
            "automatic_capitalization": {
                "type": "boolean",
                "description": "automatically capitalize words"
            },
            "automatic_spelling_correction": {
                "type": "boolean",
                "description": "automatically correct spelling"
            },
            "key_repeat": {"type": "integer", "description": "keyboard repeat interval"},
            "initial_key_repeat": {
                "type": "integer",
                "description": "delay before key repeat starts"
            },
            "press_and_hold": {
                "type": "boolean",
                "description": "enable press-and-hold accent picker"
            },
            "fn_state": {
                "type": "boolean",
                "description": "use F1, F2, etc. as standard function keys"
            }
        },
        "additionalProperties": false
    })
}

fn bootstrap_macos_trackpad() -> Schema {
    json_schema!({
        "type": "object",
        "description": "curated trackpad preferences that compile into macOS defaults",
        "properties": {
            "tap_to_click": {
                "type": "boolean",
                "description": "enable tap to click for built-in and Bluetooth trackpads"
            },
            "three_finger_drag": {
                "type": "boolean",
                "description": "enable three finger drag for built-in and Bluetooth trackpads"
            }
        },
        "additionalProperties": false
    })
}

fn bootstrap_mise_shell_activate() -> Schema {
    json_schema!({
        "type": "object",
        "description": "declarative mise shell activation snippets, applied with `mise bootstrap mise-shell-activate apply`",
        "properties": {
            "bash": {"$ref": "#/$defs/bootstrap_shell_activation"},
            "zsh": {"$ref": "#/$defs/bootstrap_shell_activation"},
            "fish": {"$ref": "#/$defs/bootstrap_shell_activation"},
            "bash_profile": {"$ref": "#/$defs/bootstrap_shell_activation"},
            "bashrc": {"$ref": "#/$defs/bootstrap_shell_activation"},
            "zshenv": {"$ref": "#/$defs/bootstrap_shell_activation"},
            "zprofile": {"$ref": "#/$defs/bootstrap_shell_activation"},
            "zshrc": {"$ref": "#/$defs/bootstrap_shell_activation"}
        },
        "additionalProperties": false
    })
}

fn macos_defaults_value() -> Schema {
    json_schema!({
        "description": "a property list value: boolean, integer, real, string, or an array or dictionary of these",
        "anyOf": [
            {"type": "boolean"},
            {"type": "integer"},
            {"type": "number"},
            {"type": "string"},
            {"type": "array", "items": {"$ref": "#/$defs/macos_defaults_value"}},
            {"type": "object", "additionalProperties": {"$ref": "#/$defs/macos_defaults_value"}}
        ]
    })
}
