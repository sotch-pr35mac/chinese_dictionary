//! Release-mode benchmark for dictionary startup, English search-to-JSON, and
//! stable-identity lookup. Run it with `cargo run --release --example
//! english_search_bench`; compare runs only when corpus, options, and limits
//! are identical.

use chinese_dictionary::{
    init, query_by_id, query_by_id_str, query_by_simplified, search_english, CompletionMode,
    EnglishSearchOptions, LexicalId,
};
use std::hint::black_box;
use std::time::{Duration, Instant};

const COMMITTED_QUERIES: &[&str] = &[
    "watermelon",
    "run",
    "to be happy",
    "the",
    "hello my name is",
];
const AUTOCOMPLETE_QUERIES: &[&str] = &["wat", "waterm", "hel"];

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    samples[(samples.len() - 1) * percentile / 100]
}

fn benchmark(label: &str, queries: &[&str], options: EnglishSearchOptions) {
    println!("{label}");
    for query in queries {
        for _ in 0..10 {
            let result = search_english(query, options).unwrap();
            black_box(serde_json::to_vec(&result).unwrap());
        }
        let mut search_samples = Vec::with_capacity(100);
        let mut json_samples = Vec::with_capacity(100);
        for _ in 0..100 {
            let started = Instant::now();
            let result = search_english(query, options).unwrap();
            search_samples.push(started.elapsed());
            black_box(serde_json::to_vec(&result).unwrap());
            json_samples.push(started.elapsed());
        }
        search_samples.sort_unstable();
        json_samples.sort_unstable();
        println!(
            "{query:?}: search p95={:?}; search+JSON p50={:?} p95={:?} p99={:?}",
            percentile(&search_samples, 95),
            percentile(&json_samples, 50),
            percentile(&json_samples, 95),
            percentile(&json_samples, 99),
        );
    }
}

fn benchmark_identities() {
    let ids = ["西瓜", "电脑", "你好", "长"]
        .into_iter()
        .flat_map(query_by_simplified)
        .map(|entry| entry.id())
        .collect::<Vec<_>>();
    assert!(!ids.is_empty());
    let encoded = ids.iter().map(ToString::to_string).collect::<Vec<_>>();

    let mut repeated_lookup = Vec::with_capacity(10_000);
    let mut distributed_lookup = Vec::with_capacity(10_000);
    let mut parsing = Vec::with_capacity(10_000);
    let mut string_lookup = Vec::with_capacity(10_000);
    for index in 0..10_000 {
        let started = Instant::now();
        black_box(query_by_id(&ids[0]));
        repeated_lookup.push(started.elapsed());

        let selected = index % ids.len();
        let started = Instant::now();
        black_box(query_by_id(&ids[selected]));
        distributed_lookup.push(started.elapsed());

        let started = Instant::now();
        black_box(encoded[selected].parse::<LexicalId>().unwrap());
        parsing.push(started.elapsed());

        let started = Instant::now();
        black_box(query_by_id_str(&encoded[selected]));
        string_lookup.push(started.elapsed());
    }
    for samples in [
        &mut repeated_lookup,
        &mut distributed_lookup,
        &mut parsing,
        &mut string_lookup,
    ] {
        samples.sort_unstable();
    }
    println!("identity lookup");
    println!(
        "parsed repeated p95={:?}; parsed distributed p95={:?}; parse-only p95={:?}; string lookup p95={:?}",
        percentile(&repeated_lookup, 95),
        percentile(&distributed_lookup, 95),
        percentile(&parsing, 95),
        percentile(&string_lookup, 95),
    );
}

fn main() {
    let started = Instant::now();
    init();
    println!("initialization: {:?}", started.elapsed());

    benchmark(
        "committed search (completion disabled, including JSON)",
        COMMITTED_QUERIES,
        EnglishSearchOptions {
            completion: CompletionMode::Disabled,
            ..EnglishSearchOptions::default()
        },
    );
    benchmark(
        "autocomplete (final-token completion, including JSON)",
        AUTOCOMPLETE_QUERIES,
        EnglishSearchOptions::default(),
    );
    benchmark_identities();
}
