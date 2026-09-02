//! Deep, wide, sparse, and dense certified-bisimulation acceptance workloads.

use lling_llang::symbolic::bisimulation::{CertifiedBisimulation, Lts};

const SMALL_NATIVE_STACK: usize = 64 * 1024;

fn on_small_stack(name: &str, test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name(name.to_owned())
        .stack_size(SMALL_NATIVE_STACK)
        .spawn(test)
        .expect("spawn certified-bisimulation scale test")
        .join()
        .expect("certified-bisimulation scale test must not overflow or panic");
}

fn assert_linear_core(result: &CertifiedBisimulation, states: usize) {
    let transitions = result.resources().transition_charge_counts().len();
    assert!(result.resources().core_heap_cells() <= 12 * (states + transitions) + 16);
    assert_eq!(result.resources().whole_partition_rescans(), 0);
    assert_eq!(result.resources().maximum_native_frames(), 1);
}

#[test]
fn deep_chain_certificate_and_witness_use_constant_native_stack() {
    const STATES: usize = 100_000;
    on_small_stack("certified-bisimulation-deep", || {
        let transitions = (0..STATES - 1)
            .map(|source| (source, 0, source + 1))
            .collect();
        let lts = Lts::new(STATES, transitions);
        let colors = vec![0; STATES];
        let result = CertifiedBisimulation::compute(&lts, &colors).unwrap();
        assert_linear_core(&result, STATES);

        let witness = result.try_witness(0, STATES - 1).unwrap().unwrap();
        assert!(witness.evaluate(&lts, &colors, 0).unwrap());
        assert!(!witness.evaluate(&lts, &colors, STATES - 1).unwrap());
    });
}

#[test]
fn wide_fanout_uses_constant_native_stack_and_linear_core_heap() {
    const STATES: usize = 100_000;
    on_small_stack("certified-bisimulation-wide", || {
        let transitions = (1..STATES).map(|target| (0, 7, target)).collect();
        let result =
            CertifiedBisimulation::compute(&Lts::new(STATES, transitions), &vec![0; STATES])
                .unwrap();
        assert_linear_core(&result, STATES);
        assert_ne!(result.blocks()[0], result.blocks()[1]);
        assert_eq!(result.blocks()[1], result.blocks()[STATES - 1]);
    });
}

#[test]
fn sparse_large_carrier_uses_constant_native_stack_and_linear_core_heap() {
    const STATES: usize = 200_000;
    on_small_stack("certified-bisimulation-sparse", || {
        let transitions = (0..STATES - 1)
            .step_by(1_000)
            .map(|source| (source, (source % 5) as u32, source + 1))
            .collect();
        let colors = (0..STATES).map(|state| state % 17).collect::<Vec<_>>();
        let result =
            CertifiedBisimulation::compute(&Lts::new(STATES, transitions), &colors).unwrap();
        assert_linear_core(&result, STATES);
    });
}

#[test]
fn dense_multilabel_system_uses_constant_native_stack_and_linear_core_heap() {
    const STATES: usize = 256;
    const ACTIONS: u32 = 4;
    on_small_stack("certified-bisimulation-dense", || {
        let mut transitions = Vec::with_capacity(STATES * STATES * ACTIONS as usize);
        for source in 0..STATES {
            for action in 0..ACTIONS {
                for target in 0..STATES {
                    transitions.push((source, action, target));
                }
            }
        }
        let result =
            CertifiedBisimulation::compute(&Lts::new(STATES, transitions), &vec![0; STATES])
                .unwrap();
        assert_linear_core(&result, STATES);
        assert!(result.blocks().iter().all(|&block| block == 0));
    });
}
