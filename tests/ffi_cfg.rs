#![cfg(feature = "ffi")]

use lling_llang::backend::HashMapBackend;
use lling_llang::cfg::{
    EarleyParser, Grammar, NonTerminal, ParseLimits, Production, RuleId, Symbol,
};
use lling_llang::ffi::*;
use lling_llang::lattice::{EdgeMetadata, LatticeBuilder};
use lling_llang::semiring::TropicalWeight;
use smallvec::smallvec;
use std::ptr;

fn limits() -> LlingCfgParseLimits {
    LlingCfgParseLimits {
        struct_size: std::mem::size_of::<LlingCfgParseLimits>() as u32,
        version: 1,
        max_tokens: 16,
        max_chart_items: 128,
        max_forest_nodes: 128,
        max_work: 1024,
    }
}

unsafe fn grammar(rules: &[LlingCfgRule], rhs_bound: usize) -> *mut LlingCfgGrammar {
    let mut output = ptr::null_mut();
    assert_eq!(
        unsafe {
            lling_cfg_grammar_compile(0, 1, rules.as_ptr(), rules.len(), 8, rhs_bound, &mut output)
        },
        LlingLlangStatus::Ok
    );
    assert!(!output.is_null());
    output
}

#[test]
fn token_analysis_is_bounded_deterministic_and_owns_its_grammar_snapshot() {
    let rhs = [LlingCfgSymbol {
        kind: LLING_CFG_TERMINAL,
        value: 7,
    }];
    let rules = [LlingCfgRule {
        lhs: 0,
        rhs: rhs.as_ptr(),
        rhs_len: 1,
        log_prob: 0.0,
    }];
    let grammar = unsafe { grammar(&rules, 1) };
    let config = limits();
    let mut analysis = ptr::null_mut();
    assert_eq!(
        unsafe { lling_cfg_parse_tokens(grammar, [7u32].as_ptr(), 1, &config, &mut analysis) },
        LlingLlangStatus::Ok
    );
    unsafe { lling_cfg_grammar_free(grammar) };
    let mut info = LlingCfgAnalysisInfo {
        accepted: 0,
        reserved: [0; 7],
        chart_items: 0,
        forest_nodes: 0,
        roots: 0,
        edges: 0,
        work: 0,
    };
    assert_eq!(
        unsafe { lling_cfg_analysis_info(analysis, &mut info) },
        LlingLlangStatus::Ok
    );
    assert_eq!(info.accepted, 1);
    assert_eq!(info.roots, 1);
    assert_eq!(info.edges, 1);
    assert!(info.chart_items >= 2);
    assert!(info.work > 0);

    let mut rows = [LlingCfgChartItem {
        position: 0,
        rule: 0,
        dot: 0,
        start: 0,
    }; 8];
    let mut written = usize::MAX;
    assert_eq!(
        unsafe {
            lling_cfg_analysis_chart_page(analysis, 0, rows.as_mut_ptr(), rows.len(), &mut written)
        },
        LlingLlangStatus::Ok
    );
    assert_eq!(written as u64, info.chart_items);
    assert!(rows[..written].windows(2).all(|pair| (
        pair[0].position,
        pair[0].rule,
        pair[0].dot,
        pair[0].start
    ) <= (
        pair[1].position,
        pair[1].rule,
        pair[1].dot,
        pair[1].start
    )));

    let mut roots = [u32::MAX; 2];
    assert_eq!(
        unsafe { lling_cfg_analysis_root_page(analysis, 0, roots.as_mut_ptr(), 2, &mut written) },
        LlingLlangStatus::Ok
    );
    assert_eq!(written, 1);
    let mut edge_labels = [0u32; 2];
    assert_eq!(
        unsafe {
            lling_cfg_analysis_edge_page(analysis, 0, edge_labels.as_mut_ptr(), 2, &mut written)
        },
        LlingLlangStatus::Ok
    );
    assert_eq!((written, edge_labels[0]), (1, 7));
    let mut node = LlingCfgForestNodeInfo {
        rule: u32::MAX,
        start: u32::MAX,
        end: u32::MAX,
        is_root: 0,
        reserved: [9; 3],
        children: 0,
    };
    assert_eq!(
        unsafe { lling_cfg_analysis_forest_node(analysis, roots[0], &mut node) },
        LlingLlangStatus::Ok
    );
    assert_eq!(
        (node.rule, node.start, node.end, node.is_root),
        (0, 0, 1, 1)
    );
    assert_eq!(node.children, 1);
    let mut child = LlingCfgForestChildInfo {
        kind: 0,
        edge_id: u32::MAX,
        members: u64::MAX,
    };
    assert_eq!(
        unsafe { lling_cfg_analysis_forest_child(analysis, roots[0], 0, &mut child) },
        LlingLlangStatus::Ok
    );
    assert_eq!(child.kind, LLING_CFG_CHILD_TERMINAL);
    assert_eq!(child.edge_id, 0);
    unsafe { lling_cfg_analysis_free(analysis) };
}

