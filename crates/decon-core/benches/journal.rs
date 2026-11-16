//! Бенчмарки hot paths S1 (ARCHITECTURE.md §7).
//!
//! `append_events` — критерий приёмки S1 №3: 0 аллокаций, p95 < 1 мкс
//! без учёта I/O SQLite. Подсчёт аллокаций — через подсчитывающий
//! аллокатор в alloc_counter (см. ниже).

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use decon_core::model::event::EventKind;
use decon_core::model::journal::SessionJournal;
use uuid::Uuid;

/// Подсчитывающий аллокатор: global counter + System allocator.
mod alloc_counter {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCATED.fetch_add(1, Ordering::Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }
}

#[global_allocator]
static GLOBAL: alloc_counter::Counting = alloc_counter::Counting;

/// Замер: append события в буфер журнала.
fn bench_append(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal");
    group.throughput(Throughput::Elements(1));

    group.bench_function("append_event", |b| {
        b.iter_batched(
            || SessionJournal::new(Uuid::now_v7()),
            |mut j| {
                for i in 0..1000i64 {
                    j.append(i, EventKind::Signal);
                }
                j
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Подтверждение бюджета 0 аллокаций: после создания журнала 1000
    // appends не добавляют ни одной аллокации.
    group.bench_function("append_1000_allocations_count", |b| {
        b.iter(|| {
            let before = alloc_counter::ALLOCATED.load(std::sync::atomic::Ordering::Relaxed);
            let mut j = SessionJournal::new(Uuid::now_v7());
            let after_new = alloc_counter::ALLOCATED.load(std::sync::atomic::Ordering::Relaxed);
            for i in 0..1000i64 {
                j.append(i, EventKind::Signal);
            }
            let after_appends =
                alloc_counter::ALLOCATED.load(std::sync::atomic::Ordering::Relaxed);
            // new() аллоцирует буфер — это setup, не hot path.
            let setup_allocs = after_new - before;
            let append_allocs = after_appends - after_new;
            assert_eq!(
                append_allocs, 0,
                "бюджет: 0 аллокаций на append (setup new(): {setup_allocs})"
            );
        })
    });

    group.finish();
}

/// Замер: генерация UUID v7 (входит в стоимость события на flush, не на append).
fn bench_uuid_v7(c: &mut Criterion) {
    c.bench_function("uuid_v7_now", |b| b.iter(Uuid::now_v7));
}

criterion_group!(benches, bench_append, bench_uuid_v7);
criterion_main!(benches);
