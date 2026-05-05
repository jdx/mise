use std::collections::HashMap;
use std::hint::black_box;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use aqua_registry::AquaPackage;
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use rkyv::rancor::Error as RkyvError;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde_derive::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use serde_yaml::Value;

const REGISTRY_YAML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/aqua-registry/registry.yml"
));
const IDS: &[&str] = &[
    "01mf02/jaq",
    "hashicorp/terraform",
    "WebAssembly/binaryen",
    "commercialhaskell/stack",
    "fastfetch-cli/fastfetch",
    "cri-o/cri-o",
];
const ITERS: usize = 10_000;

struct PackageFixture {
    id: String,
    package: AquaPackage,
    rkyv_package: RkyvAquaPackage,
    yaml: String,
    json: Vec<u8>,
    msgpack: Vec<u8>,
    rkyv: Vec<u8>,
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
        let rkyv_decode = bench(ITERS, || {
            let package: RkyvAquaPackage =
                rkyv::from_bytes::<RkyvAquaPackage, RkyvError>(&fixture.rkyv).unwrap();
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
        let rkyv_convert = bench(ITERS, || {
            black_box(rkyv::to_bytes::<RkyvError>(&fixture.rkyv_package).unwrap());
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
            &format!("{} rkyv", fixture.id),
            fixture.rkyv.len(),
            rkyv_decode,
            rkyv_convert,
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
    let rkyv_total: usize = fixtures.iter().map(|fixture| fixture.rkyv.len()).sum();
    let msgpack_z_total: usize = fixtures.iter().map(|fixture| fixture.msgpack_z.len()).sum();
    println!("total yaml size: {yaml_total}");
    println!("total json size: {json_total}");
    println!("total msgpack size: {msgpack_total}");
    println!("total rkyv size: {rkyv_total}");
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
            let package_value = packages
                .iter()
                .find(|package| canonical_package_id(package).as_deref() == Some(id))
                .unwrap_or_else(|| panic!("missing fixture package {id}"));
            package_fixture((*id).to_string(), package_value)
        })
        .collect()
}

fn package_fixture(id: String, package: &Value) -> PackageFixture {
    let rkyv_package: RkyvAquaPackage = serde_yaml::from_value(package.clone()).unwrap();
    let package: AquaPackage = serde_yaml::from_value(package.clone()).unwrap();
    let yaml = serde_yaml::to_string(&package).unwrap();
    let json = serde_json::to_vec(&package).unwrap();
    let msgpack = rmp_serde::to_vec_named(&package).unwrap();
    let rkyv = rkyv::to_bytes::<RkyvError>(&rkyv_package).unwrap().to_vec();
    let msgpack_z = encode_msgpack_z(&msgpack);
    PackageFixture {
        id,
        package,
        rkyv_package,
        yaml,
        json,
        msgpack,
        rkyv,
        msgpack_z,
    }
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

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
#[rkyv(serialize_bounds(
    __S: rkyv::ser::Writer + rkyv::ser::Allocator,
    __S::Error: rkyv::rancor::Source,
))]
#[rkyv(deserialize_bounds(__D::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(
    bounds(
        __C: rkyv::validation::ArchiveContext,
        __C::Error: rkyv::rancor::Source,
    )
))]
#[serde(default)]
struct RkyvAquaPackage {
    r#type: RkyvAquaPackageType,
    repo_owner: String,
    repo_name: String,
    name: Option<String>,
    asset: String,
    url: String,
    description: Option<String>,
    format: String,
    rosetta2: bool,
    windows_arm_emulation: bool,
    complete_windows_ext: bool,
    supported_envs: Vec<String>,
    files: Vec<RkyvAquaFile>,
    vars: Vec<RkyvAquaVar>,
    replacements: HashMap<String, String>,
    version_prefix: Option<String>,
    version_filter: Option<String>,
    version_source: Option<String>,
    checksum: Option<RkyvAquaChecksum>,
    slsa_provenance: Option<RkyvAquaSlsaProvenance>,
    minisign: Option<RkyvAquaMinisign>,
    github_artifact_attestations: Option<RkyvAquaGithubArtifactAttestations>,
    #[rkyv(omit_bounds)]
    overrides: Vec<RkyvAquaOverride>,
    version_constraint: String,
    #[rkyv(omit_bounds)]
    version_overrides: Vec<RkyvAquaPackage>,
    no_asset: bool,
    error_message: Option<String>,
    path: Option<String>,
}

