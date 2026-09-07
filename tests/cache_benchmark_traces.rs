//! The timing and allocation controls must replay the same bounded histories.
#[path = "../benches/support/cache_trace.rs"]
mod cache_trace;

#[test]
fn resident_and_capacity_plus_one_traces_have_reproducible_working_sets() {
    for capacity in [1, 2, 64, 1024] {
        for miss in [false, true] {
            let expected: Vec<_> = (0..capacity + usize::from(miss)).collect();
            for name in ["cyclic", "shuffled"] {
                let order = cache_trace::access_order(capacity, miss, name);
                assert_eq!(order, cache_trace::access_order(capacity, miss, name));
                let mut sorted = order.clone();
                sorted.sort_unstable();
                assert_eq!(sorted, expected, "every state appears once per replay");
                if name == "cyclic" {
                    assert_eq!(order, expected);
                } else if capacity >= 64 {
                    assert_ne!(order, expected, "shuffled trace changes physical adjacency");
                }
            }
        }
        let interior = cache_trace::access_order(capacity, false, "interior");
        assert_eq!(interior.len(), 10_000);
        assert!(interior.iter().all(|&index| index < capacity));
        assert_eq!(
            interior,
            cache_trace::access_order(capacity, false, "interior")
        );
        if capacity > 1 {
            assert!(interior.windows(2).any(|pair| pair[0] != pair[1]));
        }
    }
}

#[test]
fn unsupported_trace_contracts_are_rejected() {
    assert!(std::panic::catch_unwind(|| cache_trace::access_order(0, false, "cyclic")).is_err());
    assert!(std::panic::catch_unwind(|| cache_trace::access_order(2, true, "interior")).is_err());
}
