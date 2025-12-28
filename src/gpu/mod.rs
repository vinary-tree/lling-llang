//! GPU-optimized WFST data structures and algorithms.
//!
//! This module provides data structures and algorithms optimized for GPU execution,
//! based on techniques from high-performance WFST decoders.
//!
//! ## Overview
//!
//! GPU-accelerated WFST decoding can achieve 100-1000× speedup over single-core CPU
//! by exploiting massive parallelism. Key techniques include:
//!
//! 1. **CSR Representation**: Compressed Sparse Row format for cache-efficient access
//! 2. **Token Recombination**: uint64 packing for atomic operations without precision loss
//! 3. **Dynamic Load Balancing**: Cooperative groups with dispatcher pattern
//! 4. **K-Vector Reduction**: Reduce atomic contention with multiple vectors
//! 5. **Channels/Lanes**: Batched streaming for high throughput
//! 6. **Soft Pruning**: Mark-and-compact instead of immediate deallocation
//!
//! ## Architecture
//!
//! The module provides CPU implementations of all data structures that are compatible
//! with GPU execution patterns. Actual GPU kernels can be added via:
//!
//! - `cudarc` crate for CUDA runtime bindings
//! - `rust-cuda` for native CUDA kernel compilation
//! - `wgpu` for portable compute shaders (WebGPU/Vulkan/Metal/DX12)
//!
//! ## Memory Layout
//!
//! GPU-optimized memory layout follows these principles:
//!
//! - **Coalesced access**: Adjacent threads access adjacent memory
//! - **Minimal state**: Decoder state independent of graph size
//! - **Bounded memory**: Predictable memory usage for batched decoding
//!
//! ```text
//! FST Memory:    M_fst = 12|Q| + 8|E| + 4|E_E|
//! Decoder State: M_state = 64α·n_c + 544α·n_l + 1024·n_l
//! ```
//!
//! Where:
//! - |Q| = number of states
//! - |E| = number of transitions
//! - |E_E| = number of emitting transitions
//! - α = max active tokens after pruning
//! - n_c = number of channels
//! - n_l = number of lanes
//!
//! ## Performance Results (from literature)
//!
//! | Configuration | Speedup vs CPU |
//! |--------------|----------------|
//! | Single GPU (V100) | 240× |
//! | 8× GPU (DGX-1) | 1920× |
//! | Edge (Jetson Nano) | 11× |
//!
//! ## References
//!
//! - Laptev et al., "GPU-Accelerated Viterbi Exact Lattice Decoder" (NVIDIA, 2020)
//! - Chen et al., "GPU-based WFST Decoder with Exact Lattice Generation" (2018)
//! - Lv et al., "LET-Decoder: Lazy-evaluation Token-group Decoder"

mod csr;
mod token_recombine;
mod load_balance;
mod k_vector;
mod channels;
mod soft_prune;

// CSR representation for memory-efficient WFST storage
pub use csr::{
    CsrWfst, CsrBuilder, CsrArc, CsrState,
    csr_from_vector_wfst, csr_memory_size,
};

// Token recombination with uint64 packing
pub use token_recombine::{
    PackedToken, TokenPacker, RecombinationBuffer,
    pack_cost_arc, unpack_cost_arc,
};

// Dynamic load balancing
pub use load_balance::{
    WorkGroup, WorkDispatcher, LoadBalancer,
    WorkItem, WorkQueue,
};

// K-vector atomic reduction
pub use k_vector::{
    KVector, KVectorConfig, KVectorStats,
    reduce_with_k_vectors,
};

// Channels/Lanes for batched streaming
pub use channels::{
    Channel, Lane, BatchedDecoder, DecoderConfig,
    ChannelState, LaneState,
};

// Soft pruning
pub use soft_prune::{
    SoftToken, SoftPruneConfig, SoftPruneBuffer,
    SoftPruneStats, AdaptiveBeam, SoftPruneManager,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Verify all public types are accessible
        let _: fn() -> CsrBuilder<u32> = CsrBuilder::new;
        let _: fn(f32, u32) -> u64 = pack_cost_arc;
    }
}
