use std::fs;
use std::hint::black_box;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use aqua_registry::{AquaPackage, RegistryYaml};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use serde_yaml::Value;

const REGISTRY_YAML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/aqua-registry/registry.yaml"
));
const IDS: &[&str] = &[
    "01mf02/jaq",
    "hashicorp/terraform",
    "kubernetes/kubernetes/kubectl",
    "astral-sh/uv",
    "golang.org/x/perf/cmd/benchstat",
    "Automattic/harper/harper-ls",
];
const LARGE_REGISTRY_FILES: &[&str] = &[
    "WebAssembly/binaryen/registry.yaml",
    "commercialhaskell/stack/registry.yaml",
    "fastfetch-cli/fastfetch/registry.yaml",
    "cri-o/cri-o/registry.yaml",
];
const ITERS: usize = 10_000;

struct PackageFixture {
    id: String,
    package: AquaPackage,
    yaml: String,
    json: Vec<u8>,
    msgpack: Vec<u8>,
    msgpack_z: Vec<u8>,
}

fn main() {
    let fixtures = fixtures();

    println!("format,size_bytes,decode_ms,ns_per_decode,convert_ms,ns_per_convert");
    for fixture in &fixtures {
        let yaml_decode = bench(ITERS, || {
            let package: AquaPackage = serde_yaml::from_str(&fixture.yaml).unwrap();
            black_box(package.repo_name.len());
        });
        let json_decode = bench(ITERS, || {
            let package: AquaPackage = serde_json::from_slice(&fixture.json).unwrap();
            black_box(package.repo_name.len());
        });
        let msgpack_decode = bench(ITERS, || {
            let package: AquaPackage = rmp_serde::from_slice(&fixture.msgpack).unwrap();
            black_box(package.repo_name.len());
        });
        let msgpack_z_decode = bench(ITERS, || {
            let package = decode_msgpack_z(&fixture.msgpack_z);
            black_box(package.repo_name.len());
        });

        let yaml_convert = bench(ITERS, || {
            black_box(serde_yaml::to_string(&fixture.package).unwrap());
        });
        let json_convert = bench(ITERS, || {
            black_box(serde_json::to_vec(&fixture.package).unwrap());
        });
        let msgpack_convert = bench(ITERS, || {
            black_box(rmp_serde::to_vec_named(&fixture.package).unwrap());
        });
        let msgpack_z_convert = bench(ITERS, || {
            let msgpack = rmp_serde::to_vec_named(&fixture.package).unwrap();
            black_box(encode_msgpack_z(&msgpack));
        });

        print_result(
            &format!("{} yaml", fixture.id),
            fixture.yaml.len(),
            yaml_decode,
            yaml_convert,
        );
        print_result(
            &format!("{} json", fixture.id),
            fixture.json.len(),
            json_decode,
            json_convert,
        );
        print_result(
            &format!("{} msgpack", fixture.id),
            fixture.msgpack.len(),
            msgpack_decode,
            msgpack_convert,
        );
        print_result(
            &format!("{} msgpack_zlib", fixture.id),
            fixture.msgpack_z.len(),
            msgpack_z_decode,
            msgpack_z_convert,
        );
    }

    let yaml_total: usize = fixtures.iter().map(|fixture| fixture.yaml.len()).sum();
    let json_total: usize = fixtures.iter().map(|fixture| fixture.json.len()).sum();
    let msgpack_total: usize = fixtures.iter().map(|fixture| fixture.msgpack.len()).sum();
    let msgpack_z_total: usize = fixtures.iter().map(|fixture| fixture.msgpack_z.len()).sum();
    println!("total yaml size: {yaml_total}");
    println!("total json size: {json_total}");
    println!("total msgpack size: {msgpack_total}");
    println!("total msgpack_zlib size: {msgpack_z_total}");
}

fn fixtures() -> Vec<PackageFixture> {
    let registry: Value = serde_yaml::from_str(REGISTRY_YAML).unwrap();
    let packages = registry
        .get("packages")
        .and_then(Value::as_sequence)
        .unwrap();

    let mut fixtures: Vec<_> = IDS
        .iter()
        .map(|id| {
            let package_value = packages
                .iter()
                .find(|package| canonical_package_id(package).as_deref() == Some(id))
                .unwrap_or_else(|| panic!("missing fixture package {id}"));
            package_fixture((*id).to_string(), package_value)
        })
        .collect();

    fixtures.extend(large_registry_fixtures());
    fixtures
}

fn large_registry_fixtures() -> Vec<PackageFixture> {
    LARGE_REGISTRY_FILES
        .iter()
        .filter_map(|path| {
            let path = sibling_registry_path(path);
            let content = match fs::read_to_string(&path) {
                Ok(content) => content,
                Err(err) => {
                    eprintln!("skipping {}: {err}", path.display());
                    return None;
                }
            };
            let registry: RegistryYaml = serde_yaml::from_str(&content).unwrap();
            let package = registry.packages.into_iter().next().unwrap();
            let id = canonical_package_id_from_package(&package);
            Some(package_fixture_from_package(id, package))
        })
        .collect()
}

fn sibling_registry_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("aqua-registry/pkgs")
        .join(path)
}

fn package_fixture(id: String, package: &Value) -> PackageFixture {
    let package: AquaPackage = serde_yaml::from_value(package.clone()).unwrap();
    package_fixture_from_package(id, package)
}

fn package_fixture_from_package(id: String, package: AquaPackage) -> PackageFixture {
    let yaml = serde_yaml::to_string(&package).unwrap();
    let json = serde_json::to_vec(&package).unwrap();
    let msgpack = rmp_serde::to_vec_named(&package).unwrap();
    let msgpack_z = encode_msgpack_z(&msgpack);
    PackageFixture {
        id,
        package,
        yaml,
        json,
        msgpack,
        msgpack_z,
    }
}

fn canonical_package_id_from_package(package: &AquaPackage) -> String {
    package
        .name
        .clone()
        .unwrap_or_else(|| match package.path.as_ref() {
            Some(path) => path.clone(),
            None => format!("{}/{}", package.repo_owner, package.repo_name),
        })
}

fn canonical_package_id(package: &Value) -> Option<String> {
    string_field(package, "name")
        .or_else(|| {
            let repo_owner = string_field(package, "repo_owner")?;
            let repo_name = string_field(package, "repo_name")?;
            Some(format!("{repo_owner}/{repo_name}"))
        })
        .or_else(|| string_field(package, "path"))
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

fn encode_msgpack_z(msgpack: &[u8]) -> Vec<u8> {
    let mut zlib = ZlibEncoder::new(Vec::new(), Compression::best());
    zlib.write_all(msgpack).unwrap();
    zlib.finish().unwrap()
}

fn decode_msgpack_z(bytes: &[u8]) -> AquaPackage {
    let mut zlib = ZlibDecoder::new(bytes);
    let mut msgpack = Vec::new();
    zlib.read_to_end(&mut msgpack).unwrap();
    rmp_serde::from_slice(&msgpack).unwrap()
}

fn bench(iterations: usize, mut f: impl FnMut()) -> Duration {
    let started = Instant::now();
    for _ in 0..iterations {
        f();
    }
    started.elapsed()
}

fn print_result(name: &str, size: usize, decode: Duration, convert: Duration) {
    let decode_ns = decode.as_nanos() / ITERS as u128;
    let convert_ns = convert.as_nanos() / ITERS as u128;
    println!(
        "{name},{size},{:.3},{decode_ns},{:.3},{convert_ns}",
        decode.as_secs_f64() * 1000.0,
        convert.as_secs_f64() * 1000.0
    );
}