impl Default for RkyvAquaPackage {
    fn default() -> Self {
        Self {
            r#type: RkyvAquaPackageType::GithubRelease,
            repo_owner: String::new(),
            repo_name: String::new(),
            name: None,
            asset: String::new(),
            url: String::new(),
            description: None,
            format: String::new(),
            rosetta2: false,
            windows_arm_emulation: false,
            complete_windows_ext: true,
            supported_envs: Vec::new(),
            files: Vec::new(),
            vars: Vec::new(),
            replacements: HashMap::new(),
            version_prefix: None,
            version_filter: None,
            version_source: None,
            checksum: None,
            slsa_provenance: None,
            minisign: None,
            github_artifact_attestations: None,
            overrides: Vec::new(),
            version_constraint: String::new(),
            version_overrides: Vec::new(),
            no_asset: false,
            error_message: None,
            path: None,
        }
    }
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
#[serde(rename_all = "snake_case")]
enum RkyvAquaPackageType {
    GithubArchive,
    GithubContent,
    GithubRelease,
    Http,
    GoInstall,
    GoBuild,
    Cargo,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
struct RkyvAquaOverride {
    #[serde(flatten)]
    pkg: RkyvAquaPackage,
    goos: Option<String>,
    goarch: Option<String>,
}

#[derive(
    Debug, Default, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
struct RkyvAquaVar {
    name: String,
    default: Option<String>,
    #[serde(default)]
    required: bool,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
struct RkyvAquaFile {
    name: String,
    src: Option<String>,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
#[serde(rename_all = "lowercase")]
enum RkyvAquaChecksumAlgorithm {
    Sha1,
    Sha256,
    Sha512,
    Md5,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
#[serde(rename_all = "snake_case")]
enum RkyvAquaChecksumType {
    GithubRelease,
    Http,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
#[serde(rename_all = "snake_case")]
enum RkyvAquaMinisignType {
    GithubRelease,
    Http,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
struct RkyvAquaCosignSignature {
    r#type: Option<String>,
    repo_owner: Option<String>,
    repo_name: Option<String>,
    url: Option<String>,
    asset: Option<String>,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
struct RkyvAquaCosign {
    enabled: Option<bool>,
    signature: Option<RkyvAquaCosignSignature>,
    key: Option<RkyvAquaCosignSignature>,
    certificate: Option<RkyvAquaCosignSignature>,
    bundle: Option<RkyvAquaCosignSignature>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    opts: Vec<String>,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
struct RkyvAquaSlsaProvenance {
    enabled: Option<bool>,
    r#type: Option<String>,
    repo_owner: Option<String>,
    repo_name: Option<String>,
    url: Option<String>,
    asset: Option<String>,
    source_uri: Option<String>,
    source_tag: Option<String>,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
struct RkyvAquaMinisign {
    enabled: Option<bool>,
    r#type: Option<RkyvAquaMinisignType>,
    repo_owner: Option<String>,
    repo_name: Option<String>,
    url: Option<String>,
    asset: Option<String>,
    public_key: Option<String>,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
struct RkyvAquaGithubArtifactAttestations {
    enabled: Option<bool>,
    signer_workflow: Option<String>,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
struct RkyvAquaChecksum {
    r#type: Option<RkyvAquaChecksumType>,
    algorithm: Option<RkyvAquaChecksumAlgorithm>,
    pattern: Option<RkyvAquaChecksumPattern>,
    cosign: Option<RkyvAquaCosign>,
    file_format: Option<String>,
    enabled: Option<bool>,
    asset: Option<String>,
    url: Option<String>,
}

#[derive(
    Debug, SerdeDeserialize, SerdeSerialize, Archive, RkyvDeserialize, RkyvSerialize, Clone,
)]
struct RkyvAquaChecksumPattern {
    checksum: String,
    file: Option<String>,
}
