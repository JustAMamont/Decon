//! Бенчмарки hot paths тренажёров (S3, ARCHITECTURE.md §7).
//!
//! Критерий приёмки S3 №1: тик MOT — 0 аллокаций при N ≤ 32 объектов,
//! p95 тика < 100 мкс (запас к 60 Гц: бюджет кадра 16.7 мс).
//! Подсчёт аллокаций — считающий аллокатор (как в journal.rs).

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use decon_trainers::mot::{Mot, MotConfig};

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

/// Конфиг на максимальную нагрузку: 10 целей + 22 дистрактора = 32.
fn cfg_max() -> MotConfig {
    MotConfig {
        targets: 10,
        distractors: 22,
        speed: 1.0,
        motion_ms: 8000,
    }
}

/// Прогон в фазу Motion (тики тикают только там по-настоящему).
fn mot_in_motion(seed: u64) -> Mot {
    let mut m = Mot::new(cfg_max(), seed).expect("cfg valid");
    m.tick(1500); // Cue -> Motion
    m
}

fn bench_mot_tick(c: &mut Criterion) {
    let mut group = c.benchmark_group("mot");
    group.throughput(Throughput::Elements(1));

    // Один тик 16 мс (60 Гц) в разгар движения.
    group.bench_function("tick_16ms", |b| {
        b.iter_batched(
            || mot_in_motion(42),
            |mut m| {
                m.tick(16);
            },
            criterion::BatchSize::SmallInput,
        )
    });

    // Бюджет (критерий 1): 1000 тиков — 0 аллокаций.
    group.bench_function("tick_1000_allocations_count", |b| {
        b.iter(|| {
            let mut m = mot_in_motion(42);
            let before = alloc_counter::ALLOCATED.load(std::sync::atomic::Ordering::Relaxed);
            for _ in 0..1000 {
                m.tick(16);
            }
            let after = alloc_counter::ALLOCATED.load(std::sync::atomic::Ordering::Relaxed);
            assert_eq!(
                after - before,
                0,
                "бюджет S3: 0 аллокаций на 1000 тиков MOT"
            );
        })
    });

    group.finish();
}

criterion_group!(benches, bench_mot_tick);
criterion_main!(benches);
