//! Weighted Pushdown Automata (WPDA).
//!
//! This module implements weighted pushdown automata for recognizing
//! weighted context-free languages. PDAs extend finite automata with
//! a stack, enabling recognition of nested structures.
//!
//! # Mathematical Definition
//!
//! A weighted pushdown automaton is a tuple
//! $`P=(Q,\Sigma,\Gamma,q_0,Z_0,F,\Delta,\rho)`$, where:
//!
//! - $`Q`$: finite set of states.
//! - $`\Sigma`$: input alphabet.
//! - $`\Gamma`$: stack alphabet.
//! - $`q_0`$: initial state.
//! - $`Z_0`$: initial stack symbol.
//! - $`F\subseteq Q`$: final states.
//! - $`\Delta`$: transition relation.
//! - $`\rho`$: final-weight function.
//!
//! # Transition Format
//!
//! ```text
//! (state, input?, stack_top) → (next_state, stack_action, weight)
//! ```
//!
//! Stack actions:
//! - Pop: Remove top symbol
//! - Push: Add symbols to top
//! - Replace: Pop and push (combined operation)
//! - Noop: Leave stack unchanged
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::pushdown::{VectorPda, PdaBuilder, StackSymbol};
//! use lling_llang::semiring::TropicalWeight;
//!
//! // Build a PDA for balanced parentheses
//! let mut builder = PdaBuilder::<char, TropicalWeight>::new();
//!
//! let s0 = builder.add_state();
//! builder.set_start(s0);
//! builder.set_final(s0, TropicalWeight::one());
//!
//! let z0 = builder.initial_stack();
//! let left_paren = builder.add_stack_symbol();
//!
//! // On '(', push left_paren
//! builder.add_transition(s0, Some('('), z0, s0, StackAction::Push(vec![z0, left_paren]));
//!
//! // On ')', pop left_paren
//! builder.add_transition(s0, Some(')'), left_paren, s0, StackAction::Pop);
//!
//! let pda = builder.build();
//! assert!(pda.accepts("(())"));
//! ```
//!
//! # Applications
//!
//! - Weighted parsing beyond FST composition
//! - Nested structure validation
//! - Programming language parsing with weights
//! - XML/JSON validation

mod builder;
mod decode;
mod stack;
mod traits;
mod transition;
mod vector;
pub mod wpds;

pub use builder::{BracketAlphabetError, BracketPair, BuildError, DyckAlphabet, PdaBuilder};
pub use decode::PdaDecoder;
pub use stack::{StackAction, StackSymbol};
pub use traits::{PdaAcceptMode, PdaConfiguration, WeightedPda};
pub use transition::PdaTransition;
pub use vector::{PdaState, VectorPda};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::TropicalWeight;

    #[test]
    fn test_module_imports() {
        // Verify module structure compiles
        let _builder: PdaBuilder<char, TropicalWeight> = PdaBuilder::new();
    }
}
