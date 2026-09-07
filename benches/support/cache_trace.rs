/// Match the allocation harness's fixed traces, including its numerical seed
/// order: shuffled links then occupy nonadjacent physical residency slots.
pub fn access_order(capacity: usize, miss: bool, trace: &str) -> Vec<usize> {
    assert!(capacity > 0, "positive residency capacity");
    let mut order: Vec<_> = (0..capacity + usize::from(miss)).collect();
    match trace {
        "cyclic" => {}
        "shuffled" => {
            let mut random = 0x716d_192b_33ae_62d5u64;
            for index in (1..order.len()).rev() {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                order.swap(index, (random % (index as u64 + 1)) as usize);
            }
        }
        "interior" => {
            assert!(!miss, "interior trace describes resident hits only");
            let mut random = 0x482a_0753_c59f_813du64;
            order = (0..10_000)
                .map(|_| {
                    random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                    ((random >> 32) as usize) % capacity
                })
                .collect();
        }
        _ => unreachable!("enumerated access trace"),
    }
    order
}
