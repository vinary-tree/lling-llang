//! Native PDA ABI ownership, paging, validation, and weighted path parity.
#![cfg(feature = "ffi")]

use lling_llang::ffi::*;
use std::ptr;
use vinary_tree_interop::{VtUnitDomain, VtWeightDomain};

#[test]
fn count_diamond_is_exact_across_resource_and_session_boundaries() {
    let mut builder = ptr::null_mut();
    assert_eq!(
        lling_pda_builder_open(
            VtUnitDomain::UnicodeScalar as u32,
            VtWeightDomain::CountF64 as u32,
            0,
            &mut builder,
        ),
        LlingLlangStatus::Ok
    );
    let mut states = [0; 4];
    for state in &mut states {
        assert_eq!(
            lling_pda_builder_add_state(builder, state),
            LlingLlangStatus::Ok
        );
    }
    assert_eq!(
        lling_pda_builder_set_start(builder, states[0]),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_pda_builder_set_final(builder, states[3], 1.0),
        LlingLlangStatus::Ok
    );
    for middle in [states[1], states[2]] {
        for (from, to) in [(states[0], middle), (middle, states[3])] {
            assert_eq!(
                unsafe {
                    lling_pda_builder_add_transition(
                        builder,
                        from,
                        0,
                        0,
                        0,
                        to,
                        LLING_PDA_NOOP,
                        ptr::null(),
                        0,
                        1.0,
                    )
                },
                LlingLlangStatus::Ok
            );
        }
    }
    assert_eq!(
        unsafe {
            lling_pda_builder_add_transition(
                builder,
                states[3],
                'λ' as u64,
                1,
                0,
                states[3],
                LLING_PDA_NOOP,
                ptr::null(),
                0,
                3.0,
            )
        },
        LlingLlangStatus::Ok
    );
    assert_eq!(
        unsafe {
            lling_pda_builder_add_transition(
                builder,
                0,
                0xd800,
                1,
                0,
                states[3],
                LLING_PDA_NOOP,
                ptr::null(),
                0,
                1.0,
            )
        },
        LlingLlangStatus::InvalidArgument
    );
    let bottom = 0_u32;
    assert_eq!(
        unsafe {
            lling_pda_builder_add_transition(
                builder,
                states[0],
                0,
                0,
                0,
                states[3],
                LLING_PDA_PUSH,
                &bottom,
                usize::MAX,
                1.0,
            )
        },
        LlingLlangStatus::LimitExceeded
    );

    let mut pda = ptr::null_mut();
    assert_eq!(
        lling_pda_builder_build(builder, &mut pda),
        LlingLlangStatus::Ok
    );
    unsafe { lling_pda_builder_free(builder) };
    let mut unit = 0;
    let mut weight_domain = 0;
    assert_eq!(
        unsafe { lling_pda_domains(pda, &mut unit, &mut weight_domain) },
        LlingLlangStatus::Ok
    );
    assert_eq!(unit, VtUnitDomain::UnicodeScalar as u32);
    assert_eq!(weight_domain, VtWeightDomain::CountF64 as u32);

    let mut session = ptr::null_mut();
    assert_eq!(
        unsafe { lling_pda_session_open(pda, 8, &mut session) },
        LlingLlangStatus::Ok
    );
    unsafe { lling_pda_free(pda) };

    let mut accepted = 0;
    let mut weight = 0.0;
    assert_eq!(
        unsafe { lling_pda_session_acceptance(session, 100, &mut accepted, &mut weight) },
        LlingLlangStatus::Ok
    );
    assert_eq!((accepted, weight), (1, 2.0));
    let mut count = 0;
    assert_eq!(
        lling_pda_session_frontier_open(session, 100, &mut count),
        LlingLlangStatus::Ok
    );
    assert_eq!(count, 1);
    let mut choice = LlingPdaChoice {
        label: 0,
        weight: 0.0,
    };
    let mut written = 0;
    assert_eq!(
        unsafe { lling_pda_session_frontier_next(session, &mut choice, 1, &mut written) },
        LlingLlangStatus::Ok
    );
    assert_eq!((written, choice.label, choice.weight), (1, 'λ' as u64, 6.0));
    assert_eq!(
        unsafe { lling_pda_session_frontier_next(session, &mut choice, 1, &mut written) },
        LlingLlangStatus::Ok
    );
    assert_eq!(written, 0);
    assert_eq!(
        lling_pda_session_frontier_open(session, 1, &mut count),
        LlingLlangStatus::LimitExceeded
    );
    assert_eq!(count, 0);
    let mut advanced = 0;
    assert_eq!(
        lling_pda_session_advance(session, 'λ' as u64, 1, &mut advanced),
        LlingLlangStatus::LimitExceeded
    );
    assert_eq!(advanced, 0);
    assert_eq!(
        unsafe { lling_pda_session_acceptance(session, 100, &mut accepted, &mut weight) },
        LlingLlangStatus::Ok
    );
    assert_eq!((accepted, weight), (1, 2.0));
    unsafe { lling_pda_session_free(session) };
}

#[test]
fn count_weight_that_exceeds_exact_wire_range_is_rejected() {
    let mut builder = ptr::null_mut();
    assert_eq!(
        lling_pda_builder_open(
            VtUnitDomain::Byte as u32,
            VtWeightDomain::CountF64 as u32,
            0,
            &mut builder,
        ),
        LlingLlangStatus::Ok
    );
    let mut previous = 0;
    assert_eq!(
        lling_pda_builder_add_state(builder, &mut previous),
        LlingLlangStatus::Ok
    );
    assert_eq!(
        lling_pda_builder_set_start(builder, previous),
        LlingLlangStatus::Ok
    );
    for _ in 0..54 {
        let mut next = 0;
        assert_eq!(
            lling_pda_builder_add_state(builder, &mut next),
            LlingLlangStatus::Ok
        );
        for _ in 0..2 {
            assert_eq!(
                unsafe {
                    lling_pda_builder_add_transition(
                        builder,
                        previous,
                        0,
                        0,
                        0,
                        next,
                        LLING_PDA_NOOP,
                        ptr::null(),
                        0,
                        1.0,
                    )
                },
                LlingLlangStatus::Ok
            );
        }
        previous = next;
    }
    assert_eq!(
        lling_pda_builder_set_final(builder, previous, 1.0),
        LlingLlangStatus::Ok
    );
    let mut pda = ptr::null_mut();
    assert_eq!(
        lling_pda_builder_build(builder, &mut pda),
        LlingLlangStatus::Ok
    );
    unsafe { lling_pda_builder_free(builder) };
    let mut session = ptr::null_mut();
    assert_eq!(
        unsafe { lling_pda_session_open(pda, 8, &mut session) },
        LlingLlangStatus::Ok
    );
    let mut accepted = 0;
    let mut weight = 0.0;
    assert_eq!(
        unsafe { lling_pda_session_acceptance(session, 1000, &mut accepted, &mut weight) },
        LlingLlangStatus::LimitExceeded
    );
    assert_eq!((accepted, weight), (0, 0.0));
    unsafe {
        lling_pda_session_free(session);
        lling_pda_free(pda);
    }
}
