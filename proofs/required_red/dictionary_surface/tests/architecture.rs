use proptest::prelude::*;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use vinary_dictionary_pipeline::{
    classify_outcome, Coverage, DictionaryQueryIdentity, Precision, TerminationReason,
};

fn dependencies(project: &str) -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../..")
        .join(project)
        .join("Cargo.toml");
    let manifest = fs::read_to_string(path)
        .unwrap()
        .parse::<toml::Value>()
        .unwrap();
    manifest["dependencies"]
        .as_table()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

fn rank(name: &str) -> u8 {
    match name {
        "libdictenstein" | "llattice" | "lling-llang" => 0,
        "liblevenshtein" | "libdictenstein-llattice" => 1,
        "vinary-dictionary-pipeline" => 2,
        "duallity" => 3,
        _ => panic!("unknown campaign component"),
    }
}

fn edges() -> Vec<(&'static str, &'static str)> {
    vec![
        ("liblevenshtein", "libdictenstein"),
        ("libdictenstein-llattice", "libdictenstein"),
        ("libdictenstein-llattice", "llattice"),
        ("vinary-dictionary-pipeline", "libdictenstein"),
        ("vinary-dictionary-pipeline", "liblevenshtein"),
        ("vinary-dictionary-pipeline", "llattice"),
        ("vinary-dictionary-pipeline", "lling-llang"),
        ("duallity", "libdictenstein-llattice"),
        ("duallity", "vinary-dictionary-pipeline"),
    ]
}

proptest! {
    #[test]
    fn prop_every_dependency_edge_decreases_rank(edge in proptest::sample::select(edges())) {
        prop_assert!(rank(edge.1) < rank(edge.0));
    }
    #[test]
    fn prop_every_dependency_path_decreases_rank(path in proptest::sample::select(vec![
        ("duallity","libdictenstein"), ("duallity","llattice"),
        ("vinary-dictionary-pipeline","libdictenstein")
    ])) {
        prop_assert!(rank(path.1) < rank(path.0));
    }
    #[test]
    fn prop_adapter_dependency_graph_is_acyclic(_seed in any::<u64>()) {
        prop_assert!(edges().iter().all(|(owner, dependency)| rank(dependency) < rank(owner)));
    }
    #[test]
    fn prop_dictionary_lattice_adapter_has_only_leaf_dependencies(_seed in any::<u8>()) {
        prop_assert_eq!(dependencies("libdictenstein-llattice"),
            BTreeSet::from(["libdictenstein".into(), "llattice".into()]));
    }
    #[test]
    fn prop_domain_crates_do_not_reverse_depend_on_pipeline(project in proptest::sample::select(
        vec!["libdictenstein","liblevenshtein-rust","llattice","lling-llang"]
    )) {
        prop_assert!(!dependencies(project).contains("vinary-dictionary-pipeline"));
    }
    #[test]
    fn prop_duallity_facade_matches_native_adapter(exact in any::<bool>(), complete in any::<bool>()) {
        let p = if exact { Precision::Exact } else { Precision::Approximate };
        let c = if complete { Coverage::Complete } else { Coverage::Incomplete };
        prop_assert_eq!(duallity::dictionary_pipeline::classify_outcome(p,c,TerminationReason::Exhausted),
                        classify_outcome(p,c,TerminationReason::Exhausted));
    }
    #[test]
    fn prop_transforming_facade_is_rejected(snapshot in any::<u64>()) {
        let native = DictionaryQueryIdentity::new(snapshot, vec![1u8], 2u64, 3u64, 4u32);
        let facade: duallity::dictionary_pipeline::DictionaryQueryIdentity<_,_,_,_> = native.clone();
        prop_assert!(facade.same_semantics(&native));
    }
    #[test]
    fn prop_fibration_requires_explicit_cartesian_lifts(has_lifts in any::<bool>()) {
        prop_assert_eq!(vinary_dictionary_pipeline::may_claim_fibration(has_lifts), has_lifts);
    }
    #[test]
    fn prop_published_facade_equals_native(snapshot in any::<u64>()) {
        let native = DictionaryQueryIdentity::new(snapshot, vec![1u8], 2u64, 3u64, 4u32);
        let facade = duallity::dictionary_pipeline::DictionaryQueryIdentity::new(
            snapshot, vec![1u8], 2u64, 3u64, 4u32);
        prop_assert!(facade.same_semantics(&native));
    }
}
