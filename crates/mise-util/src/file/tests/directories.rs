use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_all_dirs_no_ceiling() {
    let start_dir = Path::new("/a/b/c");
    let ceiling_dirs = HashSet::new();

    let result = all_dirs(start_dir, &ceiling_dirs).unwrap();

    assert_eq!(result.len(), 4);
    assert!(result.contains(&PathBuf::from("/a/b/c")));
    assert!(result.contains(&PathBuf::from("/a/b")));
    assert!(result.contains(&PathBuf::from("/a")));
    assert!(result.contains(&PathBuf::from("/")));
}

#[test]
fn test_all_dirs_with_ceiling() {
    let start_dir = Path::new("/a/b/c");
    let mut ceiling_dirs = HashSet::new();
    ceiling_dirs.insert(PathBuf::from("/a"));

    let result = all_dirs(start_dir, &ceiling_dirs).unwrap();

    assert_eq!(result.len(), 2);
    assert!(result.contains(&PathBuf::from("/a/b/c")));
    assert!(result.contains(&PathBuf::from("/a/b")));
    assert!(!result.contains(&PathBuf::from("/a")));
    assert!(!result.contains(&PathBuf::from("/")));
}

#[test]
fn test_all_dirs_with_ceiling_at_start() {
    let start_dir = Path::new("/a/b/c");
    let mut ceiling_dirs = HashSet::new();
    ceiling_dirs.insert(PathBuf::from("/a/b/c"));

    let result = all_dirs(start_dir, &ceiling_dirs).unwrap();

    assert_eq!(result.len(), 0);
}

#[test]
fn test_all_dirs_with_multiple_ceilings() {
    let start_dir = Path::new("/a/b/c/d/e");
    let mut ceiling_dirs = HashSet::new();
    ceiling_dirs.insert(PathBuf::from("/a/b"));
    ceiling_dirs.insert(PathBuf::from("/a/b/c/d"));

    let result = all_dirs(start_dir, &ceiling_dirs).unwrap();

    assert_eq!(result.len(), 1);
    assert!(result.contains(&PathBuf::from("/a/b/c/d/e")));
}

#[test]
fn test_all_dirs_with_relative_path() {
    let start_dir = Path::new("a/b/c");
    let ceiling_dirs = HashSet::new();

    let result = all_dirs(start_dir, &ceiling_dirs).unwrap();

    assert!(result.contains(&PathBuf::from("a/b/c")));
    assert!(result.contains(&PathBuf::from("a/b")));
    assert!(result.contains(&PathBuf::from("a")));
}

#[test]
fn test_all_dirs_stops_at_a_ceiling_spelled_another_way() {
    // `real/sub/..` resolves to `real` as a symlink would, without needing the
    // privilege to make one on Windows, where std's resolved form starts with `\\?\`.
    let tmp = tempfile::TempDir::new().unwrap();
    let real = dunce::canonicalize(tmp.path()).unwrap().join("real");
    let start = real.join("sub").join("deeper");
    std::fs::create_dir_all(&start).unwrap();
    let ceiling_dirs: HashSet<PathBuf> = ceiling_path_forms(&real.join("sub").join("..")).collect();

    let result = all_dirs(&start, &ceiling_dirs).unwrap();

    assert_eq!(result, vec![start.clone(), real.join("sub")]);
}
