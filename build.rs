fn main() {
    cfg_aliases::cfg_aliases! {
        linux: { target_os = "linux" },
        macos: { target_os = "macos" },
        windows: { target_os = "windows" },

        vfox: { any(feature = "vfox", target_os = "windows") },
        asdf: { any(feature = "asdf", not(target_os = "windows")) },
    }
    built::write_built_file().expect("Failed to acquire build-time information");
}
