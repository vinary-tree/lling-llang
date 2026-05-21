//! Syntactic / semantic layers, feature-gated by their respective integrations.

#[cfg(feature = "pos-tagging")]
pub mod pos_tagging;
#[cfg(feature = "pos-tagging")]
pub use pos_tagging::PosTaggingLayer;

#[cfg(feature = "f1r3fly")]
pub mod mettail_type;
#[cfg(feature = "f1r3fly")]
pub use mettail_type::MeTTaILTypeLayer;
