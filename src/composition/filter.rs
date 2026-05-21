//! Epsilon filter for WFST composition.
//!
//! During composition, epsilon transitions must be handled carefully to avoid
//! incorrect or duplicate path enumeration. This module implements the epsilon
//! filter from Mohri (2009).
//!
//! # Filter States
//!
//! The filter maintains a state tracking which FST is currently processing
//! an epsilon transition:
//!
//! - `None`: No epsilon in progress, both FSTs can advance
//! - `Eps1`: FST1 output epsilon in progress, only FST2 can advance
//! - `Eps2`: FST2 input epsilon in progress, only FST1 can advance
//!
//! # Example
//!
//! ```rust
//! use lling_llang::composition::{EpsilonFilter, EpsilonFilterType, FilterState};
//!
//! let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);
//! let state = FilterState::None;
//!
//! // Check what transitions are allowed
//! let (can_eps1, can_eps2, can_match) = filter.allowed_moves(state);
//! ```

/// Epsilon filter type (from Mohri 2009).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EpsilonFilterType {
    /// No filter - for epsilon-free FSTs only.
    None,
    /// Sequencing filter - default for general FSTs.
    /// Ensures epsilons are processed in a specific order.
    #[default]
    Sequencing,
    /// Matching filter - for specific applications where
    /// epsilons must match between FSTs.
    Matching,
}

/// Filter state during composition.
///
/// Tracks which FST is currently in the middle of an epsilon transition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FilterState {
    /// No epsilon in progress - both FSTs can advance.
    #[default]
    None,
    /// FST1 output epsilon in progress.
    /// Only FST2 can advance or FST1 can output more epsilons.
    Eps1,
    /// FST2 input epsilon in progress.
    /// Only FST1 can advance or FST2 can consume more epsilons.
    Eps2,
}

/// Epsilon filter for WFST composition.
///
/// Manages epsilon transitions during composition to ensure correct
/// and non-redundant path enumeration.
#[derive(Clone, Debug)]
pub struct EpsilonFilter {
    filter_type: EpsilonFilterType,
}

impl Default for EpsilonFilter {
    fn default() -> Self {
        Self {
            filter_type: EpsilonFilterType::Sequencing,
        }
    }
}

impl EpsilonFilter {
    /// Create a new epsilon filter with the given type.
    pub fn new(filter_type: EpsilonFilterType) -> Self {
        Self { filter_type }
    }

    /// Get the filter type.
    pub fn filter_type(&self) -> EpsilonFilterType {
        self.filter_type
    }

    /// Determine allowed moves from the current filter state.
    ///
    /// Returns `(can_eps1_output, can_eps2_input, can_match)`:
    /// - `can_eps1_output`: FST1 can output epsilon (advance FST1 only)
    /// - `can_eps2_input`: FST2 can consume epsilon (advance FST2 only)
    /// - `can_match`: Both FSTs can advance on matching label
    pub fn allowed_moves(&self, state: FilterState) -> (bool, bool, bool) {
        match self.filter_type {
            EpsilonFilterType::None => {
                // No filter - everything allowed
                (true, true, true)
            }
            EpsilonFilterType::Sequencing => {
                match state {
                    FilterState::None => (true, true, true),
                    FilterState::Eps1 => (true, false, true), // FST1 eps or match
                    FilterState::Eps2 => (false, true, true), // FST2 eps or match
                }
            }
            EpsilonFilterType::Matching => {
                match state {
                    FilterState::None => (true, true, true),
                    FilterState::Eps1 => (true, true, false), // Epsilons only
                    FilterState::Eps2 => (true, true, false), // Epsilons only
                }
            }
        }
    }

    /// Compute the next filter state after a transition.
    ///
    /// # Arguments
    ///
    /// * `_current` - Current filter state (unused but needed for interface consistency)
    /// * `eps1_output` - FST1 produced output epsilon
    /// * `eps2_input` - FST2 consumed input epsilon
    pub fn next_state(
        &self,
        _current: FilterState,
        eps1_output: bool,
        eps2_input: bool,
    ) -> FilterState {
        match self.filter_type {
            EpsilonFilterType::None => FilterState::None,
            EpsilonFilterType::Sequencing => {
                if eps1_output && !eps2_input {
                    FilterState::Eps1
                } else if eps2_input && !eps1_output {
                    FilterState::Eps2
                } else {
                    // Both epsilon (eps-eps) or both non-epsilon (match)
                    FilterState::None
                }
            }
            EpsilonFilterType::Matching => {
                // Matching filter returns to None only on eps-eps or match
                if eps1_output == eps2_input {
                    FilterState::None
                } else if eps1_output {
                    FilterState::Eps1
                } else {
                    FilterState::Eps2
                }
            }
        }
    }

