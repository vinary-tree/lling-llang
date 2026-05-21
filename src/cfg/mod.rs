//! Context-free grammar types and Earley parser.
//!
//! This module provides:
//! - [`Grammar`]: Context-free grammar representation
//! - [`EarleyParser`]: Earley parser modified for lattice input
//! - [`ParseForest`]: Compact representation of ambiguous parses
//! - [`ParseTree`]: Single parse tree extraction
//!
//! # Earley Parsing on Lattices
//!
//! The Earley parser is modified to work with lattices instead of strings:
//! - The Scanner step follows lattice edges instead of string positions
//! - Multiple edges at a position are handled naturally
//! - The resulting parse forest can filter the lattice to grammatical paths
//!
//! # Example
//!
//! ```rust,ignore
//! use lling_llang::cfg::{GrammarBuilder, EarleyParser};
//!
//! let grammar = GrammarBuilder::new()
//!     .rule("S", &["NP", "VP"])
//!     .rule("NP", &["Det", "N"])
//!     .rule("VP", &["V", "NP"])
//!     .terminal("Det", &["the", "a"])
//!     .terminal("N", &["dog", "cat"])
//!     .terminal("V", &["saw", "chased"])
//!     .build();
//!
//! let parser = EarleyParser::new(&grammar);
//! let forest = parser.parse_lattice(&lattice)?;
//! ```

mod builder;
mod earley;
mod forest;
mod grammar;
mod types;

pub use builder::GrammarBuilder;
pub use earley::{EarleyChart, EarleyParser, EarleyState, ParseError};
pub use forest::{ForestNode, ForestNodeId, ParseForest, ParseTree};
pub use grammar::{Grammar, GrammarError, Production};
pub use types::{NonTerminal, RuleId, Symbol, SymbolKind, Terminal};
