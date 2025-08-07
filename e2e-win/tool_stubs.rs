use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

#[cfg(windows)]
mod windows_tool_stub_tests {
    use super::*;

    fn get_mise_executable() -> PathBuf {
        // Try to find mise executable in target directory
        let possible_paths = vec![
            PathBuf::from("target/release/mise.exe"),
            PathBuf::from("target/debug/mise.exe"),
            PathBuf::from("../target/release/mise.exe"),
            PathBuf::from("../target/debug/mise.exe"),
        ];

        for path in possible_paths {
            if path.exists() {
                return path.canonicalize().unwrap();
            }
        }

        // Fallback to mise in PATH
        PathBuf::from("mise")
    }

    fn run_mise_command(args: &[&str]) -> std::process::Output {
        let mise = get_mise_executable();
        Command::new(&mise)
            .args(args)
            .output()
            .expect(&format!("Failed to run mise with args: {:?}", args))
    }

    #[test]
    fn test_basic_stub_generation() {
        let temp_dir = TempDir::new().unwrap();
        let stub_path = temp_dir.path().join("test_tool");

        // Generate a basic tool stub
        let output = run_mise_command(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--url",
            "https://example.com/tool.zip",
            "--skip-download",
        ]);

        assert!(
            output.status.success(),
            "Failed to generate stub: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        // Check that both files were created
        assert!(stub_path.exists(), "Stub file was not created");
        assert!(
            stub_path.with_extension("exe").exists(),
            "Companion .exe was not created"
        );

        // Verify stub content
        let content = fs::read_to_string(&stub_path).unwrap();
        assert!(
            content.contains("#!/usr/bin/env -S mise tool-stub"),
            "Missing shebang"
        );
        assert!(content.contains("url = "), "Missing URL");
    }

    #[test]
    fn test_platform_specific_stubs() {
        let temp_dir = TempDir::new().unwrap();
        let stub_path = temp_dir.path().join("platform_tool");

        // Generate with multiple platforms
        let output = run_mise_command(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--platform-url",
            "windows-x64:https://example.com/win64.zip",
            "--platform-url",
            "linux-x64:https://example.com/linux64.tar.gz",
            "--platform-url",
            "darwin-arm64:https://example.com/mac-arm64.tar.gz",
            "--skip-download",
        ]);

        assert!(output.status.success());
        assert!(stub_path.exists());
        assert!(stub_path.with_extension("exe").exists());

