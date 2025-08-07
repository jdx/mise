use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[cfg(windows)]
#[test]
fn test_windows_tool_stub_generation() {
    let temp_dir = TempDir::new().unwrap();
    let stub_path = temp_dir.path().join("test_tool");
    
    // Generate a tool stub
    let output = std::process::Command::new("mise")
        .args(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--url",
            "https://example.com/tool.tar.gz",
            "--skip-download", // Skip actual download for testing
        ])
        .output()
        .expect("Failed to generate tool stub");
    
    assert!(output.status.success(), "Failed to generate tool stub: {}", String::from_utf8_lossy(&output.stderr));
    
    // Check that both the stub file and companion .exe were created
    assert!(stub_path.exists(), "Tool stub file was not created");
    assert!(stub_path.with_extension("exe").exists(), "Windows companion .exe was not created");
    
    // Verify the stub file contains valid TOML
    let content = fs::read_to_string(&stub_path).expect("Failed to read stub file");
    assert!(content.contains("#!/usr/bin/env -S mise tool-stub"));
    assert!(content.contains("url = \"https://example.com/tool.tar.gz\""));
}

#[cfg(windows)]
#[test]
fn test_windows_tool_stub_with_platforms() {
    let temp_dir = TempDir::new().unwrap();
    let stub_path = temp_dir.path().join("platform_tool");
    
    // Generate a tool stub with platform-specific URLs
    let output = std::process::Command::new("mise")
        .args(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--platform-url",
            "windows-x64:https://example.com/tool-windows.zip",
            "--platform-url",
            "linux-x64:https://example.com/tool-linux.tar.gz",
            "--skip-download",
        ])
        .output()
        .expect("Failed to generate tool stub");
    
    assert!(output.status.success(), "Failed to generate tool stub: {}", String::from_utf8_lossy(&output.stderr));
    
    // Check that files were created
    assert!(stub_path.exists(), "Tool stub file was not created");
    assert!(stub_path.with_extension("exe").exists(), "Windows companion .exe was not created");
    
    // Verify the stub file contains platform-specific configuration
    let content = fs::read_to_string(&stub_path).expect("Failed to read stub file");
    assert!(content.contains("[platforms.windows-x64]"));
    assert!(content.contains("[platforms.linux-x64]"));
    assert!(content.contains("url = \"https://example.com/tool-windows.zip\""));
    assert!(content.contains("url = \"https://example.com/tool-linux.tar.gz\""));
}

#[cfg(windows)]
#[test]
fn test_windows_stub_launcher_finds_stub_file() {
    use crate::windows_stub_launcher::{find_stub_file};
    
    let temp_dir = TempDir::new().unwrap();
    let exe_path = temp_dir.path().join("test_tool.exe");
    let stub_path = temp_dir.path().join("test_tool");
    
    // Create a stub file
    fs::write(&stub_path, "#!/usr/bin/env -S mise tool-stub\nversion = \"1.0.0\"").unwrap();
    
    // Test finding the stub
    let found = find_stub_file(&exe_path);
    assert_eq!(found, Some(stub_path));
}

#[cfg(windows)]
#[test]
fn test_windows_companion_exe_execution() {
    let temp_dir = TempDir::new().unwrap();
    let stub_path = temp_dir.path().join("echo_tool");
    
    // Create a simple tool stub that uses a system command
    let stub_content = r#"#!/usr/bin/env -S mise tool-stub
tool = "echo"
version = "latest"
"#;
    
    fs::write(&stub_path, stub_content).expect("Failed to write stub file");
    
    // Generate the companion .exe
    let output = std::process::Command::new("mise")
        .args(&[
            "generate",
            "tool-stub",
            &stub_path.to_string_lossy(),
            "--skip-download",
        ])
        .output()
        .expect("Failed to generate tool stub");
    
    assert!(output.status.success());
    
    let exe_path = stub_path.with_extension("exe");
    assert!(exe_path.exists(), "Companion .exe was not created");
    
    // Try to execute the companion .exe
    // Note: This would require mise to be in PATH and the echo tool to be available
    // For a full integration test, we'd need a more controlled environment
}