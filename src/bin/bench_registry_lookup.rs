#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hint::black_box;
use std::time::{Duration, Instant};

static REGISTRY: Registry = include!(concat!(env!("OUT_DIR"), "/registry.rs"));

struct Registry {
    entries: &'static [(&'static str, RegistryTool)],
    lookup: phf::Map<&'static str, usize>,
}

impl Registry {
    fn get(&self, name: &str) -> Option<&'static RegistryTool> {
        self.lookup.get(name).map(|index| &self.entries[*index].1)
    }

    fn iter(&self) -> impl Iterator<Item = (&'static str, &'static RegistryTool)> {
        self.entries.iter().map(|(name, tool)| (*name, tool))
    }

    fn keys(&self) -> impl Iterator<Item = &'static str> {
        self.entries.iter().map(|(name, _)| *name)
    }
}

#[derive(Debug, Clone)]
struct RegistryTool {
    short: &'static str,
    description: Option<&'static str>,
    backends: &'static [RegistryBackend],
    aliases: &'static [&'static str],
    overrides: &'static [&'static str],
    test: &'static Option<RegistryToolTest>,
    os: &'static [&'static str],
    idiomatic_files: &'static [&'static str],
    detect: &'static [&'static str],
}

#[derive(Debug, Clone)]
struct RegistryToolTest {
    cmd: &'static str,
    expected: &'static str,
    tools: &'static [&'static str],
}

#[derive(Debug, Clone)]
struct RegistryBackend {
    full: &'static str,
    platforms: &'static [&'static str],
    options: &'static [(&'static str, &'static str)],
}

struct LookupMeasurement {
    name: &'static str,
    lookups: usize,
    elapsed: Duration,
    checksum: usize,
}

impl LookupMeasurement {
    fn ns_per_lookup(&self) -> f64 {
        self.elapsed.as_nanos() as f64 / self.lookups as f64
    }
}

struct BuildMeasurement {
    name: &'static str,
    elapsed: Duration,
    checksum: usize,
}

fn main() {
    let rounds = 2_000;
    let lookup_samples = 9;
    let build_samples = 101;
    let registry_keys = REGISTRY.keys().collect::<Vec<_>>();
    let registry_unique = REGISTRY
        .iter()
        .map(|(_, tool)| tool.short)
        .collect::<HashSet<_>>()
        .len();
    let registry_btree = build_btree();
    let registry_hash = build_hash();

    println!(
        "mise registry: {} lookup keys, {} canonical tools",
        registry_keys.len(),
        registry_unique
    );
    println!("lookup rounds per sample: {rounds}");
    println!("lookup samples: {lookup_samples}");
    println!("build samples: {build_samples}");
    println!();
    println!("## Construction");
    println!("| structure | median us | checksum |");
    println!("| --- | ---: | ---: |");
    for measurement in [
        bench_build("BTreeMap", build_samples, || build_btree().len()),
        bench_build("HashMap", build_samples, || build_hash().len()),
    ] {
        println!(
            "| {} | {:.2} | {} |",
            measurement.name,
            measurement.elapsed.as_secs_f64() * 1_000_000.0,
            measurement.checksum
        );
    }

    println!();
    println!("## Lookup");
    println!("| lookup | ns/lookup | total ms | checksum |");
    println!("| --- | ---: | ---: | ---: |");
    for measurement in [
        bench_lookup("phf", &registry_keys, rounds, lookup_samples, |key| {
            REGISTRY.get(key).map_or(0, |tool| tool.short.len())
        }),
        bench_lookup("BTreeMap", &registry_keys, rounds, lookup_samples, |key| {
            registry_btree.get(key).map_or(0, |tool| tool.short.len())
        }),
        bench_lookup("HashMap", &registry_keys, rounds, lookup_samples, |key| {
            registry_hash.get(key).map_or(0, |tool| tool.short.len())
        }),
    ] {
        println!(
            "| {} | {:.2} | {:.2} | {} |",
            measurement.name,
            measurement.ns_per_lookup(),
            measurement.elapsed.as_secs_f64() * 1_000.0,
            measurement.checksum
        );
    }
}

fn build_btree() -> BTreeMap<&'static str, RegistryTool> {
    REGISTRY
        .iter()
        .map(|(name, tool)| (name, tool.clone()))
        .collect()
}

fn build_hash() -> HashMap<&'static str, RegistryTool> {
    REGISTRY
        .iter()
        .map(|(name, tool)| (name, tool.clone()))
        .collect()
}

fn bench_build(
    name: &'static str,
    samples: usize,
    mut build: impl FnMut() -> usize,
) -> BuildMeasurement {
    let mut results = Vec::with_capacity(samples);

    for _ in 0..samples {
        let start = Instant::now();
        let checksum = black_box(build());
        results.push((start.elapsed(), checksum));
    }

    results.sort_by_key(|(elapsed, _)| *elapsed);
    let (elapsed, checksum) = results[samples / 2];

    BuildMeasurement {
        name,
        elapsed,
        checksum,
    }
}

fn bench_lookup(
    name: &'static str,
    keys: &[&str],
    rounds: usize,
    samples: usize,
    mut get: impl FnMut(&str) -> usize,
) -> LookupMeasurement {
    let lookups = keys.len() * rounds;
    let mut results = Vec::with_capacity(samples);

    for _ in 0..samples {
        let start = Instant::now();
        let mut checksum = 0usize;
        for _ in 0..rounds {
            for key in keys {
                checksum = checksum.wrapping_add(black_box(get(black_box(key))));
            }
        }
        results.push((start.elapsed(), checksum));
    }

    results.sort_by_key(|(elapsed, _)| *elapsed);
    let (elapsed, checksum) = results[samples / 2];

    LookupMeasurement {
        name,
        lookups,
        elapsed,
        checksum,
    }
}