#[test]
fn rejected_and_limited_parses_have_distinct_transactional_results() {
    let rhs = [LlingCfgSymbol {
        kind: LLING_CFG_TERMINAL,
        value: 7,
    }];
    let rules = [LlingCfgRule {
        lhs: 0,
        rhs: rhs.as_ptr(),
        rhs_len: 1,
        log_prob: 0.0,
    }];
    let grammar = unsafe { grammar(&rules, 1) };
    let mut result = ptr::null_mut();
    let mut config = limits();
    assert_eq!(
        unsafe { lling_cfg_parse_tokens(grammar, [8u32].as_ptr(), 1, &config, &mut result) },
        LlingLlangStatus::Ok
    );
    let mut info = LlingCfgAnalysisInfo {
        accepted: 9,
        reserved: [9; 7],
        chart_items: 0,
        forest_nodes: 0,
        roots: 0,
        edges: 0,
        work: 0,
    };
    assert_eq!(
        unsafe { lling_cfg_analysis_info(result, &mut info) },
        LlingLlangStatus::Ok
    );
    assert_eq!((info.accepted, info.roots), (0, 0));
    unsafe { lling_cfg_analysis_free(result) };

    config.max_work = 1;
    result = ptr::dangling_mut::<LlingCfgAnalysis>();
    assert_eq!(
        unsafe { lling_cfg_parse_tokens(grammar, [7u32].as_ptr(), 1, &config, &mut result) },
        LlingLlangStatus::LimitExceeded
    );
    assert!(result.is_null());
    unsafe { lling_cfg_grammar_free(grammar) };
}

#[test]
fn malformed_grammar_is_rejected_before_publishing_handle() {
    let bad_rhs = [LlingCfgSymbol {
        kind: 999,
        value: 0,
    }];
    let rule = LlingCfgRule {
        lhs: 0,
        rhs: bad_rhs.as_ptr(),
        rhs_len: 1,
        log_prob: 0.0,
    };
    let mut output = ptr::dangling_mut::<LlingCfgGrammar>();
    assert_eq!(
        unsafe { lling_cfg_grammar_compile(0, 1, &rule, 1, 8, 8, &mut output) },
        LlingLlangStatus::InvalidArgument
    );
    assert!(output.is_null());
}

