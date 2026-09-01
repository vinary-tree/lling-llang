//! Bit-precise bounded model of typed ABI v2 metadata and handle rules.

const ABI_V2: u32 = 2;
const DESCRIPTOR_SIZE: u32 = 120;
const BUDGET_SIZE: u32 = 72;
const OUTCOME_SIZE: u32 = 96;
const KNOWN_DESCRIPTOR_FLAGS: u64 = 0b111;
const KNOWN_BUDGET_FLAGS: u64 = 0b1111;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Header {
    struct_size: u32,
    abi_version: u32,
    flags: u64,
    reserved: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Id128 {
    bytes: [u8; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Digest256 {
    bytes: [u8; 32],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Descriptor {
    header: Header,
    input_tape: Id128,
    output_tape: Id128,
    algebra: Id128,
    snapshot: Id128,
    context: Digest256,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Budget {
    header: Header,
    max_states: u64,
    max_arcs: u64,
    max_bytes: u64,
    max_work: u64,
    reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Outcome {
    header: Header,
    precision: u32,
    completeness: u32,
    applicability: u32,
    termination: u32,
    evidence: u32,
    reserved0: u32,
    states: u64,
    arcs: u64,
    bytes: u64,
    work: u64,
    limitations: u64,
    reserved1: u64,
}

fn valid_header(header: Header, required_size: u32, known_flags: u64) -> bool {
    header.struct_size >= required_size
        && header.abi_version == ABI_V2
        && header.flags & !known_flags == 0
        && header.reserved == 0
}

fn canonical_limit(enabled: bool, value: u64) -> bool {
    if enabled { value > 0 } else { value == 0 }
}

fn valid_budget(budget: Budget) -> bool {
    valid_header(budget.header, BUDGET_SIZE, KNOWN_BUDGET_FLAGS)
        && canonical_limit(budget.header.flags & 1 != 0, budget.max_states)
        && canonical_limit(budget.header.flags & 2 != 0, budget.max_arcs)
        && canonical_limit(budget.header.flags & 4 != 0, budget.max_bytes)
        && canonical_limit(budget.header.flags & 8 != 0, budget.max_work)
        && budget.reserved == [0; 2]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CancelState {
    Live,
    Cancelled(u32),
}

fn request_cancel(state: CancelState, reason: u32) -> CancelState {
    match state {
        CancelState::Live => CancelState::Cancelled(reason),
        CancelState::Cancelled(first) => CancelState::Cancelled(first),
    }
}

#[kani::proof]
fn metadata_layouts_are_fixed_width() {
    assert_eq!(core::mem::size_of::<Header>(), 24);
    assert_eq!(core::mem::size_of::<Id128>(), 16);
    assert_eq!(core::mem::align_of::<Id128>(), 1);
    assert_eq!(core::mem::size_of::<Digest256>(), 32);
    assert_eq!(core::mem::align_of::<Digest256>(), 1);
    assert_eq!(core::mem::size_of::<Descriptor>(), DESCRIPTOR_SIZE as usize);
    assert_eq!(core::mem::size_of::<Budget>(), BUDGET_SIZE as usize);
    assert_eq!(core::mem::size_of::<Outcome>(), OUTCOME_SIZE as usize);
}

#[kani::proof]
fn additive_headers_reject_unknown_or_reserved_data() {
    let extra: u16 = kani::any();
    let valid = Header {
        struct_size: DESCRIPTOR_SIZE + u32::from(extra),
        abi_version: ABI_V2,
        flags: KNOWN_DESCRIPTOR_FLAGS,
        reserved: 0,
    };
    assert!(valid_header(valid, DESCRIPTOR_SIZE, KNOWN_DESCRIPTOR_FLAGS));

    let unknown = Header { flags: 1 << 63, ..valid };
    assert!(!valid_header(unknown, DESCRIPTOR_SIZE, KNOWN_DESCRIPTOR_FLAGS));
    let reserved = Header { reserved: 1, ..valid };
    assert!(!valid_header(reserved, DESCRIPTOR_SIZE, KNOWN_DESCRIPTOR_FLAGS));
}

#[kani::proof]
fn budget_flags_and_values_are_bijective() {
    let flags: u64 = kani::any();
    let values: [u64; 4] = kani::any();
    kani::assume(flags <= KNOWN_BUDGET_FLAGS);
    let budget = Budget {
        header: Header {
            struct_size: BUDGET_SIZE,
            abi_version: ABI_V2,
            flags,
            reserved: 0,
        },
        max_states: values[0],
        max_arcs: values[1],
        max_bytes: values[2],
        max_work: values[3],
        reserved: [0; 2],
    };
    let expected = (0..4).all(|index| canonical_limit(flags & (1 << index) != 0, values[index]));
    assert_eq!(valid_budget(budget), expected);
}

#[kani::proof]
fn cancellation_reason_is_sticky() {
    let first: u32 = kani::any();
    let second: u32 = kani::any();
    let state = request_cancel(request_cancel(CancelState::Live, first), second);
    assert_eq!(state, CancelState::Cancelled(first));
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HandleState {
    Live,
    Released,
}

fn release(state: HandleState) -> Option<HandleState> {
    match state {
        HandleState::Live => Some(HandleState::Released),
        HandleState::Released => None,
    }
}

#[kani::proof]
fn owned_handles_release_exactly_once() {
    assert_eq!(release(HandleState::Live), Some(HandleState::Released));
    assert_eq!(release(HandleState::Released), None);
}
