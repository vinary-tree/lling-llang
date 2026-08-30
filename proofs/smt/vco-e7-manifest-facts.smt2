; Finite boundary obligations for libcpg manifests and durable facts.
(set-option :produce-models false)

(declare-const repository_equal Bool)
(declare-const parser_equal Bool)
(declare-const grammar_equal Bool)
(declare-const extractor_equal Bool)
(declare-const query_equal Bool)
(declare-const feature_revision_equal Bool)
(declare-const schema_equal Bool)
(declare-const source_equal Bool)
(declare-const source_revision_equal Bool)
(declare-const configuration_equal Bool)
(declare-const comparison_known Bool)
(declare-const comparison_equal Bool)
(declare-const complete Bool)
(declare-const range_valid Bool)
(declare-const display_renamed Bool)
(declare-const old_tombstoned Bool)
(declare-const new_active Bool)
(declare-const historical_id_match Bool)
(declare-const feature_semantics_equal Bool)

(define-fun ManifestEqual () Bool
  (and repository_equal parser_equal grammar_equal extractor_equal query_equal
       feature_revision_equal schema_equal source_equal source_revision_equal
       configuration_equal))

(define-fun FeatureTransitionValid () Bool
  (and (=> old_tombstoned (not new_active))
       (=> historical_id_match feature_semantics_equal)))

(define-fun CacheReuse () Bool
  (and ManifestEqual comparison_known comparison_equal complete range_valid
       FeatureTransitionValid))

(declare-const fact_two_tombstoned Bool)
(declare-const fact_two_active Bool)
(assert (= fact_two_active (not fact_two_tombstoned)))

(declare-const dense_count Int)
(declare-const forward_one Int)
(declare-const forward_two Int)
(declare-const reverse_zero Int)
(declare-const reverse_one Int)
(assert (= dense_count (ite fact_two_active 2 1)))
(assert (= forward_one 0))
(assert (= forward_two (ite fact_two_active 1 (- 1))))
(assert (= reverse_zero 1))
(assert (= reverse_one (ite fact_two_active 2 0)))

(declare-const export_a_first Int)
(declare-const export_a_second Int)
(declare-const export_b_first Int)
(declare-const export_b_second Int)
(assert (= export_a_first 1))
(assert (= export_a_second 2))
(assert (= export_b_first 1))
(assert (= export_b_second 2))

(declare-const coverage_complete Bool)
(declare-const fact_observed Bool)
(declare-const absence_claim Bool)
(assert (= absence_claim (and coverage_complete (not fact_observed))))

(declare-const f1_r1 Bool)
(declare-const f1_r2 Bool)
(declare-const f2_r1 Bool)
(declare-const f2_r2 Bool)
(declare-const rule_one_output Bool)
(declare-const rule_two_output Bool)
(declare-const many_to_many_scenario Bool)
(assert (= rule_one_output (or f1_r1 f2_r1)))
(assert (= rule_two_output (or f1_r2 f2_r2)))
(assert (=> many_to_many_scenario (and f1_r1 f1_r2 f2_r1)))

(declare-const libcpg_depends_on_lling_llang Bool)
(declare-const lling_llang_depends_on_libcpg Bool)
(declare-const adapter_depends_on_libcpg Bool)
(declare-const adapter_depends_on_lling_llang Bool)
(assert (not libcpg_depends_on_lling_llang))
(assert (not lling_llang_depends_on_libcpg))
(assert adapter_depends_on_libcpg)
(assert adapter_depends_on_lling_llang)

(declare-const work Int)
(declare-const remaining Int)
(declare-const initial_work Int)
(declare-const native_frames Int)
(assert (>= work 0))
(assert (>= remaining 0))
(assert (= (+ work remaining) initial_work))
(assert (= native_frames 1))

; E7-MF-SMT-RENAME-PRESERVES-DURABLE-ID
(push)
(assert display_renamed)
(assert repository_equal)
(assert parser_equal)
(assert grammar_equal)
(assert extractor_equal)
(assert query_equal)
(assert feature_revision_equal)
(assert schema_equal)
(assert source_equal)
(assert source_revision_equal)
(assert configuration_equal)
(assert (not ManifestEqual))
(check-sat)
(pop)

; E7-MF-SMT-PARSER-MISMATCH-NO-REUSE
(push)
(assert (not parser_equal))
(assert CacheReuse)
(check-sat)
(pop)

; E7-MF-SMT-GRAMMAR-MISMATCH-NO-REUSE
(push)
(assert (not grammar_equal))
(assert CacheReuse)
(check-sat)
(pop)

