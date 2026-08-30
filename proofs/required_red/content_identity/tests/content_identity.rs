use proptest::prelude::*;
use vinary_content_identity::{ContentIdentity, IdentityDomain};

fn identities_are_distinct(payload: &[u8]) -> bool {
    ContentIdentity::digest(IdentityDomain::WireSchema, payload)
        != ContentIdentity::digest(IdentityDomain::CanonicalContent, payload)
}

proptest! {
    #[test]
    fn prop_schema_and_content_identity_are_distinct(payload in any::<[u8; 32]>()) {
        prop_assert!(identities_are_distinct(&payload));
    }

    #[test]
    fn prop_wire_and_content_identity_domains_are_separate(payload in any::<[u8; 32]>()) {
        prop_assert!(identities_are_distinct(&payload));
    }

    #[test]
    fn prop_e9_nf_smt_identity_domains_separate(payload in any::<[u8; 32]>()) {
        prop_assert!(identities_are_distinct(&payload));
    }
}
