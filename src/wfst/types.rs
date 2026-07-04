//! Shared WFST identifiers.

/// State identifier for WFST states.
///
/// Uses `u32` for compact storage while supporting millions of states.
pub type StateId = u32;

/// Sentinel value indicating no state.
pub const NO_STATE: StateId = StateId::MAX;