    /// Check if a transition is allowed given the filter state.
    ///
    /// # Arguments
    ///
    /// * `state` - Current filter state
    /// * `eps1_output` - FST1 would produce output epsilon
    /// * `eps2_input` - FST2 would consume input epsilon
    /// * `is_match` - Labels match (non-epsilon transition)
    pub fn is_transition_allowed(
        &self,
        state: FilterState,
        eps1_output: bool,
        eps2_input: bool,
        is_match: bool,
    ) -> bool {
        let (can_eps1, can_eps2, can_match) = self.allowed_moves(state);

        if is_match {
            can_match
        } else if eps1_output && !eps2_input {
            can_eps1
        } else if eps2_input && !eps1_output {
            can_eps2
        } else if eps1_output && eps2_input {
            // Both epsilon - allowed if either is allowed
            can_eps1 || can_eps2
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_type_default() {
        let filter = EpsilonFilter::default();
        assert_eq!(filter.filter_type(), EpsilonFilterType::Sequencing);
    }

    #[test]
    fn test_filter_state_default() {
        let state = FilterState::default();
        assert_eq!(state, FilterState::None);
    }

    #[test]
    fn test_no_filter_allows_all() {
        let filter = EpsilonFilter::new(EpsilonFilterType::None);

        for state in [FilterState::None, FilterState::Eps1, FilterState::Eps2] {
            let (eps1, eps2, match_) = filter.allowed_moves(state);
            assert!(eps1);
            assert!(eps2);
            assert!(match_);
        }
    }

    #[test]
    fn test_sequencing_filter_none_state() {
        let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);

        let (eps1, eps2, match_) = filter.allowed_moves(FilterState::None);
        assert!(eps1);
        assert!(eps2);
        assert!(match_);
    }

    #[test]
    fn test_sequencing_filter_eps1_state() {
        let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);

        let (eps1, eps2, match_) = filter.allowed_moves(FilterState::Eps1);
        assert!(eps1); // FST1 can continue with epsilons
        assert!(!eps2); // FST2 cannot start epsilon sequence
        assert!(match_); // Matching still allowed
    }

    #[test]
    fn test_sequencing_filter_eps2_state() {
        let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);

