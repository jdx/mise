use std::hint::black_box;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use aqua_registry::AquaPackage;
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
const ITERS: usize = 10_000;

struct PackageFixture {
    id: &'static str,
    package: Value,
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

        let yaml_convert = Duration::ZERO;
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

    IDS.iter()
        .map(|id| {
            let package = packages
                .iter()
                .find(|package| canonical_package_id(package).as_deref() == Some(id))
                .unwrap_or_else(|| panic!("missing fixture package {id}"));
            let yaml = serde_yaml::to_string(package).unwrap();
            let json = serde_json::to_vec(package).unwrap();
            let msgpack = rmp_serde::to_vec_named(package).unwrap();
            let msgpack_z = encode_msgpack_z(&msgpack);
            PackageFixture {
                id,
                package: package.clone(),
                yaml,
                json,
                msgpack,
                msgpack_z,
            }
        })
        .collect()
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