#[test]
fn ffi_token_analysis_matches_native_earley_for_ambiguity_epsilon_and_rejection() {
    let native = Grammar::new(
        NonTerminal::new(0),
        vec![
            Production::new(
                RuleId::new(0),
                NonTerminal::new(0),
                smallvec![Symbol::non_terminal(1)],
            ),
            Production::new(
                RuleId::new(1),
                NonTerminal::new(0),
                smallvec![Symbol::non_terminal(2)],
            ),
            Production::new(RuleId::new(2), NonTerminal::new(0), smallvec![]),
            Production::new(
                RuleId::new(3),
                NonTerminal::new(1),
                smallvec![Symbol::terminal(0)],
            ),
            Production::new(
                RuleId::new(4),
                NonTerminal::new(2),
                smallvec![Symbol::terminal(0)],
            ),
        ],
        3,
    )
    .unwrap();
    let rhs_0 = [LlingCfgSymbol {
        kind: LLING_CFG_NONTERMINAL,
        value: 1,
    }];
    let rhs_1 = [LlingCfgSymbol {
        kind: LLING_CFG_NONTERMINAL,
        value: 2,
    }];
    let rhs_3 = [LlingCfgSymbol {
        kind: LLING_CFG_TERMINAL,
        value: 0,
    }];
    let rules = [
        LlingCfgRule {
            lhs: 0,
            rhs: rhs_0.as_ptr(),
            rhs_len: 1,
            log_prob: 0.0,
        },
        LlingCfgRule {
            lhs: 0,
            rhs: rhs_1.as_ptr(),
            rhs_len: 1,
            log_prob: 0.0,
        },
        LlingCfgRule {
            lhs: 0,
            rhs: ptr::null(),
            rhs_len: 0,
            log_prob: 0.0,
        },
        LlingCfgRule {
            lhs: 1,
            rhs: rhs_3.as_ptr(),
            rhs_len: 1,
            log_prob: 0.0,
        },
        LlingCfgRule {
            lhs: 2,
            rhs: rhs_3.as_ptr(),
            rhs_len: 1,
            log_prob: 0.0,
        },
    ];
    let mut compiled = ptr::null_mut();
    assert_eq!(
        unsafe {
            lling_cfg_grammar_compile(0, 3, rules.as_ptr(), rules.len(), 5, 4, &mut compiled)
        },
        LlingLlangStatus::Ok
    );
    for tokens in [&[][..], &[0][..], &[1][..], &[0, 0][..]] {
        let mut builder = LatticeBuilder::<TropicalWeight, _>::with_capacity(
            HashMapBackend::new(),
            tokens.len(),
            1,
        );
        for (index, &token) in tokens.iter().enumerate() {
            builder.add_correction_by_id(
                index,
                index + 1,
                token,
                TropicalWeight::new(0.0),
                EdgeMetadata::default(),
            );
        }
        let lattice = builder.build(tokens.len());
        let expected = EarleyParser::new(&native)
            .analyze_lattice_with_limits(
                &lattice,
                ParseLimits {
                    max_chart_items: 128,
                    max_forest_nodes: 128,
                    max_work: 1024,
                },
            )
            .unwrap();
        let mut analysis = ptr::null_mut();
        assert_eq!(
            unsafe {
                lling_cfg_parse_tokens(
                    compiled,
                    tokens.as_ptr(),
                    tokens.len(),
                    &limits(),
                    &mut analysis,
                )
            },
            LlingLlangStatus::Ok
        );
        let mut actual = LlingCfgAnalysisInfo {
            accepted: 0,
            reserved: [0; 7],
            chart_items: 0,
            forest_nodes: 0,
            roots: 0,
            edges: 0,
            work: 0,
        };
        assert_eq!(
            unsafe { lling_cfg_analysis_info(analysis, &mut actual) },
            LlingLlangStatus::Ok
        );
        assert_eq!(actual.accepted != 0, !expected.forest.is_empty());
        assert_eq!(actual.chart_items, expected.chart.len() as u64);
        assert_eq!(actual.forest_nodes, expected.forest.num_nodes() as u64);
        assert_eq!(actual.roots, expected.forest.roots().count() as u64);
        assert_eq!(actual.edges, tokens.len() as u64);
        assert_eq!(actual.work, expected.work);
        unsafe { lling_cfg_analysis_free(analysis) };
    }
    unsafe { lling_cfg_grammar_free(compiled) };
}
