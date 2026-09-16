//! Counts heap allocations for representative release-mode lookup paths.
//! Run with `cargo run --release --example allocation_bench`.

use chinese_dictionary::{
    query_by_id, query_by_simplified, search_english, CompletionMode, EnglishSearchOptions,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

// This delegates every operation to the standard system allocator and only
// adds relaxed diagnostic counters for this benchmark process.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller supplies the allocation contract; it is forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the pointer and layout came from the delegated system allocator.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(size, Ordering::Relaxed);
        // SAFETY: the caller supplies the reallocation contract; it is forwarded unchanged.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn snapshot() -> (usize, usize) {
    (
        ALLOCATIONS.load(Ordering::Relaxed),
        ALLOCATED_BYTES.load(Ordering::Relaxed),
    )
}

fn measure<T>(label: &str, operation: impl FnOnce() -> T) -> T {
    let before = snapshot();
    let value = operation();
    let after = snapshot();
    println!(
        "{label}: {} allocations, {} requested bytes",
        after.0 - before.0,
        after.1 - before.1
    );
    value
}

fn main() {
    let entry = query_by_simplified("西瓜")[0];
    let id = entry.id();
    let id_string = id.to_string();

    measure("parsed ID lookup", || black_box(query_by_id(&id)));
    measure("ID parsing", || {
        black_box(id_string.parse::<chinese_dictionary::LexicalId>().unwrap())
    });

    let committed = EnglishSearchOptions {
        completion: CompletionMode::Disabled,
        ..EnglishSearchOptions::default()
    };
    let result = measure("committed English search", || {
        black_box(search_english("to be happy", committed).unwrap())
    });
    measure("borrowed result JSON", || {
        black_box(serde_json::to_vec(&result).unwrap())
    });
    measure("multi-concept English search", || {
        black_box(search_english("hello my name is", committed).unwrap())
    });
    measure("autocomplete English search", || {
        black_box(search_english("wat", EnglishSearchOptions::default()).unwrap())
    });
}
