// use std::fs;
// use std::path::Path;
//
// use eyre::Result;
// use rayon::Scope;
//
// use crate::dirs::*;
// use crate::env::{XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_STATE_HOME};
// use crate::file;

pub fn run() {
    // rayon::scope(|s| {
    // task(s, || rename_plugin("nodejs", "node"));
    // task(s, || rename_plugin("golang", "go"));
    // task(s, migrate_trusted_configs);
    // task(s, migrate_tracked_configs);
    // task(s, || remove_deprecated_plugin("node", "rtx-nodejs"));
    // task(s, || remove_deprecated_plugin("go", "rtx-golang"));
    // task(s, || remove_deprecated_plugin("java", "rtx-java"));
    // task(s, || remove_deprecated_plugin("python", "rtx-python"));
    // task(s, || remove_deprecated_plugin("ruby", "rtx-ruby"));
    // });
}

// fn task(s: &Scope, job: impl FnOnce() -> Result<()> + Send + 'static) {
//     s.spawn(|_| {
//         if let Err(err) = job() {
//             eprintln!("[WARN] migrate: {}", err);
//         }
//     });
// }
//
// fn move_subdirs(from: &Path, to: &Path) -> Result<()> {
//     if from.exists() {
//         eprintln!("migrating {} to {}", from.display(), to.display());
//         file::create_dir_all(to)?;
//         for f in from.read_dir()? {
//             let f = f?.file_name();
//             let from_file = from.join(&f);
//             let to_file = to.join(&f);
//             if !to_file.exists() {
//                 eprintln!("moving {} to {}", from_file.display(), to_file.display());
//                 file::rename(from_file, to_file)?;
//             }
//         }
//         file::remove_all(from)?;
//     }
//
//     Ok(())
// }
//
// fn rename_plugin(from: &str, to: &str) -> Result<()> {
//     move_subdirs(&INSTALLS.join(from), &INSTALLS.join(to))?;
//     move_subdirs(&PLUGINS.join(from), &PLUGINS.join(to))?;
//     Ok(())
// }
//
// // fn migrate_tracked_configs() -> Result<()> {
// //     move_dirs(&DATA.join("tracked_config_files"), &TRACKED_CONFIGS)?;
// //     move_dirs(&DATA.join("tracked-config-files"), &TRACKED_CONFIGS)?;
// //     Ok(())
// // }
//
// // fn migrate_trusted_configs() -> Result<()> {
// //     move_dirs(&CACHE.join("trusted-configs"), &TRUSTED_CONFIGS)?;
// //     move_dirs(&CONFIG.join("trusted-configs"), &TRUSTED_CONFIGS)?;
// //     move_dirs(&DATA.join("trusted-configs"), &TRUSTED_CONFIGS)?;
// //     Ok(())
// // }
//
// fn move_dirs(from: &Path, to: &Path) -> Result<bool> {
//     if from.exists() && !to.exists() {
//         eprintln!("migrating {} to {}", from.display(), to.display());
//         file::create_dir_all(to.parent().unwrap())?;
//         file::rename(from, to)?;
//         Ok(true)
//     } else {
//         Ok(false)
//     }
// }
//
// // fn remove_deprecated_plugin(name: &str, plugin_name: &str) -> Result<()> {
// //     let plugin_root = PLUGINS.join(name);
// //     let gitconfig = plugin_root.join(".git").join("config");
// //     let gitconfig_body = fs::read_to_string(gitconfig).unwrap_or_default();
// //     if !gitconfig_body.contains(&format!("github.com/mise-plugins/{plugin_name}")) {
// //         return Ok(());
// //     }
// //     eprintln!("removing deprecated plugin {plugin_name}, will use core {name} plugin from now on");
// //     file::remove_all(plugin_root)?;
// //     Ok(())
// // }
