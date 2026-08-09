//! FFI composition benchmarks (wave W8 / T7).
//!
//! Evidence base for the ABI composition path: these measure that
//! `lling_wfst_compose` construction is O(1) in the inputs (it captures two
//! snapshots and returns; no product-state expansion happens until the
//! composed resource is traversed), versus the cost of building and exporting
//! the operand WFSTs. All WFSTs are built through the public C ABI so the
//! numbers reflect the boundary a foreign caller crosses. Hardware for recorded
//! results: see /home/dylon/.claude/hardware-specifications.md.
//!
//! Run: `cargo bench --features ffi --bench ffi_compose_benchmarks`
//! (taskset + performance governor via a bench wrapper for stable numbers).

use std::hint::black_box;
use std::ptr;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use lling_llang::ffi::{
    lling_resource_release, lling_wfst_builder_add_arc, lling_wfst_builder_add_state,
    lling_wfst_builder_build, lling_wfst_builder_new, lling_wfst_builder_set_final,
    lling_wfst_builder_set_start, lling_wfst_compose, lling_wfst_free, lling_wfst_resource,
    LlingStatus, LlingWfst,
};
use vinary_tree_interop::VtResource;

/// Build a linear chain WFST of `len` states over ascending scalar labels via
/// the C ABI, returning the owned handle.
fn build_chain(len: u32) -> *mut LlingWfst {
    let mut builder = ptr::null_mut();
    assert_eq!(lling_wfst_builder_new(&mut builder), LlingStatus::Ok);
    let mut prev = 0u32;
    assert_eq!(
        lling_wfst_builder_add_state(builder, &mut prev),
        LlingStatus::Ok
    );
    assert_eq!(lling_wfst_builder_set_start(builder, prev), LlingStatus::Ok);
    for index in 0..len {
        let mut next = 0u32;
        assert_eq!(
            lling_wfst_builder_add_state(builder, &mut next),
            LlingStatus::Ok
        );
        let label = u64::from('a') + u64::from(index % 26);
        assert_eq!(
            lling_wfst_builder_add_arc(builder, prev, label, 1, label, 1, next, 1.0),
            LlingStatus::Ok
        );
        prev = next;
    }
    assert_eq!(
        lling_wfst_builder_set_final(builder, prev, 0.0),
        LlingStatus::Ok
    );
    let mut wfst = ptr::null_mut();
    assert_eq!(
        lling_wfst_builder_build(builder, &mut wfst),
        LlingStatus::Ok
    );
    wfst
}

fn resource_of(wfst: *mut LlingWfst) -> VtResource {
    let mut resource = VtResource::NULL;
    assert_eq!(
        unsafe { lling_wfst_resource(wfst, &mut resource) },
        LlingStatus::Ok
    );
    resource
}

/// Composition construction should be flat in the operand sizes: it captures
/// two snapshots and returns without expanding the product.
fn bench_compose_construction(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("compose_construction");
    for &len in &[4u32, 16, 64, 256] {
        let left = build_chain(len);
        let right = build_chain(len);
        let left_res = resource_of(left);
        let right_res = resource_of(right);
        group.bench_with_input(BenchmarkId::from_parameter(len), &len, |b, _| {
            b.iter(|| {
                let mut composed = ptr::null_mut();
                let status = lling_wfst_compose(
                    black_box(left_res),
                    black_box(right_res),
                    &mut composed,
                );
                assert_eq!(status, LlingStatus::Ok);
                unsafe { lling_wfst_free(composed) };
            });
        });
        lling_resource_release(left_res);
        lling_resource_release(right_res);
        unsafe { lling_wfst_free(left) };
        unsafe { lling_wfst_free(right) };
    }
    group.finish();
}

/// Baseline: the cost of building and exporting a chain of a given size, so the
/// flat composition curve can be read against a growing one.
fn bench_build_and_export(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("build_and_export");
    for &len in &[4u32, 16, 64, 256] {
        group.bench_with_input(BenchmarkId::from_parameter(len), &len, |b, &len| {
            b.iter(|| {
                let wfst = build_chain(black_box(len));
                let _resource = resource_of(wfst);
                let mut resource = VtResource::NULL;
                let _ = unsafe { lling_wfst_resource(wfst, &mut resource) };
                lling_resource_release(resource);
                unsafe { lling_wfst_free(wfst) };
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_compose_construction, bench_build_and_export);
criterion_main!(benches);
