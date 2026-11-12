use super::*;

#[test]
fn windows_drive_mounts_are_recognized() {
    for p in ["/mnt/c", "/mnt/c/Windows/System32", "/mnt/D/tools"] {
        assert!(is_windows_drive_mount(Path::new(p)), "{p}");
    }
    for p in [
        "/mnt",
        "/mnt/wsl",
        "/mnt/cc/x",
        "/usr/bin",
        "mnt/c/x",
        "/home/mnt/c",
        "/mnt/c/mise/../custom-bin",
        "/mnt/c/..",
    ] {
        assert!(!is_windows_drive_mount(Path::new(p)), "{p}");
    }
}
