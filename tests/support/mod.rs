//! Shared in-repo ABI test infrastructure for the `ffi` integration suites.
//!
//! Per the family placement rule, lling-llang tests use PROJECT-OWNED
//! providers only: no duallity (or other dependent-crate) fixtures may appear
//! here. Every foreign `vt.scalar-wfst.1` or `vt.dictionary.v1` producer these
//! suites exercise is hand-rolled in this module tree.
//!
//! Each integration-test crate that declares `mod support;` compiles its own
//! copy and typically uses a subset of the harness, so dead-code lints are
//! silenced for the whole subtree.
#![allow(dead_code)]

pub mod interop_wfst;
