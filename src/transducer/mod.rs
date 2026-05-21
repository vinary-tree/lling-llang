//! # Neural Transducer Module
//!
//! This module provides WFST-based infrastructure for Neural Transducers (RNN-T/Transducer models),
//! which have become the dominant architecture for production ASR systems.
//!
//! ## Architecture
//!
//! Neural Transducers consist of three components:
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐
//! │   Encoder   │     │  Predictor  │
//! │ (Conformer) │     │   (LSTM)    │
//! └──────┬──────┘     └──────┬──────┘
//!        │                   │
//!        └───────┬───────────┘
//!                ▼
//!         ┌────────────┐
//!         │   Joiner   │
//!         │  (FFN)     │
//!         └──────┬─────┘
//!                ▼
//!          P(y|x,history)
//! ```
//!
//! The WFST framework enables:
//! - Efficient beam search decoding with external LM composition
//! - Differentiable loss computation (k2-style)
//! - Contextual biasing via WFST composition
//! - Unified framework for CTC and RNN-T
//!
//! ## References
//!
//! - [Sequence Transduction with Recurrent Neural Networks (Graves, 2012)](https://arxiv.org/abs/1211.3711)
//! - [k2-fsa/k2: FSA/FST algorithms, differentiable](https://github.com/k2-fsa/k2)
//! - [Factorized Neural Transducer (Microsoft)](https://arxiv.org/abs/2403.13423)

mod decoding;
mod joiner;
mod lattice;
mod loss;
mod traits;

pub use decoding::*;
pub use joiner::*;
pub use lattice::*;
pub use loss::*;
pub use traits::*;