        let (eps1, eps2, match_) = filter.allowed_moves(FilterState::Eps2);
        assert!(!eps1); // FST1 cannot start epsilon sequence
        assert!(eps2); // FST2 can continue with epsilons
        assert!(match_); // Matching still allowed
    }

    #[test]
    fn test_next_state_sequencing() {
        let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);

        // eps1 output -> Eps1 state
        assert_eq!(
            filter.next_state(FilterState::None, true, false),
            FilterState::Eps1
        );

        // eps2 input -> Eps2 state
        assert_eq!(
            filter.next_state(FilterState::None, false, true),
            FilterState::Eps2
        );

        // Match (both non-eps) -> None
        assert_eq!(
            filter.next_state(FilterState::Eps1, false, false),
            FilterState::None
        );

        // Both epsilon -> None
        assert_eq!(
            filter.next_state(FilterState::None, true, true),
            FilterState::None
        );
    }

    #[test]
    fn test_is_transition_allowed_match() {
        let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);

        // Match allowed in all states
        assert!(filter.is_transition_allowed(FilterState::None, false, false, true));
        assert!(filter.is_transition_allowed(FilterState::Eps1, false, false, true));
        assert!(filter.is_transition_allowed(FilterState::Eps2, false, false, true));
    }

    #[test]
    fn test_is_transition_allowed_eps1() {
        let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);

        // eps1 output
        assert!(filter.is_transition_allowed(FilterState::None, true, false, false));
        assert!(filter.is_transition_allowed(FilterState::Eps1, true, false, false));
        assert!(!filter.is_transition_allowed(FilterState::Eps2, true, false, false));
    }

    #[test]
    fn test_is_transition_allowed_eps2() {
        let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);

        // eps2 input
        assert!(filter.is_transition_allowed(FilterState::None, false, true, false));
        assert!(!filter.is_transition_allowed(FilterState::Eps1, false, true, false));
        assert!(filter.is_transition_allowed(FilterState::Eps2, false, true, false));
    }

    #[test]
    fn test_matching_filter() {
        let filter = EpsilonFilter::new(EpsilonFilterType::Matching);

        // In Eps1 or Eps2, matching is not allowed (only epsilons)
        let (_, _, match_) = filter.allowed_moves(FilterState::Eps1);
        assert!(!match_);

        let (_, _, match_) = filter.allowed_moves(FilterState::Eps2);
        assert!(!match_);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    /// Strategy for generating arbitrary filter types.
    fn arb_filter_type() -> impl Strategy<Value = EpsilonFilterType> {
        prop_oneof![
            Just(EpsilonFilterType::None),
            Just(EpsilonFilterType::Sequencing),
            Just(EpsilonFilterType::Matching),
        ]
    }

    /// Strategy for generating arbitrary filter states.
    fn arb_filter_state() -> impl Strategy<Value = FilterState> {
        prop_oneof![
            Just(FilterState::None),
            Just(FilterState::Eps1),
            Just(FilterState::Eps2),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// next_state is deterministic - same inputs always produce same output.
        #[test]
        fn next_state_deterministic(
            filter_type in arb_filter_type(),
            state in arb_filter_state(),
            eps1 in any::<bool>(),
            eps2 in any::<bool>()
        ) {
            let filter = EpsilonFilter::new(filter_type);
            let result1 = filter.next_state(state, eps1, eps2);
            let result2 = filter.next_state(state, eps1, eps2);
            prop_assert_eq!(result1, result2);
        }

        /// No filter type always returns FilterState::None.
        #[test]
        fn no_filter_always_none(
            state in arb_filter_state(),
            eps1 in any::<bool>(),
            eps2 in any::<bool>()
        ) {
            let filter = EpsilonFilter::new(EpsilonFilterType::None);
            let result = filter.next_state(state, eps1, eps2);
            prop_assert_eq!(result, FilterState::None);
        }

        /// No filter allows all moves in all states.
        #[test]
        fn no_filter_allows_all(state in arb_filter_state()) {
            let filter = EpsilonFilter::new(EpsilonFilterType::None);
            let (eps1, eps2, match_) = filter.allowed_moves(state);
            prop_assert!(eps1);
            prop_assert!(eps2);
            prop_assert!(match_);
        }

        /// Sequencing filter allows matching in all states.
        #[test]
        fn sequencing_allows_match(state in arb_filter_state()) {
            let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);
            let (_, _, match_) = filter.allowed_moves(state);
            prop_assert!(match_);
        }

        /// Matching filter disallows matching in Eps1 and Eps2.
        #[test]
        fn matching_disallows_match_in_eps_states(state in arb_filter_state()) {
            let filter = EpsilonFilter::new(EpsilonFilterType::Matching);
            let (_, _, match_) = filter.allowed_moves(state);
            match state {
                FilterState::None => prop_assert!(match_),
                FilterState::Eps1 | FilterState::Eps2 => prop_assert!(!match_),
            }
        }

        /// From None state, both filters start fresh.
        #[test]
        fn none_state_symmetric(filter_type in arb_filter_type()) {
            let filter = EpsilonFilter::new(filter_type);
            let (eps1, eps2, match_) = filter.allowed_moves(FilterState::None);
            // From None, both eps1 and eps2 should be allowed
            prop_assert!(eps1);
            prop_assert!(eps2);
            prop_assert!(match_);
        }

        /// Sequencing filter state transitions are symmetric for eps1/eps2.
        #[test]
        fn sequencing_symmetric(eps1_only in any::<bool>()) {
            let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);

            // Test that eps1 alone leads to Eps1 state, eps2 alone to Eps2
            if eps1_only {
                let next = filter.next_state(FilterState::None, true, false);
                prop_assert_eq!(next, FilterState::Eps1);
            } else {
                let next = filter.next_state(FilterState::None, false, true);
                prop_assert_eq!(next, FilterState::Eps2);
            }
        }

        /// Match transition resets to None for sequencing filter.
        #[test]
        fn sequencing_match_resets(state in arb_filter_state()) {
            let filter = EpsilonFilter::new(EpsilonFilterType::Sequencing);
            // A match (both non-eps) should reset to None
            let next = filter.next_state(state, false, false);
            prop_assert_eq!(next, FilterState::None);
        }

        /// is_transition_allowed is consistent with allowed_moves.
        #[test]
        fn is_transition_allowed_consistent(
            filter_type in arb_filter_type(),
            state in arb_filter_state()
        ) {
            let filter = EpsilonFilter::new(filter_type);
            let (can_eps1, can_eps2, can_match) = filter.allowed_moves(state);

            // Check match case
            prop_assert_eq!(
                filter.is_transition_allowed(state, false, false, true),
                can_match
            );

            // Check eps1 only case
            prop_assert_eq!(
                filter.is_transition_allowed(state, true, false, false),
                can_eps1
            );

            // Check eps2 only case
            prop_assert_eq!(
                filter.is_transition_allowed(state, false, true, false),
                can_eps2
            );
        }

        /// Both eps transitions need at least one allowed.
        #[test]
        fn both_eps_needs_at_least_one(
            filter_type in arb_filter_type(),
            state in arb_filter_state()
        ) {
            let filter = EpsilonFilter::new(filter_type);
            let (can_eps1, can_eps2, _) = filter.allowed_moves(state);

            // Both eps is allowed if either is allowed
            let both_eps_allowed = filter.is_transition_allowed(state, true, true, false);
            prop_assert_eq!(both_eps_allowed, can_eps1 || can_eps2);
        }
    }
}
