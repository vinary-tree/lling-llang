#![cfg(feature = "ffi")]

use lling_llang::ffi::{
    lling_tree_free, lling_tree_new, lling_tree_result_copy, lling_tree_result_info,
    lling_tree_results_free, lling_tree_results_info, lling_tree_rule_info,
    lling_tree_rule_pattern_copy, lling_tree_transduce, LlingAbiV2Header, LlingBudgetV2,
    LlingLlangStatus, LlingTreeNode, LlingTreeResults, LlingTreeRule, LlingTreeTransducer,
    LLING_ABI_V2, LLING_BUDGET_ARCS, LLING_BUDGET_BYTES, LLING_BUDGET_STATES, LLING_BUDGET_WORK,
};
use std::mem::size_of;
use std::ptr;
use vinary_tree_interop::{VtUnitDomain, VtWeightDomain};

fn budget() -> LlingBudgetV2 {
    LlingBudgetV2 {
        header: LlingAbiV2Header {
            struct_size: size_of::<LlingBudgetV2>() as u32,
            abi_version: LLING_ABI_V2,
            flags: LLING_BUDGET_STATES | LLING_BUDGET_ARCS | LLING_BUDGET_BYTES | LLING_BUDGET_WORK,
            reserved: 0,
        },
        max_states: 16,
        max_arcs: 128,
        max_bytes: 16_384,
        max_work: 1_024,
        reserved: [0; 2],
    }
}

fn symbol(label: char, arity: u64) -> LlingTreeNode {
    LlingTreeNode {
        kind: 0,
        symbol: u64::from(label),
        arity,
        state: 0,
        variable_index: 0,
    }
}

fn rule(rank: u64) -> LlingTreeRule {
    LlingTreeRule {
        state: 0,
        input_symbol: u64::from('a'),
        input_arity: rank,
        pattern_offset: 0,
        pattern_len: 1,
        weight: 0.0,
    }
}

fn capture(
    rules: &[LlingTreeRule],
    patterns: &[LlingTreeNode],
    limits: &LlingBudgetV2,
    out: &mut *mut LlingTreeTransducer,
) -> LlingLlangStatus {
    let flags = [0u8];
    let weights = [0.0f64];
    unsafe {
        lling_tree_new(
            VtUnitDomain::UnicodeScalar as u32,
            VtWeightDomain::TropicalF64 as u32,
            1,
            0,
            flags.as_ptr(),
            weights.as_ptr(),
            rules.as_ptr(),
            rules.len() as u64,
            patterns.as_ptr(),
            patterns.len() as u64,
            limits,
            out,
        )
    }
}

#[test]
fn tree_abi_captures_snapshot_and_bounds_results() {
    assert_eq!(size_of::<LlingTreeNode>(), 40);
    assert_eq!(size_of::<LlingTreeRule>(), 48);
    let limits = budget();
    let mut patterns = [symbol('b', 0)];
    let mut graph = ptr::null_mut();
    assert_eq!(
        capture(&[rule(0)], &patterns, &limits, &mut graph),
        LlingLlangStatus::Ok
    );
    assert!(!graph.is_null());
    patterns[0] = symbol('z', 0);
    assert_eq!(patterns[0], symbol('z', 0));

    let mut descriptor = rule(9);
    assert_eq!(
        unsafe { lling_tree_rule_info(graph, 0, &mut descriptor) },
        LlingLlangStatus::Ok
    );
    assert_eq!(descriptor.input_arity, 0);
    let mut copied = [symbol('x', 0)];
    assert_eq!(
        unsafe { lling_tree_rule_pattern_copy(graph, 0, copied.as_mut_ptr(), 1) },
        LlingLlangStatus::Ok
    );
    assert_eq!(copied[0], symbol('b', 0));

    let input = [symbol('a', 0)];
    let mut results: *mut LlingTreeResults = ptr::null_mut();
    assert_eq!(
        unsafe {
            lling_tree_transduce(
                graph,
                input.as_ptr(),
                1,
                u64::from(u32::MAX),
                &limits,
                &mut results,
            )
        },
        LlingLlangStatus::Ok
    );
    unsafe { lling_tree_free(graph) };
    graph = ptr::null_mut();
    let mut count = 0usize;
    assert_eq!(
        unsafe { lling_tree_results_info(results, &mut count) },
        LlingLlangStatus::Ok
    );
    assert_eq!(count, 1);
    let mut nodes = 0usize;
    let mut weight = f64::NAN;
    assert_eq!(
        unsafe { lling_tree_result_info(results, 0, &mut nodes, &mut weight) },
        LlingLlangStatus::Ok
    );
    assert_eq!((nodes, weight), (1, 0.0));
    let mut output = [symbol('x', 0)];
    assert_eq!(
        unsafe { lling_tree_result_copy(results, 0, output.as_mut_ptr(), 0) },
        LlingLlangStatus::LimitExceeded
    );
    assert_eq!(output[0], symbol('x', 0));
    assert_eq!(
        unsafe { lling_tree_result_copy(results, 0, output.as_mut_ptr(), 1) },
        LlingLlangStatus::Ok
    );
    assert_eq!(output[0], symbol('b', 0));
    unsafe { lling_tree_results_free(results) };

    let sentinel = ptr::dangling_mut::<LlingTreeTransducer>();
    let mut unchanged = sentinel;
    let malformed = [LlingTreeNode {
        kind: 1,
        symbol: 0,
        arity: 0,
        state: 0,
        variable_index: 0,
    }];
    assert_eq!(
        capture(&[rule(0)], &malformed, &limits, &mut unchanged),
        LlingLlangStatus::InvalidArgument
    );
    assert_eq!(unchanged, sentinel);
    let mut second = rule(1);
    second.pattern_offset = 1;
    let two_patterns = [symbol('b', 0), symbol('c', 0)];
    assert_eq!(
        capture(&[rule(0), second], &two_patterns, &limits, &mut unchanged),
        LlingLlangStatus::InvalidArgument
    );
    assert_eq!(unchanged, sentinel);
    let mut short = limits;
    short.max_arcs = 1;
    assert_eq!(
        capture(&[rule(0)], &[symbol('b', 0)], &short, &mut unchanged),
        LlingLlangStatus::LimitExceeded
    );
    assert_eq!(unchanged, sentinel);
    assert!(graph.is_null());
}
