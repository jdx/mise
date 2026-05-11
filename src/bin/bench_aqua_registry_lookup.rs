#[path = "../aqua/standard_registry.rs"]
mod standard_registry;

use std::collections::HashMap;
use std::hint::black_box;
use std::mem::size_of;
use std::time::{Duration, Instant};

const SAMPLES: usize = 7;

fn main() {
    let rounds = std::env::var("MISE_BENCH_ROUNDS")
        .ok()
        .and_then(|rounds| rounds.parse().ok())
        .unwrap_or(2_000);
    let mut keys = standard_registry::AQUA_STANDARD_REGISTRY_FILES
        .keys()
        .copied()
        .collect::<Vec<_>>();
    keys.sort_unstable();
    let entries = standard_registry::AQUA_STANDARD_REGISTRY_FILES
        .entries()
        .map(|(key, value)| (*key, *value))
        .collect::<Vec<_>>();
    let hashmap = build_hashmap(&entries);
    let lookups = keys.len() * rounds;
    let estimated_hashmap_heap =
        hashmap.capacity() * (size_of::<(&'static str, &'static [u8])>() + 1);

    println!("entries: {}", entries.len());
    println!("rounds: {rounds}");
    println!("lookups/sample: {lookups}");
    println!("hashmap capacity: {}", hashmap.capacity());
    println!(
        "estimated runtime HashMap heap: >= {:.1} KiB",
        estimated_hashmap_heap as f64 / 1024.0
    );
    println!();
    println!("| lookup | ns/lookup | total ms | checksum |");
    println!("| --- | ---: | ---: | ---: |");

    let phf = median_lookup(|| phf_lookup(&keys, rounds));
    print_lookup_row("phf", phf, lookups);

    let hashmap_lookup = median_lookup(|| hashmap_lookup(&hashmap, &keys, rounds));
    print_lookup_row("HashMap warmed", hashmap_lookup, lookups);

    println!();
    println!("| first-use path | total ms | checksum |");
    println!("| --- | ---: | ---: |");

    let first_key = keys[0];
    let hashmap_first_use = median_total(|| hashmap_build_plus_first_lookup(&entries, first_key));
    print_total_row("HashMap build + first lookup", hashmap_first_use);

    let phf_first_lookup = median_total(|| phf_first_lookup(first_key));
    print_total_row("phf first lookup", phf_first_lookup);
}

fn build_hashmap(
    entries: &[(&'static str, &'static [u8])],
) -> HashMap<&'static str, &'static [u8]> {
    let mut map = HashMap::with_capacity(entries.len());
    for &(key, value) in entries {
        map.insert(key, value);
    }
    map
}

fn phf_lookup(keys: &[&'static str], rounds: usize) -> usize {
    let mut checksum = 0usize;
    for _ in 0..rounds {
        for &key in keys {
            let content = standard_registry::AQUA_STANDARD_REGISTRY_FILES
                .get(black_box(key))
                .unwrap();
            checksum = checksum.wrapping_add(black_box(content.len()));
        }
    }
    black_box(checksum)
}

fn hashmap_lookup(
    map: &HashMap<&'static str, &'static [u8]>,
    keys: &[&'static str],
    rounds: usize,
) -> usize {
    let mut checksum = 0usize;
    for _ in 0..rounds {
        for &key in keys {
            let content = map.get(black_box(key)).unwrap();
            checksum = checksum.wrapping_add(black_box(content.len()));
        }
    }
    black_box(checksum)
}

fn hashmap_build_plus_first_lookup(
    entries: &[(&'static str, &'static [u8])],
    key: &'static str,
) -> usize {
    let map = build_hashmap(entries);
    let content = map.get(black_box(key)).unwrap();
    black_box(content.len() + map.len() + map.capacity())
}

fn phf_first_lookup(key: &'static str) -> usize {
    let content = standard_registry::AQUA_STANDARD_REGISTRY_FILES
        .get(black_box(key))
        .unwrap();
    black_box(content.len())
}

fn median_lookup(mut f: impl FnMut() -> usize) -> (Duration, usize) {
    let mut samples = (0..SAMPLES)
        .map(|_| {
            let start = Instant::now();
            let checksum = f();
            (start.elapsed(), checksum)
        })
        .collect::<Vec<_>>();
    samples.sort_unstable_by_key(|(duration, _)| *duration);
    samples[SAMPLES / 2]
}

fn median_total(mut f: impl FnMut() -> usize) -> (Duration, usize) {
    median_lookup(&mut f)
}

fn print_lookup_row(name: &str, (duration, checksum): (Duration, usize), lookups: usize) {
    let total_ns = duration.as_nanos() as f64;
    let ns_per_lookup = total_ns / lookups as f64;
    println!(
        "| {name} | {ns_per_lookup:.2} | {:.2} | {checksum} |",
        duration.as_secs_f64() * 1_000.0
    );
}

fn print_total_row(name: &str, (duration, checksum): (Duration, usize)) {
    println!(
        "| {name} | {:.6} | {checksum} |",
        duration.as_secs_f64() * 1_000.0
    );
}
