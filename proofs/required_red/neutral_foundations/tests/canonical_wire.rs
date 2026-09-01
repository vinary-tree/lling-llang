use proptest::prelude::*;
use vinary_canonical_json::{
    CanonicalProfileId, DigestByteSink, NumericDomain, VINARY_CANONICAL_JSON_V1,
};
use vinary_wire_schema::SchemaFingerprintDigest;

fn canonical_profile_contract() {
    let _ = CanonicalProfileId::from_static(VINARY_CANONICAL_JSON_V1);
    let _ = NumericDomain::FiniteJsonNumbers;
    let _ = DigestByteSink::sha256();
    let _ = SchemaFingerprintDigest::for_type::<u64>();
}

proptest! {
    #[test]
    fn prop_non_finite_numbers_are_rejected(bits in any::<u64>()) {
        canonical_profile_contract();
        prop_assert!(!f64::from_bits(bits).is_finite() || NumericDomain::FiniteJsonNumbers.admits_bits(bits));
    }
    #[test]
    fn prop_negative_zero_has_the_zero_encoding(negative in any::<bool>()) {
        canonical_profile_contract();
        let bits = if negative { (-0.0_f64).to_bits() } else { 0.0_f64.to_bits() };
        prop_assert_eq!(VINARY_CANONICAL_JSON_V1.encode_f64_bits(bits), b"0");
    }
    #[test]
    fn prop_rejected_chunk_is_atomic(prefix in proptest::collection::vec(any::<u8>(), 0..64), chunk in proptest::collection::vec(any::<u8>(), 1..64)) {
        canonical_profile_contract();
        let mut sink = DigestByteSink::bounded(prefix.clone(), 0);
        let _ = sink.write_atomic(&chunk);
        prop_assert_eq!(sink.buffered_bytes(), prefix.as_slice());
    }
    #[test]
    fn prop_streaming_and_buffered_emission_agree(chunks in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..32), 0..16)) {
        canonical_profile_contract();
        let buffered: Vec<_> = chunks.iter().flatten().copied().collect();
        prop_assert_eq!(VINARY_CANONICAL_JSON_V1.digest_chunks(&chunks), VINARY_CANONICAL_JSON_V1.digest(&buffered));
    }
    #[test]
    fn prop_wire_machine_native_stack_is_constant(depth in 0_usize..100_000) {
        canonical_profile_contract();
        prop_assert_eq!(VINARY_CANONICAL_JSON_V1.native_frame_bound(depth), 1);
    }
    #[test]
    fn prop_malformed_and_budget_outcomes_are_not_success(offset in any::<u32>()) {
        canonical_profile_contract();
        prop_assert!(!VINARY_CANONICAL_JSON_V1.malformed(offset).is_success());
        prop_assert!(!VINARY_CANONICAL_JSON_V1.budget_exhausted(offset as u64).is_success());
    }
    #[test]
    fn prop_e9_nf_smt_nonfinite_number_rejected(bits in any::<u64>()) {
        canonical_profile_contract();
        prop_assume!(!f64::from_bits(bits).is_finite());
        prop_assert!(VINARY_CANONICAL_JSON_V1.encode_f64_bits(bits).is_err());
    }
    #[test]
    fn prop_e9_nf_smt_sink_rejection_atomic(prefix in proptest::collection::vec(any::<u8>(), 0..32), chunk in proptest::collection::vec(any::<u8>(), 1..32)) {
        canonical_profile_contract();
        let mut sink = DigestByteSink::bounded(prefix.clone(), 0);
        prop_assert!(sink.write_atomic(&chunk).is_err());
        prop_assert_eq!(sink.buffered_bytes(), prefix.as_slice());
    }
}
