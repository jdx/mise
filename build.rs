#![allow(unknown_lints)]
#![deny(dead_code_pub_in_binary, unreachable_pub)]

use serde::Serialize as _;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use aqua_registry::encode_package_rkyv;
use aqua_registry::types::{AquaPackage, AquaPackageType, RegistryPackageRow, RegistryYaml};
use eyre::{Result, eyre};

#[path = "build/lockfile_rollout.rs"]
mod lockfile_rollout;
#[path = "build/registry_url.rs"]
mod registry_url;

fn main() -> Result<()> {
    let release = (
        env!("CARGO_PKG_VERSION_MAJOR").parse::<u32>()?,
        env!("CARGO_PKG_VERSION_MINOR").parse::<u32>()?,
    );
    lockfile_rollout::check_release(release.0, release.1);
    cfg_aliases::cfg_aliases! {
        asdf: { any(feature = "asdf", not(target_os = "windows")) },
        macos: { target_os = "macos" },
        linux: { target_os = "linux" },
        vfox: { any(feature = "vfox", target_os = "windows") },
    }
    built::write_built_file()?;
    link_without_pie();
    build_enclave_helper()?;

    let aqua_registry = load_aqua_registry()?;
    codegen_registry(&aqua_registry.packages);
    codegen_aqua_standard_registry(&aqua_registry)?;
    Ok(())
}

/// Release builds for Linux GNU set `MISE_NO_PIE=1` (see scripts/build-tarball.sh)
/// to link the `mise` executable at a fixed address. As a position-independent
/// executable, mise makes the dynamic loader patch about 300k pointers on every
/// launch, which copies roughly 2k pages and dominates the startup of short
/// commands such as `hook-env`. Linked non-PIE, those pointers are final in the
/// file. Dependencies are still compiled position-independent; the flag reaches
/// only bin targets, so no shared library is linked with it. musl is left out on
/// purpose: its static-PIE start code crashes when linked with `-no-pie`, and
/// the alternative, `-C relocation-model=static`, is not something a build
/// script can set.
fn link_without_pie() {
    println!("cargo:rerun-if-env-changed=MISE_NO_PIE");
    let linux_gnu = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
    if linux_gnu && env::var("MISE_NO_PIE").as_deref() == Ok("1") {
        println!("cargo:rustc-link-arg-bins=-no-pie");
    }
}

