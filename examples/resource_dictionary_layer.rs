use std::error::Error;
use std::sync::Arc;

use libdictenstein::bindings::{BindingUnitDomain, DynamicDawgBinding};
use lling_llang::layers::{EditDistanceLayer, ResourceDictionary, ResourceDictionaryNormalization};
use lling_llang::semiring::TropicalWeight;

fn main() -> Result<(), Box<dyn Error>> {
    let producer = DynamicDawgBinding::new(BindingUnitDomain::UnicodeScalar);
    for term in ["ten", "the", "then"] {
        producer.insert_text(term.as_bytes(), None)?;
    }

    let resource = producer.resource();
    // SAFETY: OwnedDictionaryResource keeps the ABI vtables and callback
    // implementation valid. ResourceDictionary retains its own snapshot.
    let dictionary = unsafe {
        ResourceDictionary::from_resource_with_normalization(
            resource.as_raw(),
            ResourceDictionaryNormalization::UnicodeLowercaseKeys,
        )
    }?;
    let layer = EditDistanceLayer::<TropicalWeight>::with_dictionary(Arc::new(dictionary))
        .with_max_distance(1)
        .with_max_corrections(3);

    let corrections = layer.find_corrections("TEH")?;
    assert!(corrections
        .iter()
        .any(|(term, cost)| term == "the" && (*cost - 1.0).abs() < f64::EPSILON));
    println!("{corrections:?}");
    Ok(())
}