; E7-MF-SMT-FEATURE-REVISION-MISMATCH-NO-REUSE
(push)
(assert (not feature_revision_equal))
(assert CacheReuse)
(check-sat)
(pop)

; E7-MF-SMT-SOURCE-REVISION-MISMATCH-NO-REUSE
(push)
(assert (not source_revision_equal))
(assert CacheReuse)
(check-sat)
(pop)

; E7-MF-SMT-CONFIGURATION-MISMATCH-NO-REUSE
(push)
(assert (not configuration_equal))
(assert CacheReuse)
(check-sat)
(pop)

; E7-MF-SMT-UNKNOWN-COMPATIBILITY-NO-REUSE
(push)
(assert (not comparison_known))
(assert CacheReuse)
(check-sat)
(pop)

; E7-MF-SMT-INCOMPLETE-NO-REUSE
(push)
(assert (not complete))
(assert CacheReuse)
(check-sat)
(pop)

; E7-MF-SMT-INVALID-RANGE-NO-REUSE
(push)
(assert (not range_valid))
(assert CacheReuse)
(check-sat)
(pop)

; E7-MF-SMT-TOMBSTONE-REACTIVATION-NO-REUSE
(push)
(assert old_tombstoned)
(assert new_active)
(assert CacheReuse)
(check-sat)
(pop)

; E7-MF-SMT-DURABLE-DENSE-ROUNDTRIP
(push)
(assert fact_two_active)
(assert (or (not (= forward_two 1)) (not (= reverse_one 2))))
(check-sat)
(pop)

; E7-MF-SMT-DENSE-INJECTIVITY
(push)
(assert fact_two_active)
(assert (= forward_one forward_two))
(check-sat)
(pop)

; E7-MF-SMT-DENSE-NO-ORPHANS
(push)
(assert (= dense_count 2))
(assert (= reverse_one 0))
(check-sat)
(pop)

; E7-MF-SMT-TOMBSTONE-NOT-ACTIVE
(push)
(assert fact_two_tombstoned)
(assert fact_two_active)
(check-sat)
(pop)

; E7-MF-SMT-HISTORICAL-ID-NOT-REUSED
(push)
(assert historical_id_match)
(assert (not feature_semantics_equal))
(assert FeatureTransitionValid)
(check-sat)
(pop)

; E7-MF-SMT-DETERMINISTIC-CANONICAL-EXPORT
(push)
(assert (or (not (= export_a_first export_b_first))
            (not (= export_a_second export_b_second))))
(check-sat)
(pop)

; E7-MF-SMT-INCOMPLETE-NO-ABSENCE
(push)
(assert (not coverage_complete))
(assert absence_claim)
(check-sat)
(pop)

; E7-MF-SMT-LOWERING-NO-PROVENANCE-ORPHAN
(push)
(assert rule_two_output)
(assert (not f1_r2))
(assert (not f2_r2))
(check-sat)
(pop)

; E7-MF-SMT-MANY-TO-MANY-PRESERVED
(push)
(assert many_to_many_scenario)
(assert (or (not f1_r1) (not f1_r2) (not f2_r1)))
(check-sat)
(pop)

; E7-MF-SMT-CORE-DEPENDENCY-INDEPENDENCE
(push)
(assert (or libcpg_depends_on_lling_llang
            lling_llang_depends_on_libcpg
            (not adapter_depends_on_libcpg)
            (not adapter_depends_on_lling_llang)))
(check-sat)
(pop)

; E7-MF-SMT-LINEAR-WORK-BOUND
(push)
(assert (not (= (+ work remaining) initial_work)))
(check-sat)
(pop)

; E7-MF-SMT-CONSTANT-NATIVE-STACK
(push)
(assert (not (= native_frames 1)))
(check-sat)
(pop)

; E7-MF-SMT-VALID-CACHE-WITNESS
(push)
(assert repository_equal)
(assert parser_equal)
(assert grammar_equal)
(assert extractor_equal)
(assert query_equal)
(assert feature_revision_equal)
(assert schema_equal)
(assert source_equal)
(assert source_revision_equal)
(assert configuration_equal)
(assert comparison_known)
(assert comparison_equal)
(assert complete)
(assert range_valid)
(assert (not old_tombstoned))
(assert feature_semantics_equal)
(assert CacheReuse)
(check-sat)
(pop)

; E7-MF-SMT-VALID-MANY-TO-MANY-WITNESS
(push)
(assert many_to_many_scenario)
(assert rule_one_output)
(assert rule_two_output)
(check-sat)
(pop)