/// The Secure Enclave helper (`src/enclave/macos.swift`) keeps the GitHub OAuth
/// refresh token behind Touch ID. Persisting an enclave key outside the keychain
/// needs CryptoKit, so unlike the notification helper it is Swift. A build
/// without `swiftc` still works: the helper is then reported as unavailable and
/// the token stays in the cache file.
fn build_enclave_helper() -> Result<()> {
    let source = "src/enclave/macos.swift";
    let info = "src/enclave/macos.plist";
    println!("cargo:rerun-if-changed={source}");
    println!("cargo:rerun-if-changed={info}");
    println!("cargo:rerun-if-env-changed=MISE_NOTIFICATION_SIGN_IDENTITY");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return Ok(());
    }
    let app = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("mise-enclave.app");
    let contents = app.join("Contents");
    let output = contents.join("MacOS/mise-enclave");
    fs::create_dir_all(contents.join("MacOS"))?;
    fs::create_dir_all(contents.join("_CodeSignature"))?;
    fs::copy(info, contents.join("Info.plist"))?;
    let identity = env::var("MISE_NOTIFICATION_SIGN_IDENTITY").unwrap_or_else(|_| "-".into());
    let built = Command::new("swiftc")
        .args(["-O", "-target"])
        .arg(format!(
            "{}-apple-macosx13.0",
            env::var("CARGO_CFG_TARGET_ARCH")?.replace("aarch64", "arm64")
        ))
        .arg(source)
        .arg("-o")
        .arg(&output)
        .output();
    let ok = match &built {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            println!(
                "cargo:warning=failed to build the macOS Secure Enclave helper: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            false
        }
        Err(err) => {
            println!("cargo:warning=swiftc unavailable, skipping the Secure Enclave helper: {err}");
            false
        }
    };
    // Releases are the only builds whose helper is signed with an identity the
    // enclave accepts, so an ad-hoc signed helper is reported as unavailable.
    let signed = ok && identity != "-";
    println!(
        "cargo:rustc-env=MISE_ENCLAVE_RELEASE_SIGNED={}",
        if signed { "1" } else { "0" }
    );
    if !ok {
        fs::write(&output, b"")?;
        fs::write(contents.join("_CodeSignature/CodeResources"), b"")?;
        return Ok(());
    }
    let mut codesign = Command::new("/usr/bin/codesign");
    codesign
        .args(["--force", "--sign"])
        .arg(&identity)
        .args(["--identifier", "dev.jdx.mise.enclave"]);
    if identity != "-" {
        codesign.args(["--options", "runtime", "--timestamp"]);
    }
    let signed_out = codesign.arg(&app).output()?;
    if !signed_out.status.success() {
        return Err(eyre!(
            "failed to sign the macOS Secure Enclave helper: {}",
            String::from_utf8_lossy(&signed_out.stderr).trim()
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct AquaPackageRegistry {
    id: String,
    content: Vec<u8>,
    aliases: Vec<String>,
    search_backend: Option<String>,
    search_overrides: Vec<AquaSearchOverrideRegistry>,
}

#[derive(Debug)]
struct AquaSearchOverrideRegistry {
    goos: Option<String>,
    goarch: Option<String>,
    envs: Vec<String>,
    libc: Option<String>,
    backend: Option<String>,
}

/// Generate a raw string literal that safely contains the given content.
/// Dynamically determines the minimum number of '#' needed.
fn raw_string_literal(s: &str) -> String {
    // Find the longest sequence of '#' characters following a '"' in the string
    let mut max_hashes = 0;
    let mut current_hashes = 0;
    let mut after_quote = false;

    for c in s.chars() {
        if after_quote {
            if c == '#' {
                current_hashes += 1;
                max_hashes = max_hashes.max(current_hashes);
            } else {
                after_quote = false;
                current_hashes = 0;
            }
        }
        if c == '"' {
            after_quote = true;
            current_hashes = 0;
        }
    }

    // Use one more '#' than the longest sequence found
    let hashes = "#".repeat(max_hashes + 1);
    format!("r{hashes}\"{s}\"{hashes}")
}

/// Parse options from a TOML value into a Vec of (key, value) pairs
fn parse_options(opts: Option<&toml::Value>) -> Vec<(String, String)> {
    opts.map(|opts| {
        if let Some(table) = opts.as_table() {
            table
                .iter()
                .map(|(k, v)| {
                    let mut value = String::new();
                    v.serialize(toml::ser::ValueSerializer::new(&mut value))
                        .unwrap_or_else(|e| panic!("failed to serialize registry option {k}: {e}"));
                    (k.clone(), value)
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        }
    })
    .unwrap_or_default()
}

fn load_registry_tools() -> toml::map::Map<String, toml::Value> {
    let mut tools = toml::map::Map::new();
    let registry_dir = Path::new("registry");

    println!("cargo:rerun-if-changed=registry");

    let mut files: Vec<_> = fs::read_dir(registry_dir)
        .expect("registry directory not found")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();

    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        let tool_name = file
            .file_stem()
            .expect("file has no stem")
            .to_str()
            .expect("filename is not valid UTF-8")
            .to_string();
        let content = fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", file.display(), e));
        let tool_info: toml::Value = toml::de::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", file.display(), e));
        tools.insert(tool_name, tool_info);
    }
    tools
}

fn codegen_registry(aqua_packages: &[RegistryPackageRow]) {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("registry.rs");
    let mut generated_entries = BTreeMap::new();

    let tools = load_registry_tools();
    let aqua_bins = aqua_package_bin_names(aqua_packages);
    for (short, info) in &tools {
        let info = info.as_table().unwrap();
        let version_order = match info.get("version_order").and_then(|v| v.as_str()) {
            Some("source") => "VersionOrder::Source",
            Some("semver") => "VersionOrder::Semver",
            Some(_) => panic!("[{short}] 'version_order' must be 'source' or 'semver'"),
            None => {
                panic!("[{short}] 'version_order' must be explicitly set to 'source' or 'semver'")
            }
        };
        let aliases = info
            .get("aliases")
            .cloned()
            .unwrap_or(toml::Value::Array(vec![]))
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        let test = info.get("test").map(|t| {
            let t = t
                .as_table()
                .unwrap_or_else(|| panic!("[{short}] 'test' field must be a table"));
            let cmd = t
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("[{short}] 'test.cmd' must be a string"))
                .to_string();
            let expected = t
                .get("expected")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("[{short}] 'test.expected' must be a string"))
                .to_string();
            let tools = t
                .get("tools")
                .map(|tools| {
                    let mut tools = tools
                        .as_array()
                        .unwrap_or_else(|| panic!("[{short}] 'test.tools' must be an array"))
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .unwrap_or_else(|| {
                                    panic!("[{short}] 'test.tools' must contain only strings")
                                })
                                .to_string()
                        })
                        .collect::<Vec<_>>();
                    tools.sort();
                    tools
                })
                .unwrap_or_default();
            (cmd, expected, tools)
        });
        let mut backends = vec![];
        // Each entry's backend and `[min_version, max_version)` range, to
        // check that a backend listed more than once never overlaps itself.
        type Range = (
            String,
            Vec<String>,
            Option<semver::Version>,
            Option<semver::Version>,
            Vec<(String, String)>,
        );
        let mut ranges: Vec<Range> = vec![];
        for backend in info.get("backends").unwrap().as_array().unwrap() {
            match backend {
                toml::Value::String(backend) => {
                    let backend = backend
                        .strip_prefix("pipx:")
                        .map(|name| format!("pypi:{name}"))
                        .unwrap_or_else(|| backend.clone());
                    ranges.push((backend.clone(), vec![], None, None, vec![]));
                    backends.push(format!(
                        r##"RegistryBackend{{
                            full: r#"{backend}"#,
                            platforms: &[],
                            min_version: None,
                            max_version: None,
                            attestations_since: None,
                            options: &[],
                        }}"##
                    ));
                }
                toml::Value::Table(backend) => {
                    let full = backend.get("full").unwrap().as_str().unwrap();
                    let full = full
                        .strip_prefix("pipx:")
                        .map(|name| format!("pypi:{name}"))
                        .unwrap_or_else(|| full.to_owned());
                    let platforms = backend
                        .get("platforms")
                        .map(|p| {
                            p.as_array()
                                .unwrap()
                                .iter()
                                .map(|p| p.as_str().unwrap().to_string())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let version_bound = |key: &str| {
                        backend.get(key).map(|value| {
                            let value = value
                                .as_str()
                                .unwrap_or_else(|| panic!("backend {key} must be a string"));
                            assert_eq!(
                                version_order, "VersionOrder::Semver",
                                "[{short}] backend {key} requires version_order = semver"
                            );
                            let version = semver::Version::parse(value).unwrap_or_else(|_| {
                                panic!("[{short}] backend {key} must be a semantic version")
                            });
                            (value.to_string(), version)
                        })
                    };
                    let min_version = version_bound("min_version");
                    let max_version = version_bound("max_version");
                    if let (Some((_, minimum)), Some((_, maximum))) = (&min_version, &max_version) {
                        assert!(
                            minimum.cmp_precedence(maximum).is_lt(),
                            "[{short}] backend min_version must be lower than max_version"
                        );
                    }
                    let backend_options = parse_options(backend.get("options"));
                    ranges.push((
                        full.clone(),
                        platforms.clone(),
                        min_version.as_ref().map(|(_, v)| v.clone()),
                        max_version.as_ref().map(|(_, v)| v.clone()),
                        backend_options.clone(),
                    ));
                    let bound_literal = |bound: Option<(String, semver::Version)>| {
                        bound
                            .map(|(value, _)| format!("Some({})", raw_string_literal(&value)))
                            .unwrap_or_else(|| "None".to_string())
                    };
                    let min_version = bound_literal(min_version);
                    let max_version = bound_literal(max_version);
                    let attestations_since = backend
                        .get("attestations_since")
                        .map(|value| {
                            let value = value
                                .as_str()
                                .expect("backend attestations_since must be a string");
                            assert!(
                                full.starts_with("github:"),
                                "[{short}] backend attestations_since is only supported for github: backends"
                            );
                            semver::Version::parse(value)
                                .expect("backend attestations_since must be a semantic version");
                            format!("Some({})", raw_string_literal(value))
                        })
                        .unwrap_or_else(|| "None".to_string());
                    backends.push(format!(
                        r##"RegistryBackend{{
                            full: r#"{full}"#,
                            platforms: &[{platforms}],
                            min_version: {min_version},
                            max_version: {max_version},
                            attestations_since: {attestations_since},
                            options: &[{options}],
                        }}"##,
                        platforms = platforms
                            .into_iter()
                            .map(|p| format!("\"{p}\""))
                            .collect::<Vec<_>>()
                            .join(", "),
                        options = backend_options
                            .iter()
                            .map(|(k, v)| format!(
                                "({}, {})",
                                raw_string_literal(k),
                                raw_string_literal(v)
                            ))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                _ => panic!("Unknown backend type"),
            }
        }
        // `lower < upper`, where a missing lower bound is -inf and a missing upper +inf.
        let below =
            |lower: &Option<semver::Version>, upper: &Option<semver::Version>| match (lower, upper)
            {
                (Some(lower), Some(upper)) => lower.cmp_precedence(upper).is_lt(),
                _ => true,
            };
        for (i, (full, platforms, min, max, options)) in ranges.iter().enumerate() {
            for (_, other_platforms, other_min, other_max, other_options) in
                ranges[i + 1..].iter().filter(|(f, ..)| f == full)
            {
                assert!(
                    options == other_options
                        || !(below(min, other_max) && below(other_min, max))
                        || platforms_disjoint(platforms, other_platforms),
                    "[{short}] backend {full} is listed more than once with different options for overlapping min_version/max_version ranges"
                );
            }
        }
        let os = info
            .get("os")
            .map(|os| {
                let os = os.as_array().unwrap();
                let mut os = os
                    .iter()
                    .map(|o| o.as_str().unwrap().to_string())
                    .collect::<Vec<_>>();
                os.sort();
                os
            })
            .unwrap_or_default();
        let description = info
            .get("description")
            .map(|d| d.as_str().unwrap().to_string());
        let url = info.get("url").map(|url| {
            let url = url
                .as_str()
                .unwrap_or_else(|| panic!("[{short}] 'url' must be a string"));
            assert!(
                registry_url::is_project_url(url),
                "[{short}] 'url' must be a project homepage or repository URL, not a download template"
            );
            url.to_string()
        });
        let deprecated = info.get("deprecated").map(|deprecated| {
            deprecated
                .as_str()
                .unwrap_or_else(|| panic!("[{short}] 'deprecated' must be a string"))
                .to_string()
        });
        let bins = info
            .get("bins")
            .map(|bins| {
                bins.as_array()
                    .unwrap_or_else(|| panic!("[{short}] 'bins' must be an array"))
                    .iter()
                    .map(|bin| {
                        bin.as_str()
                            .unwrap_or_else(|| panic!("[{short}] 'bins' must contain only strings"))
                            .to_string()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                let preferred_aqua_package = info
                    .get("backends")
                    .and_then(toml::Value::as_array)
                    .and_then(|backends| backends.first())
                    .and_then(registry_backend_full)
                    .and_then(|backend| backend.strip_prefix("aqua:"));
                let Some(package) = preferred_aqua_package else {
                    return vec![];
                };
                let bins = aqua_bins.get(package).unwrap_or_else(|| {
                    panic!("[{short}] preferred Aqua package {package:?} was not found")
                });
                assert!(
                    !bins.is_empty(),
                    "[{short}] preferred Aqua package {package:?} has no inferred bins"
                );
                bins.clone()
            });
        let idiomatic_files = info
            .get("idiomatic_files")
            .map(|idiomatic_files| {
                idiomatic_files
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| match f {
                        toml::Value::String(path) => format!(
                            "RegistryIdiomaticFile {{ path: {}, version_regex: None, version_json_path: None, version_expr: None, deprecated: None }}",
                            raw_string_literal(path)
                        ),
                        toml::Value::Table(spec) => {
                            for key in spec.keys() {
                                assert!(
                                    matches!(
                                        key.as_str(),
                                        "path"
                                            | "version_regex"
                                            | "version_json_path"
                                            | "version_expr"
                                            | "deprecated"
                                    ),
                                    "[{short}] unknown idiomatic file field: {key}"
                                );
                            }
                            let required = |key: &str| {
                                spec.get(key)
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_else(|| {
                                        panic!(
                                            "[{short}] idiomatic_files.{key} must be a string"
                                        )
                                    })
                            };
                            let optional = |key: &str| {
                                spec.get(key)
                                    .map(|value| {
                                        value.as_str().unwrap_or_else(|| {
                                            panic!(
                                                "[{short}] idiomatic_files.{key} must be a string"
                                            )
                                        })
                                    })
                                    .map(raw_string_literal)
                                    .map(|value| format!("Some({value})"))
                                    .unwrap_or_else(|| "None".to_string())
                            };
                            format!(
                                "RegistryIdiomaticFile {{ path: {}, version_regex: {}, version_json_path: {}, version_expr: {}, deprecated: {} }}",
                                raw_string_literal(required("path")),
                                optional("version_regex"),
                                optional("version_json_path"),
                                optional("version_expr"),
                                optional("deprecated"),
                            )
                        }
                        _ => panic!(
                            "[{short}] idiomatic_files entries must be strings or tables"
                        ),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let detect = info
            .get("detect")
            .map(|detect| {
                detect
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let overrides = info
            .get("overrides")
            .map(|overrides| {
                overrides
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let rt = format!(
            r#"RegistryTool{{short: "{short}", description: {description}, url: {url}, deprecated: {deprecated}, version_order: {version_order}, backends: &[{backends}], bins: &[{bins}], aliases: &[{aliases}], test: &{test}, os: &[{os}], idiomatic_files: &[{idiomatic_files}], detect: &[{detect}], overrides: &[{overrides}]}}"#,
            version_order = version_order,
            description = description
                .map(|d| format!("Some({})", raw_string_literal(&d)))
                .unwrap_or("None".to_string()),
            url = url
                .map(|url| format!("Some({})", raw_string_literal(&url)))
                .unwrap_or("None".to_string()),
            deprecated = deprecated
                .map(|deprecated| format!("Some({})", raw_string_literal(&deprecated)))
                .unwrap_or("None".to_string()),
            backends = backends.into_iter().collect::<Vec<_>>().join(", "),
            bins = bins
                .iter()
                .map(|bin| raw_string_literal(bin))
                .collect::<Vec<_>>()
                .join(", "),
            aliases = aliases
                .iter()
                .map(|a| format!("\"{a}\""))
                .collect::<Vec<_>>()
                .join(", "),
            test = test
                .map(|(cmd, expected, tools)| format!(
                    "Some(RegistryToolTest{{ cmd: {}, expected: {}, tools: &[{}] }})",
                    raw_string_literal(&cmd),
                    raw_string_literal(&expected),
                    tools
                        .iter()
                        .map(|tool| format!("\"{tool}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .unwrap_or("None".to_string()),
            os = os
                .iter()
                .map(|o| format!("\"{o}\""))
                .collect::<Vec<_>>()
                .join(", "),
            idiomatic_files = idiomatic_files.to_vec().join(", "),
            detect = detect
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(", "),
            overrides = overrides
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(", "),
        );
        generated_entries.insert(short.clone(), rt.clone());
        for alias in aliases {
            generated_entries.insert(alias, rt.clone());
        }
    }

    let entries = generated_entries.into_iter().collect::<Vec<_>>();
    fs::write(&dest_path, registry_code(&entries)).unwrap();
}

fn registry_code(entries: &[(String, String)]) -> String {
    let mut code = String::from("Registry {\n    entries: &[\n");
    for (key, tool) in entries {
        code.push_str(&format!("        ({key:?}, {tool}),\n"));
    }
    code.push_str("    ],\n    lookup: RegistryLookup::Static(");
    code.push_str(&phf_usize_map_code(
        entries
            .iter()
            .enumerate()
            .map(|(index, (key, _))| (key.clone(), index.to_string()))
            .collect::<Vec<_>>(),
    ));
    code.push_str("),\n    missing_version_order: false,\n}");
    code
}

fn phf_usize_map_code(entries: Vec<(String, String)>) -> String {
    let mut map = phf_codegen::Map::new();
    for (key, value) in &entries {
        map.entry(key, value);
    }
    map.build().to_string()
}

fn load_aqua_registry() -> Result<RegistryYaml> {
    let registry_file = Path::new("vendor/aqua-registry/registry.yml");
    println!("cargo:rerun-if-changed={}", registry_file.display());
    Ok(RegistryYaml::from_yaml(&fs::read_to_string(
        registry_file,
    )?)?)
}

fn registry_backend_full(backend: &toml::Value) -> Option<&str> {
    match backend {
        toml::Value::String(backend) => Some(backend),
        toml::Value::Table(backend) => backend.get("full")?.as_str(),
        _ => None,
    }
}

fn aqua_package_bin_names(rows: &[RegistryPackageRow]) -> HashMap<String, Vec<String>> {
    let mut bins = HashMap::new();
    for row in rows {
        let Some(id) = aqua_canonical_package_id(&row.package) else {
            continue;
        };
        let package_bins = row.package.possible_bin_names();
        bins.insert(id, package_bins.clone());
        for alias in &row.aliases {
            bins.insert(alias.clone(), package_bins.clone());
        }
    }
    bins
}

fn codegen_aqua_standard_registry(registry_yaml: &RegistryYaml) -> Result<()> {
    let out_dir = env::var("OUT_DIR")?;
    let files_dest_path = Path::new(&out_dir).join("aqua_standard_registry_files.rs");
    let aliases_dest_path = Path::new(&out_dir).join("aqua_standard_registry_aliases.rs");
    let search_dest_path = Path::new(&out_dir).join("aqua_standard_registry_search.rs");
    let metadata_dest_path = Path::new(&out_dir).join("aqua_standard_registry_metadata.rs");
    let packages_dir = Path::new(&out_dir).join("aqua_standard_registry_packages");

    let metadata_file = Path::new("vendor/aqua-registry/metadata.json");

    println!("cargo:rerun-if-changed={}", metadata_file.display());

    let registries = aqua_package_registries(&registry_yaml.packages)?;
    if registries.is_empty() {
        return Err(eyre!(
            "Aqua registry file {} contains no packages",
            "vendor/aqua-registry/registry.yml"
        ));
    }

    fs::create_dir_all(&packages_dir)?;
    fs::write(
        files_dest_path,
        aqua_registry_files_code(&registries, &packages_dir)?,
    )?;
    fs::write(aliases_dest_path, aqua_registry_aliases_code(&registries)?)?;
    fs::write(search_dest_path, aqua_registry_search_code(&registries))?;

    #[derive(serde::Deserialize)]
    struct AquaRegistryMetadata {
        repository: Option<String>,
        tag: Option<String>,
    }
    let metadata: AquaRegistryMetadata =
        serde_saphyr::from_str(&fs::read_to_string(metadata_file)?)?;
    let repository = metadata.repository.ok_or_else(|| {
        eyre!(
            "Aqua registry metadata file {} does not contain a repository",
            metadata_file.display()
        )
    })?;
    let tag = metadata.tag.ok_or_else(|| {
        eyre!(
            "Aqua registry metadata file {} does not contain a tag",
            metadata_file.display()
        )
    })?;

    fs::write(
        metadata_dest_path,
        format!("AquaRegistryMetadata {{ repository: {repository:?}, tag: {tag:?} }}"),
    )?;

    Ok(())
}

fn aqua_package_registries(rows: &[RegistryPackageRow]) -> Result<Vec<AquaPackageRegistry>> {
    let mut registries = Vec::new();
    let mut canonical_ids = HashMap::new();
    for (index, row) in rows.iter().enumerate() {
        let package = &row.package;
        let Some(id) = aqua_canonical_package_id(package) else {
            println!(
                "cargo:warning=skipping aqua registry package row {index}: missing name, repo_owner/repo_name, and path"
            );
            continue;
        };
        if let Some(existing) = canonical_ids.insert(id.clone(), index) {
            return Err(eyre!(
                "baked aqua registry package id collision for {id:?}: rows {existing} and {index}"
            ));
        }
        let content = encode_package_rkyv(package)?;
        let (effective_package, allow_go_repo_fallback) =
            package.clone().with_unconditional_version_override();
        let search_backend = aqua_search_backend(&effective_package, allow_go_repo_fallback);
        let mut search_overrides = effective_package
            .platform_overrides()
            .into_iter()
            .map(|package_override| AquaSearchOverrideRegistry {
                goos: package_override.goos,
                goarch: package_override.goarch,
                envs: package_override.envs,
                libc: package_override.libc,
                backend: aqua_search_backend(&package_override.package, allow_go_repo_fallback),
            })
            .collect::<Vec<_>>();
        if search_overrides
            .iter()
            .all(|package_override| package_override.backend == search_backend)
        {
            search_overrides.clear();
        }
        registries.push(AquaPackageRegistry {
            id,
            content,
            aliases: row.aliases.clone(),
            search_backend,
            search_overrides,
        });
    }
    Ok(registries)
}

fn aqua_search_backend(package: &AquaPackage, allow_go_repo_fallback: bool) -> Option<String> {
    match package.package_type() {
        AquaPackageType::Cargo => package
            .crate_name
            .as_deref()
            .or_else(|| {
                package
                    .name
                    .as_deref()
                    .and_then(|name| name.strip_prefix("crates.io/"))
            })
            .filter(|name| !is_aqua_template(name))
            .map(|name| format!("cargo:{name}"))
            .or(Some(String::new())),
        AquaPackageType::GoInstall => package
            .path
            .as_deref()
            .filter(|path| !is_aqua_template(path))
            .map(|path| format!("go:{path}"))
            .or_else(|| {
                (allow_go_repo_fallback
                    && !package.repo_owner.is_empty()
                    && !package.repo_name.is_empty()
                    && !is_aqua_template(&package.repo_owner)
                    && !is_aqua_template(&package.repo_name))
                .then(|| format!("go:github.com/{}/{}", package.repo_owner, package.repo_name))
            })
            .or(Some(String::new())),
        AquaPackageType::GoBuild => Some(String::new()),
        AquaPackageType::GithubArchive
        | AquaPackageType::GithubContent
        | AquaPackageType::GithubRelease
        | AquaPackageType::Http => None,
    }
}

fn is_aqua_template(value: &str) -> bool {
    value.contains("{{") || value.contains("}}")
}

fn aqua_registry_files_code(
    registries: &[AquaPackageRegistry],
    packages_dir: &Path,
) -> Result<String> {
    let mut used_stems = HashMap::new();
    let mut entries = registries
        .iter()
        .map(|registry| {
            let stem = aqua_package_file_stem(&registry.id);
            if let Some(other_id) = used_stems.insert(stem.clone(), registry.id.as_str()) {
                return Err(eyre!(
                    "baked aqua registry package filename collision for {other_id:?} and {:?}: {stem}",
                    registry.id
                ));
            }
            let filename = format!("{stem}.rkyv");
            let path = packages_dir.join(filename);
            fs::write(&path, &registry.content)?;
            Ok((registry.id.clone(), path))
        })
        .collect::<Result<Vec<_>>>()?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(aqua_registry_bytes_map_code(&entries))
}

fn aqua_registry_bytes_map_code(entries: &[(String, PathBuf)]) -> String {
    let mut map = phf_codegen::Map::new();
    let mut values = Vec::new();
    for (key, path) in entries {
        values.push((
            key.clone(),
            format!(
                "include_bytes!({:?}).as_slice()",
                path.display().to_string()
            ),
        ));
    }
    for (key, value) in &values {
        map.entry(key, value);
    }
    map.build().to_string()
}

/// Hashes the canonical package ID with FNV-1a 64-bit to generate compact,
/// deterministic baked package filenames without leaking path separators.
fn aqua_package_file_stem(id: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn aqua_registry_aliases_code(registries: &[AquaPackageRegistry]) -> Result<String> {
    let canonical_ids = registries
        .iter()
        .map(|registry| registry.id.as_str())
        .collect::<HashSet<_>>();
    let mut aliases = BTreeMap::new();

    for registry in registries {
        for alias in &registry.aliases {
            if alias != &registry.id
                && !canonical_ids.contains(alias.as_str())
                && let Some(existing) = aliases.insert(alias.clone(), registry.id.clone())
                && existing != registry.id
            {
                return Err(eyre!(
                    "baked aqua registry alias collision for {alias:?}: {existing:?} and {:?}",
                    registry.id
                ));
            }
        }
    }

    let entries = aliases.into_iter().collect::<Vec<_>>();

    Ok(aqua_registry_string_map_code(&entries))
}

fn aqua_registry_search_code(registries: &[AquaPackageRegistry]) -> String {
    let entries = registries
        .iter()
        .filter_map(|registry| {
            if registry.search_backend.is_none() && registry.search_overrides.is_empty() {
                return None;
            }
            let overrides = registry
                .search_overrides
                .iter()
                .map(|package_override| {
                    let envs = package_override
                        .envs
                        .iter()
                        .map(|env| format!("{env:?}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "AquaSearchBackendOverride {{ goos: {}, goarch: {}, envs: &[{envs}], libc: {}, backend: {} }}",
                        rust_option_str(package_override.goos.as_deref()),
                        rust_option_str(package_override.goarch.as_deref()),
                        rust_option_str(package_override.libc.as_deref()),
                        rust_option_str(package_override.backend.as_deref()),
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            Some((
                registry.id.clone(),
                format!(
                    "AquaSearchBackends {{ default: {}, overrides: &[{overrides}] }}",
                    rust_option_str(registry.search_backend.as_deref()),
                ),
            ))
        })
        .collect::<Vec<_>>();
    let mut map = phf_codegen::Map::new();
    for (key, value) in &entries {
        map.entry(key, value);
    }
    map.build().to_string()
}

fn rust_option_str(value: Option<&str>) -> String {
    value.map_or_else(|| "None".to_string(), |value| format!("Some({value:?})"))
}

fn aqua_registry_string_map_code(entries: &[(String, String)]) -> String {
    let mut map = phf_codegen::Map::new();
    let mut values = Vec::new();
    for (key, value) in entries {
        values.push((key.clone(), format!("{value:?}")));
    }
    for (key, value) in &values {
        map.entry(key, value);
    }
    map.build().to_string()
}

fn aqua_canonical_package_id(package: &AquaPackage) -> Option<String> {
    package
        .name
        .clone()
        .or_else(|| {
            if package.repo_owner.is_empty() || package.repo_name.is_empty() {
                None
            } else {
                Some(format!("{}/{}", package.repo_owner, package.repo_name))
            }
        })
        .or_else(|| package.path.clone())
}

/// Whether no platform matches both registry selector lists; mirrors
/// `registry::platforms_disjoint`.
fn platforms_disjoint(a: &[String], b: &[String]) -> bool {
    const ARCHES: [&str; 5] = ["x64", "arm64", "x86", "loongarch64", "riscv64"];
    let split = |selector: &str| -> (Option<String>, Option<String>) {
        match selector.split_once('-') {
            Some((os, arch)) => (Some(os.to_string()), Some(arch.to_string())),
            None if ARCHES.contains(&selector) => (None, Some(selector.to_string())),
            None => (Some(selector.to_string()), None),
        }
    };
    let differ =
        |x: &Option<String>, y: &Option<String>| matches!((x, y), (Some(x), Some(y)) if x != y);
    // Windows on arm64 also matches x64 selectors, as `backend_matches_platform` does.
    let windows_arm64 = |list: &[String]| {
        list.iter().any(|s| {
            matches!(
                s.as_str(),
                "windows" | "arm64" | "windows-arm64" | "x64" | "windows-x64"
            )
        })
    };
    if a.is_empty() || b.is_empty() || (windows_arm64(a) && windows_arm64(b)) {
        return false;
    }
    a.iter().all(|a| {
        let (a_os, a_arch) = split(a);
        b.iter().all(|b| {
            let (b_os, b_arch) = split(b);
            differ(&a_os, &b_os) || differ(&a_arch, &b_arch)
        })
    })
}
