use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn find_stub_file(exe_path: &Path) -> Option<PathBuf> {
    // Try to find the TOML stub file next to the executable
    // Remove .exe extension and look for file without extension or with common extensions
    let stem = exe_path.file_stem()?;
    let parent = exe_path.parent()?;

    // Try these patterns in order
    let possible_names = vec![
        stem.to_os_string(), // filename without .exe
        format!("{}.toml", stem.to_string_lossy()).into(),
        format!("{}.stub", stem.to_string_lossy()).into(),
    ];

    for name in possible_names {
        let stub_path = parent.join(&name);
        if stub_path.exists() && stub_path.is_file() {
            // Verify it's a valid stub file by checking for shebang or TOML content
            if let Ok(content) = fs::read_to_string(&stub_path) {
                if content.starts_with("#!")
                    || content.contains("version")
                    || content.contains("tool")
                {
                    return Some(stub_path);
                }
            }
        }
    }

    None
}

fn find_mise_executable() -> Option<PathBuf> {
    // First, try to find mise in PATH
    if let Ok(path_var) = env::var("PATH") {
        let paths = env::split_paths(&path_var);
        for path in paths {
            let mise_exe = if cfg!(windows) {
                path.join("mise.exe")
            } else {
                path.join("mise")
            };
            if mise_exe.exists() {
                return Some(mise_exe);
            }
        }
    }

    // Try common installation locations on Windows
    let mut common_locations = vec![
        PathBuf::from(r"C:\Program Files\mise\mise.exe"),
        PathBuf::from(r"C:\Program Files (x86)\mise\mise.exe"),
    ];

    if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
        common_locations.push(PathBuf::from(local_app_data).join(r"mise\bin\mise.exe"));
    }

    if let Ok(user_profile) = env::var("USERPROFILE") {
        common_locations.push(PathBuf::from(&user_profile).join(r".local\bin\mise.exe"));
        common_locations.push(PathBuf::from(&user_profile).join(r".cargo\bin\mise.exe"));
    }

    for location in common_locations {
        if location.exists() {
            return Some(location);
        }
    }

    None
}

fn main() -> ExitCode {
    // Get the path to this executable
    let exe_path = match env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Failed to get current executable path: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Find the corresponding stub file
    let stub_file = match find_stub_file(&exe_path) {
        Some(path) => path,
        None => {
            eprintln!("Failed to find tool stub file for: {}", exe_path.display());
            eprintln!("Expected to find a TOML stub file next to the executable");
            return ExitCode::FAILURE;
        }
    };

    // Find mise executable
    let mise_exe = match find_mise_executable() {
        Some(path) => path,
        None => {
            eprintln!("Failed to find mise executable in PATH or common locations");
            eprintln!("Please ensure mise is installed and available in your PATH");
            return ExitCode::FAILURE;
        }
    };

    // Build command: mise tool-stub <stub_file> [args...]
    let mut cmd = Command::new(&mise_exe);
    cmd.arg("tool-stub");
    cmd.arg(&stub_file);

    // Pass through all arguments from the original invocation
    let args: Vec<String> = env::args().skip(1).collect();
    for arg in args {
        cmd.arg(arg);
    }

    // Execute mise and propagate the exit code
    match cmd.status() {
        Ok(status) => {
            if let Some(code) = status.code() {
                // Use the exit code from mise
                match u8::try_from(code) {
                    Ok(code) => ExitCode::from(code),
                    _ => ExitCode::FAILURE,
                }
            } else {
                // Process was terminated by signal (unlikely on Windows)
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("Failed to execute mise: {e}");
            eprintln!(
                "Command: {} tool-stub {}",
                mise_exe.display(),
                stub_file.display()
            );
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_find_stub_file() {
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("mise_stub_test");
        fs::create_dir_all(&test_dir).unwrap();

        let exe_path = test_dir.join("test_tool.exe");
        let stub_path = test_dir.join("test_tool");

        // Create a stub file
        fs::write(
            &stub_path,
            "#!/usr/bin/env -S mise tool-stub\nversion = \"1.0.0\"",
        )
        .unwrap();

        // Test finding the stub
        let found = find_stub_file(&exe_path);
        assert_eq!(found, Some(stub_path.clone()));

        // Cleanup
        fs::remove_file(&stub_path).ok();
    }

    #[test]
    fn test_find_stub_file_with_toml_extension() {
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("mise_stub_test2");
        fs::create_dir_all(&test_dir).unwrap();

        let exe_path = test_dir.join("test_tool.exe");
        let stub_path = test_dir.join("test_tool.toml");

        // Create a stub file with .toml extension
        fs::write(&stub_path, "version = \"1.0.0\"\ntool = \"node\"").unwrap();

        // Test finding the stub
        let found = find_stub_file(&exe_path);
        assert_eq!(found, Some(stub_path.clone()));

        // Cleanup
        fs::remove_file(&stub_path).ok();
    }
}
