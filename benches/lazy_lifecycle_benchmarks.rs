//! Focused benchmarks for the formally specified lazy-WFST lifecycle.

use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use lling_llang::semiring::TropicalWeight;
use lling_llang::wfst::{
    CachePolicy, ExpansionRequest, LazyWfstWrapper, StateExpansion, StateSource,
};
use smallvec::SmallVec;

#[derive(Clone, Copy, Debug)]
struct EmptyStateSource {
    states: usize,
}

impl StateSource<u32, TropicalWeight> for EmptyStateSource {
    fn compute_state(&self, request: ExpansionRequest<'_>) -> StateExpansion<u32, TropicalWeight> {
        assert!((request.state() as usize) < self.states);
        StateExpansion::non_final(SmallVec::new())
    }

    fn start(&self) -> u32 {
        0
    }

    fn num_states_hint(&self) -> Option<usize> {
        Some(self.states)
    }
}

fn populated_lru(states: usize) -> LazyWfstWrapper<EmptyStateSource, u32, TropicalWeight> {
    let mut lazy = LazyWfstWrapper::with_cache_policy(
        EmptyStateSource { states },
        CachePolicy::Lru { max_states: states },
    );
    for state in 0..states {
        lazy.expand(state as u32)
            .expect("benchmark source always completes");
    }
    lazy
}

fn lifecycle_benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("lazy_lifecycle");

    let mut cache_all = LazyWfstWrapper::new(EmptyStateSource { states: 1 });
    cache_all
        .expand(0)
        .expect("benchmark source always completes");
    group.bench_function("cache_all_completed_hit", |bencher| {
        bencher.iter(|| {
            black_box(
                cache_all
                    .expand(black_box(0))
                    .expect("completed hit cannot fail"),
            )
        });
    });

    for states in [64usize, 4096] {
        let mut lazy = populated_lru(states);
        let mut state = 0usize;
        group.bench_with_input(
            BenchmarkId::new("lru_completed_hit", states),
            &states,
            |bencher, &state_count| {
                bencher.iter(|| {
                    state = state.wrapping_add(1) % state_count;
                    black_box(
                        lazy.expand(black_box(state as u32))
                            .expect("completed LRU hit cannot fail"),
                    )
                });
            },
        );
    }

    let mut cold = LazyWfstWrapper::new(EmptyStateSource { states: 1 });
    group.bench_function("clear_and_expand", |bencher| {
        bencher.iter(|| {
            cold.clear_state(0);
            black_box(
                cold.expand(black_box(0))
                    .expect("benchmark source always completes"),
            )
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(30)
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_millis(500));
    targets = lifecycle_benchmarks
}
criterion_main!(benches);
