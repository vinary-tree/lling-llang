use vinary_content_identity::{ContentIdentity, IdentityDomain};

/// Required-red domain-separation witness for the neutral identity primitive.
pub fn domains_are_distinct(bytes: &[u8]) -> bool {
    ContentIdentity::digest(IdentityDomain::WireSchema, bytes)
        != ContentIdentity::digest(IdentityDomain::CanonicalContent, bytes)
}
