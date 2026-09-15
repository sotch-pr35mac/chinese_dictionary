use chinese_dictionary::{init, search_english, EnglishSearchOptions};
use std::hint::black_box;
use std::time::{Duration, Instant};

const QUERIES: &[&str] = &[
    "watermelon",
    "run",
    "to be happy",
    "the",
    "wat",
    "hello my name is",
];

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    samples[(samples.len() - 1) * percentile / 100]
}

fn main() {
    let started = Instant::now();
    init();
    println!("initialization: {:?}", started.elapsed());

    for query in QUERIES {
        for _ in 0..10 {
            black_box(search_english(query, EnglishSearchOptions::default()).unwrap());
        }
        let mut samples = Vec::with_capacity(100);
        for _ in 0..100 {
            let started = Instant::now();
            black_box(search_english(query, EnglishSearchOptions::default()).unwrap());
            samples.push(started.elapsed());
        }
        samples.sort_unstable();
        println!(
            "{query:?}: p50={:?} p95={:?} p99={:?}",
            percentile(&samples, 50),
            percentile(&samples, 95),
            percentile(&samples, 99),
        );
    }
}
