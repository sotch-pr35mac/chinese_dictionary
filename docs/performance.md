# Performance benchmarking

Performance comparisons must use the same generated corpus, release profile,
query options, completion mode, and result limits. Record the host architecture,
operating system, Rust version, bundle manifest digest, and whether filesystem
pages were already cached.

Run the built-in search workload with:

```sh
cargo run --release --example english_search_bench
```

Count heap activity on representative lookup and serialization paths with:

```sh
cargo run --release --example allocation_bench
```

The workload separates initialization from warm queries and covers exact English,
morphology, final-token completion, a common word, and multi-concept phrases. For
committed search comparisons, disable completion explicitly instead of combining
autocomplete and submitted-query timings.

In addition to search latency, release measurements should report:

- initialization and first-lookup latency;
- repeated and distributed parsed-identity lookup latency;
- string identity parsing separately from lookup;
- search-to-JSON latency;
- process resident memory and allocations on the normal query path;
- raw and compressed artifact sizes; and
- the final executable size.

Use percentile distributions rather than a single run. The release acceptance
targets are a warm parsed-ID lookup below 1 microsecond p95 and 2 milliseconds p95
for the historically slow English cases. These are targets to measure and report,
not API guarantees.

The schema-4 measurements and the explicitly missed complex-query target are in
[`performance-results-2026-09-16.md`](performance-results-2026-09-16.md).
