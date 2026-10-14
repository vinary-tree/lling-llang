use lling_llang::semiring::{Semiring, TropicalWeight};
use lling_llang::tree_transducers::{
    Tree, TreeChild, TreePattern, TreeRule, TreeTransducerLimits, TreeTransducerOps,
    TreeTransducerRunError, VectorTreeTransducer, WeightedTreeTransducer,
};

#[test]
fn bounded_tree_transduction_matches_native_and_stops_expansion() {
    let mut transducer = VectorTreeTransducer::new();
    let state = transducer.add_state();
    transducer.set_start(state);
    transducer.set_final(state, TropicalWeight::one());
    for symbol in ["a", "b"] {
        transducer
            .try_add_rule(TreeRule::new(
                state,
                symbol,
                0,
                TreePattern::leaf(symbol),
                TropicalWeight::one(),
            ))
            .unwrap();
    }
    for pattern in [
        TreePattern::new(
            "T",
            vec![TreeChild::variable(state, 1), TreeChild::variable(state, 0)],
        ),
        TreePattern::new(
            "C",
            vec![TreeChild::variable(state, 0), TreeChild::variable(state, 0)],
        ),
        TreePattern::leaf("D"),
        TreePattern::new(
            "F",
            vec![
                TreeChild::subtree(TreePattern::leaf("fixed")),
                TreeChild::variable(state, 1),
            ],
        ),
    ] {
        transducer
            .try_add_rule(TreeRule::new(state, "S", 2, pattern, TropicalWeight::one()))
            .unwrap();
    }

    let input = Tree::node("S", vec![Tree::leaf("a"), Tree::leaf("b")]);
    let generous = TreeTransducerLimits {
        max_results: 8,
        max_nodes: 100,
        max_bytes: 10_000,
        max_work: 1_000,
    };
    let expected = transducer.transduce(&input);
    assert_eq!(expected.len(), 4);
    assert_eq!(
        transducer.transduce_bounded(&input, generous).unwrap(),
        expected
    );
    assert_eq!(
        transducer
            .transduce_from_state_bounded(state, &input, generous)
            .unwrap(),
        expected
    );
    let first_root_rule = transducer.all_rules()[2];
    assert_eq!(
        transducer
            .apply_rule_bounded(first_root_rule, &input, generous)
            .unwrap(),
        transducer.apply_rule(first_root_rule, &input)
    );
    assert_eq!(
        transducer
            .instantiate_output_pattern_bounded(&first_root_rule.output_pattern, &input, generous,)
            .unwrap(),
        transducer.instantiate_output_pattern(&first_root_rule.output_pattern, &input)
    );

    assert_eq!(
        transducer.transduce_bounded(
            &input,
            TreeTransducerLimits {
                max_results: 1,
                ..generous
            }
        ),
        Err(TreeTransducerRunError::ResultLimitExceeded)
    );
    assert_eq!(
        transducer.transduce_bounded(
            &input,
            TreeTransducerLimits {
                max_nodes: 0,
                ..generous
            }
        ),
        Err(TreeTransducerRunError::NodeLimitExceeded)
    );
    assert_eq!(
        transducer.transduce_bounded(
            &input,
            TreeTransducerLimits {
                max_bytes: 0,
                ..generous
            }
        ),
        Err(TreeTransducerRunError::ByteLimitExceeded)
    );
    assert_eq!(
        transducer.transduce_bounded(
            &input,
            TreeTransducerLimits {
                max_work: 0,
                ..generous
            }
        ),
        Err(TreeTransducerRunError::WorkLimitExceeded)
    );
}
