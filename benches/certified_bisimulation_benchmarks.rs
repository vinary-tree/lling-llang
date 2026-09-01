//! Preregistered certified strong-bisimulation benchmarks.
//!
//! The only comparator is the signature-rescan implementation replaced by the
//! certified Valmari path. It is benchmark-local and never compiled into the
//! production library.

use std::collections::BTreeMap;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use lling_llang::symbolic::bisimulation::{CertifiedBisimulation, Lts};

type Edge = (usize, u32, usize);

fn legacy_signature_rescan(lts: &Lts, colors: &[usize]) -> Vec<usize> {
    let mut adjacency = vec![Vec::<(u32, usize)>::new(); lts.num_states];
    for &(source, action, target) in &lts.transitions {
        adjacency[source].push((action, target));
    }
    let mut blocks = legacy_refine_once(colors, &vec![Vec::new(); lts.num_states]);
    loop {
        let refined = legacy_refine_once(&blocks, &adjacency);
        if block_count(&refined) == block_count(&blocks) {
            return refined;
        }
        blocks = refined;
    }
}

fn legacy_refine_once(blocks: &[usize], adjacency: &[Vec<(u32, usize)>]) -> Vec<usize> {
    let mut ids = BTreeMap::<(usize, Vec<(u32, usize)>), usize>::new();
    let mut refined = Vec::with_capacity(blocks.len());
    for state in 0..blocks.len() {
        let mut signature = adjacency[state]
            .iter()
            .map(|&(action, target)| (action, blocks[target]))
            .collect::<Vec<_>>();
        signature.sort_unstable();
        signature.dedup();
        let key = (blocks[state], signature);
        let fresh = ids.len();
        refined.push(*ids.entry(key).or_insert(fresh));
    }
    refined
}

fn block_count(blocks: &[usize]) -> usize {
    blocks
        .iter()
        .copied()
        .max()
        .map_or(0, |maximum| maximum + 1)
}

fn chain(states: usize) -> (Lts, Vec<usize>) {
    let transitions = (0..states - 1)
        .map(|source| (source, 0, source + 1))
        .collect();
    (Lts::new(states, transitions), vec![0; states])
}

fn wide(states: usize) -> (Lts, Vec<usize>) {
    let transitions = (1..states).map(|target| (0, 7, target)).collect();
    (Lts::new(states, transitions), vec![0; states])
}

fn sparse(states: usize) -> (Lts, Vec<usize>) {
    let transitions = (0..states - 1)
        .step_by(17)
        .map(|source| (source, (source % 5) as u32, source + 1))
        .collect();
    let colors = (0..states).map(|state| state % 3).collect();
    (Lts::new(states, transitions), colors)
}

fn dense(states: usize) -> (Lts, Vec<usize>) {
    let mut transitions = Vec::<Edge>::with_capacity(states * states * 4);
    for source in 0..states {
        for action in 0..4 {
            for target in 0..states {
                transitions.push((source, action, target));
            }
        }
    }
    (Lts::new(states, transitions), vec![0; states])
}

fn paired(
    criterion: &mut Criterion,
    shape: &str,
    sizes: &[usize],
    build: fn(usize) -> (Lts, Vec<usize>),
) {
    let mut group = criterion.benchmark_group(format!("certified_bisimulation/paired/{shape}"));
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for &states in sizes {
        let (lts, colors) = build(states);
        group.throughput(Throughput::Elements(
            (states + lts.transitions.len()) as u64,
        ));
        group.bench_with_input(
            BenchmarkId::new("legacy_signature_rescan", states),
            &states,
            |b, _| {
                b.iter(|| legacy_signature_rescan(&lts, &colors));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("certified_valmari", states),
            &states,
            |b, _| {
                b.iter(|| CertifiedBisimulation::compute(&lts, &colors).unwrap());
            },
        );
    }
    group.finish();
}

fn certified_scale(
    criterion: &mut Criterion,
    shape: &str,
    sizes: &[usize],
    build: fn(usize) -> (Lts, Vec<usize>),
) {
    let mut group = criterion.benchmark_group(format!("certified_bisimulation/scale/{shape}"));
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for &states in sizes {
        let (lts, colors) = build(states);
        group.throughput(Throughput::Elements(
            (states + lts.transitions.len()) as u64,
        ));
        group.bench_with_input(
            BenchmarkId::new("certified_valmari", states),
            &states,
            |b, _| {
                b.iter(|| CertifiedBisimulation::compute(&lts, &colors).unwrap());
            },
        );
    }
    group.finish();
}

fn certified_bisimulation_benchmarks(criterion: &mut Criterion) {
    paired(criterion, "chain", &[32, 64, 128, 256], chain);
    paired(criterion, "wide", &[128, 1_024, 8_192], wide);
    paired(criterion, "sparse", &[1_024, 8_192, 65_536], sparse);
    paired(criterion, "dense", &[16, 32, 64], dense);

    certified_scale(criterion, "chain", &[4_096, 32_768], chain);
    certified_scale(criterion, "wide", &[65_536], wide);
    certified_scale(criterion, "sparse", &[131_072], sparse);
    certified_scale(criterion, "dense", &[128, 256], dense);
}

criterion_group!(benches, certified_bisimulation_benchmarks);
criterion_main!(benches);
