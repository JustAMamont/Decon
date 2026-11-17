//! Бенчмарки hot paths аналитики (S4, ARCHITECTURE.md §7).
//!
//! Критерий приёмки S4 №2: холодный проход корреляций по 10k сессий
//! < 50 мс, без промежуточных аллокаций (подсчёт аллокатором).
//! «Холодный» — первый вызов на свежем RankBuf (setup — не в счёте).

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use decon_core::analysis::correlation::{CorrKind, RankBuf, correlation_missing};
use decon_core::analysis::n_of_1::{DEFAULT_PERMUTATIONS, compare_periods};
use decon_core::analysis::trends::{sma, split_by_version};
use decon_core::db::repo::scale_repo::SeriesPoint;

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

/// Синтетический ряд 10k сессий: LCG-шум с сидом (детерминированность
/// бенча — сравнение между запусками честное).
fn lcg_series(n: usize, seed: u64, scale: f64) -> Vec<Option<f64>> {
    let mut state = seed.max(1);
    (0..n)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            Some((state >> 33) as f64 / (u64::MAX >> 33) as f64 * scale)
        })
        .collect()
}

fn bench_correlations(c: &mut Criterion) {
    let mut group = c.benchmark_group("analysis");
    const N: usize = 10_000;
    group.throughput(Throughput::Elements(N as u64));

    // Холодный проход: Спирмен по 10k пар (условия ↔ качество).
    group.bench_function("spearman_10k", |b| {
        b.iter_batched(
            || {
                (
                    lcg_series(N, 42, 10.0),
                    lcg_series(N, 43, 10.0),
                    RankBuf::new(),
                )
            },
            |(x, y, mut buf)| correlation_missing(&x, &y, &mut buf, CorrKind::Spearman),
            criterion::BatchSize::SmallInput,
        )
    });

    // Пирсон — то же для непрерывных (сон).
    group.bench_function("pearson_10k", |b| {
        b.iter_batched(
            || {
                (
                    lcg_series(N, 44, 8.0),
                    lcg_series(N, 45, 10.0),
                    RankBuf::new(),
                )
            },
            |(x, y, mut buf)| correlation_missing(&x, &y, &mut buf, CorrKind::Pearson),
            criterion::BatchSize::SmallInput,
        )
    });

    // Бюджет аллокаций: горячий проход (буфер уже прогрет) — 0 аллокаций.
    group.bench_function("spearman_10k_hot_allocations", |b| {
        b.iter(|| {
            let x = lcg_series(N, 46, 10.0);
            let y = lcg_series(N, 47, 10.0);
            let mut buf = RankBuf::new();
            correlation_missing(&x, &y, &mut buf, CorrKind::Spearman); // прогрев
            let before = alloc_counter::ALLOCATED.load(std::sync::atomic::Ordering::Relaxed);
            for _ in 0..100 {
                correlation_missing(&x, &y, &mut buf, CorrKind::Spearman);
            }
            let after = alloc_counter::ALLOCATED.load(std::sync::atomic::Ordering::Relaxed);
            assert_eq!(
                after - before,
                0,
                "бюджет S4: 0 аллокаций на 100 горячих корреляций 10k"
            );
        })
    });

    // Тренд: SMA по 10k точек.
    group.bench_function("sma_10k", |b| {
        b.iter_batched(
            || {
                let pts: Vec<SeriesPoint> = (0..N)
                    .map(|i| SeriesPoint {
                        answered_at: String::new(),
                        value: (i as f64 * 0.37) % 10.0,
                        delay_minutes: None,
                        version: 1,
                    })
                    .collect();
                (pts, Vec::new())
            },
            |(pts, mut out)| sma(&pts, 30, &mut out),
            criterion::BatchSize::SmallInput,
        )
    });

    // Сегментация по версиям (аллоцирует результат — это не hot path,
    // но измеряем для полноты картины трендов).
    group.bench_function("split_versions_10k", |b| {
        b.iter_batched(
            || {
                (0..N)
                    .map(|i| SeriesPoint {
                        answered_at: String::new(),
                        value: (i as f64) % 10.0,
                        delay_minutes: None,
                        version: 1 + (i / 2500) as i64,
                    })
                    .collect::<Vec<_>>()
            },
            |pts| split_by_version(&pts),
            criterion::BatchSize::SmallInput,
        )
    });

    // n-of-1: 10k перестановок × пул 100 значений.
    group.bench_function("n_of_1_permutations", |b| {
        b.iter_batched(
            || {
                (
                    (0..50).map(|i| (i % 11) as f64).collect::<Vec<_>>(),
                    (0..50).map(|i| ((i + 3) % 11) as f64).collect::<Vec<_>>(),
                )
            },
            |(a, b)| compare_periods(&a, &b, 42, DEFAULT_PERMUTATIONS),
            criterion::BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(benches, bench_correlations);
criterion_main!(benches);
