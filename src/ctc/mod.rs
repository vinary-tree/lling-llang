//! CTC (Connectionist Temporal Classification) WFST topologies and decoding.
//!
//! This module provides various CTC topology implementations as WFSTs,
//! enabling different trade-offs between graph size and expressiveness,
//! along with decoders for CTC-WFST integration.
//!
//! ## Topology Comparison
//!
//! Graph-size and memory figures are those reported by Laptev et al. (2022).
//!
//! | Topology | States | Arcs | Memory | Accuracy |
//! |----------|--------|------|--------|----------|
//! | Correct-CTC | N | N² | Baseline | Best |
//! | Compact-CTC | N | 3N-2 | 1.5× smaller | Same |
//! | Minimal-CTC | 1 | N | 2× smaller | Slight penalty |
//! | Selfless variants | varies | fewer | varies | Better for wide context |
//!
//! ## CTC Blank Token
//!
//! All topologies use label index 0 as the blank token. The blank token:
//! - Allows the model to emit "nothing" at a frame
//! - Separates repeated labels (e.g., "aa" needs blank between: a-blank-a)
//! - Is removed from the final output sequence
//!
//! ## Example
//!
//! ```
//! use lling_llang::ctc::{CtcTopology, correct_ctc, compact_ctc, minimal_ctc};
//! use lling_llang::semiring::LogWeight;
//!
//! // Create a Correct-CTC topology for 10 labels (including blank at index 0)
//! let correct = correct_ctc::<LogWeight>(10);
//! assert_eq!(correct.info().num_states, 10);
//! assert_eq!(correct.info().num_arcs, 100); // N²
//!
//! // Create a Compact-CTC topology (smaller graph, same accuracy)
//! let compact = compact_ctc::<LogWeight>(10);
//! assert_eq!(compact.info().num_states, 10);
//! assert_eq!(compact.info().num_arcs, 28); // 3N-2
//!
//! // Create a Minimal-CTC topology (smallest graph, slight accuracy penalty)
//! let minimal = minimal_ctc::<LogWeight>(10);
//! assert_eq!(minimal.info().num_states, 1);
//! assert_eq!(minimal.info().num_arcs, 10); // N
//! ```
//!
//! ## Decoding Example
//!
//! ```ignore
//! use lling_llang::ctc::{CtcDecoder, CtcDecoderConfig, compact_ctc};
//! use lling_llang::semiring::LogWeight;
//!
//! // Create decoder with compact CTC topology
//! let ctc = compact_ctc::<LogWeight>(vocab_size);
//! let decoder = CtcDecoder::new(ctc)
//!     .with_config(CtcDecoderConfig::beam(10.0));
//!
//! // Decode posteriors
//! let result = decoder.decode(&posteriors)?;
//! println!("Labels: {:?}, Score: {}", result.labels, result.score);
//! ```
//!
//! ## References
//!
//! - Graves et al., "Connectionist Temporal Classification" (ICML 2006)
//! - Laptev et al., "CTC Variations Through New WFST Topologies" (Interspeech 2022)
//! - Miao et al., "EESEN: End-to-end speech recognition using deep RNN" (ASRU 2015)

mod decoder;
mod topologies;

pub use topologies::{
    compact_ctc, correct_ctc, minimal_ctc, selfless_compact_ctc, selfless_correct_ctc, CtcLabel,
    CtcTopology, CtcTopologyInfo, BLANK,
};

pub use decoder::{
    CtcDecoder, CtcDecoderConfig, DecoderToken, DecodingError, DecodingResult, DecodingStats,
    ObservationFst, StreamingCtcDecoder,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::LogWeight;

    #[test]
    fn test_correct_ctc_graph_size() {
        let ctc = correct_ctc::<LogWeight>(10);
        let info = ctc.info();
        assert_eq!(info.num_states, 10);
        assert_eq!(info.num_arcs, 100); // N²
    }

    #[test]
    fn test_compact_ctc_graph_size() {
        let ctc = compact_ctc::<LogWeight>(10);
        let info = ctc.info();
        assert_eq!(info.num_states, 10);
        assert_eq!(info.num_arcs, 28); // 3N-2
    }

    #[test]
    fn test_minimal_ctc_graph_size() {
        let ctc = minimal_ctc::<LogWeight>(10);
        let info = ctc.info();
        assert_eq!(info.num_states, 1);
        assert_eq!(info.num_arcs, 10); // N
    }

    #[test]
    fn test_selfless_reduces_arcs() {
        let correct = correct_ctc::<LogWeight>(10);
        let selfless = selfless_correct_ctc::<LogWeight>(10);

        // Selfless removes N-1 self-loops (non-blank labels only)
        assert_eq!(
            correct.info().num_arcs - selfless.info().num_arcs,
            9 // N-1 self-loops removed
        );
    }
}
