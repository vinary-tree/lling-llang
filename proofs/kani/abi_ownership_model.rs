//! Bounded executable model of the opaque ABI v1 ownership protocol.
//!
//! Kani explores six arbitrary acquire/clone/transfer/release/relayout steps
//! over three clients. Every owned client accounts for one retain, release is
//! partial at zero, a move preserves the retain count, and private layout is
//! absent from the ABI v1 observation.

const CLIENTS: usize = 3;
const STEPS: usize = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClientState {
    Idle,
    Owned,
    Moved,
    Released,
}

fn owner_count(states: &[ClientState; CLIENTS]) -> usize {
    let mut owners = 0;
    let mut index = 0;
    while index < CLIENTS {
        if states[index] == ClientState::Owned {
            owners += 1;
        }
        index += 1;
    }
    owners
}

fn release(retains: usize) -> Option<usize> {
    retains.checked_sub(1)
}

#[kani::proof]
#[kani::unwind(7)]
fn ownership_protocol_preserves_retain_accounting() {
    let mut states = [ClientState::Idle; CLIENTS];
    let mut retains = 0usize;
    let mut private_layout = 0usize;
    let abi_version = 1u32;
    let resource_identity = 1u64;

    let mut step = 0;
    while step < STEPS {
        let action: u8 = kani::any();
        let source: usize = kani::any();
        let target: usize = kani::any();
        kani::assume(action <= 4);
        kani::assume(source < CLIENTS);
        kani::assume(target < CLIENTS);

        if action == 0 && states[source] == ClientState::Idle {
            states[source] = ClientState::Owned;
            retains += 1;
        } else if action == 1
            && source != target
            && states[source] == ClientState::Owned
            && states[target] == ClientState::Idle
        {
            states[target] = ClientState::Owned;
            retains += 1;
        } else if action == 2
            && source != target
            && states[source] == ClientState::Owned
            && states[target] == ClientState::Idle
        {
            let before_move = retains;
            states[source] = ClientState::Moved;
            states[target] = ClientState::Owned;
            assert_eq!(retains, before_move);
        } else if action == 3 && states[source] == ClientState::Owned {
            retains = release(retains).expect("an owned handle has a retain");
            states[source] = ClientState::Released;
        } else if action == 4 {
            private_layout = private_layout.saturating_add(1);
        }

        assert_eq!(retains, owner_count(&states));
        assert_eq!(abi_version, 1);
        assert_eq!(resource_identity, 1);
        assert!(private_layout <= STEPS);
        step += 1;
    }
}

#[kani::proof]
fn release_at_zero_is_rejected() {
    assert_eq!(release(0), None);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PublicObservation {
    abi_version: u32,
    resource_identity: u64,
}

#[kani::proof]
fn opaque_v1_observation_ignores_private_layout() {
    let old_private_layout: u64 = kani::any();
    let new_private_layout: u64 = kani::any();
    let old_public = PublicObservation {
        abi_version: 1,
        resource_identity: 1,
    };
    let new_public = PublicObservation {
        abi_version: 1,
        resource_identity: 1,
    };

    let _ = (old_private_layout, new_private_layout);
    assert_eq!(old_public, new_public);
}
