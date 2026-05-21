//! # Training Module
//!
//! This module provides advanced training objectives for WFST-based models:
//!
//! - **LF-MMI (Lattice-Free Maximum Mutual Information)**: Sequence-discriminative training
//! - **WST (Weakly Supervised Training)**: Training with noisy/imperfect transcripts
//! - **Pruned Training**: Memory-efficient training through composition pruning
//!
//! ## References
//!
//! - [LF-MMI: Purely Sequence-Trained Neural Networks for ASR (Povey et al., 2016)](https://www.danielpovey.com/files/2016_interspeech_mmi.pdf)
//! - [Integrate Lattice-Free MMI into End-to-End Speech Recognition](https://arxiv.org/abs/2203.15614)
//! - [WST: Weakly Supervised Transducer for ASR (arXiv 2511.04035)](https://arxiv.org/abs/2511.04035)

mod lfmmi;
mod pruned;
mod weak_supervision;

pub use lfmmi::*;
pub use pruned::*;
pub use weak_supervision::*;