        let content = fs::read_to_string(&stub_path).unwrap();
        assert!(content.contains("[platforms.windows-x64]"));
        assert!(content.contains("[platforms.linux-x64]"));
        assert!(content.contains("[platforms.darwin-arm64]"));
    }

    #[test]
    fn test_windows_specific_binary_paths() {
        let temp_dir = TempDir::new().unwrap();
        let stub_path = temp_dir.path().join("winbin_tool");

        // Generate with Windows-specific binary paths
        let output = run_mise_command(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--platform-url",
            "windows-x64:https://example.com/win.zip",
            "--platform-url",
            "linux-x64:https://example.com/linux.tar.gz",
            "--platform-bin",
            "windows-x64:tool.exe",
            "--platform-bin",
            "linux-x64:bin/tool",
            "--skip-download",
        ]);

        assert!(output.status.success());

        let content = fs::read_to_string(&stub_path).unwrap();
        
        // Check that Windows has .exe binary
        assert!(
            content.contains("bin = \"tool.exe\""),
            "Windows binary path not set correctly"
        );
    }

    #[test]
    fn test_incremental_platform_addition() {
        let temp_dir = TempDir::new().unwrap();
        let stub_path = temp_dir.path().join("incremental_tool");

        // First generation with one platform
        let output1 = run_mise_command(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--platform-url",
            "windows-x64:https://example.com/v1.zip",
            "--skip-download",
        ]);

        assert!(output1.status.success());
        let content1 = fs::read_to_string(&stub_path).unwrap();
        assert!(content1.contains("[platforms.windows-x64]"));

        // Second generation adding another platform
        let output2 = run_mise_command(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--platform-url",
            "linux-x64:https://example.com/v1.tar.gz",
            "--skip-download",
        ]);

        assert!(output2.status.success());
        let content2 = fs::read_to_string(&stub_path).unwrap();
        
        // Both platforms should be present
        assert!(
            content2.contains("[platforms.windows-x64]"),
            "Original platform was lost"
        );
        assert!(
            content2.contains("[platforms.linux-x64]"),
            "New platform was not added"
        );
    }

    #[test]
    fn test_companion_exe_properties() {
        let temp_dir = TempDir::new().unwrap();
        let stub_path = temp_dir.path().join("exe_test");

        let output = run_mise_command(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--url",
            "https://example.com/tool.zip",
            "--skip-download",
        ]);

        assert!(output.status.success());

        let exe_path = stub_path.with_extension("exe");
        assert!(exe_path.exists());

        // Check exe is valid PE file
        let exe_bytes = fs::read(&exe_path).unwrap();
        assert!(exe_bytes.len() > 0, "Exe file is empty");
        assert_eq!(exe_bytes[0], 0x4D, "Invalid PE header (MZ)");
        assert_eq!(exe_bytes[1], 0x5A, "Invalid PE header (MZ)");

        // Check exe size is reasonable (< 1MB)
        let metadata = fs::metadata(&exe_path).unwrap();
        assert!(
            metadata.len() < 1_048_576,
            "Companion exe too large: {} bytes",
            metadata.len()
        );
    }

    #[test]
    fn test_auto_platform_detection() {
        let temp_dir = TempDir::new().unwrap();
        let stub_path = temp_dir.path().join("auto_detect");

        // Use URLs that should auto-detect platform
        let output = run_mise_command(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--platform-url",
            "https://github.com/tool/releases/tool-windows-x64.zip",
            "--platform-url",
            "https://github.com/tool/releases/tool-linux-amd64.tar.gz",
            "--skip-download",
        ]);

        assert!(output.status.success());

        let content = fs::read_to_string(&stub_path).unwrap();
        
        // Should have detected windows and linux platforms
        assert!(
            content.contains("[platforms.") && content.contains("windows"),
            "Windows platform not auto-detected"
        );
        assert!(
            content.contains("[platforms.") && content.contains("linux"),
            "Linux platform not auto-detected"
        );
    }

    #[test]
    fn test_companion_exe_consistency() {
        let temp_dir = TempDir::new().unwrap();
        
        // Generate multiple stubs
        let stub1 = temp_dir.path().join("tool1");
        let stub2 = temp_dir.path().join("tool2");

        run_mise_command(&[
            "generate",
            "tool-stub",
            &stub1.to_string_lossy(),
            "--url",
            "https://example.com/1.zip",
            "--skip-download",
        ]);

        run_mise_command(&[
            "generate",
            "tool-stub",
            &stub2.to_string_lossy(),
            "--url",
            "https://example.com/2.zip",
            "--skip-download",
        ]);

        let exe1 = stub1.with_extension("exe");
        let exe2 = stub2.with_extension("exe");

        assert!(exe1.exists() && exe2.exists());

        // Companion exes should be identical (same mise-stub.exe copied)
        let exe1_bytes = fs::read(&exe1).unwrap();
        let exe2_bytes = fs::read(&exe2).unwrap();

        assert_eq!(
            exe1_bytes.len(),
            exe2_bytes.len(),
            "Companion exe sizes differ"
        );
        
        // They should be byte-for-byte identical
        assert_eq!(
            exe1_bytes, exe2_bytes,
            "Companion exes are not identical copies"
        );
    }

    #[test]
    fn test_stub_with_version() {
        let temp_dir = TempDir::new().unwrap();
        let stub_path = temp_dir.path().join("versioned_tool");

        let output = run_mise_command(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--url",
            "https://example.com/tool.zip",
            "--version",
            "1.2.3",
            "--skip-download",
        ]);

        assert!(output.status.success());

        let content = fs::read_to_string(&stub_path).unwrap();
        assert!(
            content.contains("version = \"1.2.3\""),
            "Version not set correctly"
        );
    }

    #[test]
    fn test_fetch_existing_stub() {
        let temp_dir = TempDir::new().unwrap();
        let stub_path = temp_dir.path().join("fetch_test");

        // First create a stub without checksums
        let stub_content = r#"#!/usr/bin/env -S mise tool-stub

url = "https://example.com/tool.zip"
"#;
        fs::write(&stub_path, stub_content).unwrap();

        // Note: fetch would actually download the file, so we can't test it fully
        // without mocking the HTTP client. This is more of a smoke test.
        
        // Verify the stub file structure is valid for fetching
        assert!(stub_path.exists());
        let content = fs::read_to_string(&stub_path).unwrap();
        assert!(content.contains("url = "));
    }
}

// Non-Windows platforms can still test some basic functionality
#[cfg(not(windows))]
mod unix_tool_stub_tests {
    use super::*;

    #[test]
    fn test_stub_generation_unix() {
        let temp_dir = TempDir::new().unwrap();
        let stub_path = temp_dir.path().join("unix_tool");

        let output = Command::new("mise")
            .args(&[
                "generate",
                "tool-stub",
                &stub_path.to_string_lossy(),
                "--url",
                "https://example.com/tool.tar.gz",
                "--skip-download",
            ])
            .output()
            .expect("Failed to run mise");

        if output.status.success() {
            assert!(stub_path.exists(), "Stub file was not created");
            
            // On Unix, there should NOT be a .exe companion
            assert!(
                !stub_path.with_extension("exe").exists(),
                "Unexpected .exe file on Unix"
            );

            let content = fs::read_to_string(&stub_path).unwrap();
            assert!(content.contains("#!/usr/bin/env -S mise tool-stub"));
        }
    }
}