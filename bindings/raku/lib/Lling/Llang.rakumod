unit module Lling::Llang;

use NativeCall;
need Lling::Llang::GeneratedAbi;
need Vinary::Tree::Interop;

our constant ABI-VERSION is export = Lling::Llang::GeneratedAbi::ABI-VERSION;
our constant API-REVISION is export = Lling::Llang::GeneratedAbi::API-REVISION;
our constant TYPED-ABI-VERSION is export =
    Lling::Llang::GeneratedAbi::TYPED-ABI-VERSION;
our constant DESCRIPTOR-SIGNATURE-KNOWN is export =
    Lling::Llang::GeneratedAbi::DESCRIPTOR-SIGNATURE-KNOWN;
our constant DESCRIPTOR-SNAPSHOT-PRESENT is export =
    Lling::Llang::GeneratedAbi::DESCRIPTOR-SNAPSHOT-PRESENT;
our constant DESCRIPTOR-CONTEXT-PRESENT is export =
    Lling::Llang::GeneratedAbi::DESCRIPTOR-CONTEXT-PRESENT;
our constant BUDGET-STATES is export = Lling::Llang::GeneratedAbi::BUDGET-STATES;
our constant BUDGET-ARCS is export = Lling::Llang::GeneratedAbi::BUDGET-ARCS;
our constant BUDGET-BYTES is export = Lling::Llang::GeneratedAbi::BUDGET-BYTES;
our constant BUDGET-WORK is export = Lling::Llang::GeneratedAbi::BUDGET-WORK;
our enum CancellationReasonV2 is export (
    REQUESTED => 1,
    DEADLINE => 2,
    BUDGET => 3,
    SOURCE => 4,
);
our constant Status is export = Lling::Llang::GeneratedAbi::Status;
our constant OK is export = Lling::Llang::GeneratedAbi::OK;
our constant INVALID-ARGUMENT is export = Lling::Llang::GeneratedAbi::INVALID-ARGUMENT;
our constant NULL-POINTER is export = Lling::Llang::GeneratedAbi::NULL-POINTER;
our constant PANIC is export = Lling::Llang::GeneratedAbi::PANIC;
our constant INCOMPATIBLE-RESOURCE is export =
    Lling::Llang::GeneratedAbi::INCOMPATIBLE-RESOURCE;
our constant PROVIDER-ERROR is export = Lling::Llang::GeneratedAbi::PROVIDER-ERROR;
our constant LIMIT-EXCEEDED is export = Lling::Llang::GeneratedAbi::LIMIT-EXCEEDED;
our constant CLOSED is export = Lling::Llang::GeneratedAbi::CLOSED;

module InteropAccess {
    use Vinary::Tree::Interop;

    our constant ResourceType = Resource;
    our constant WfstType = Wfst;
    our constant RawResourceType = RawResource;
    our constant WfstArcType = WfstArc;
    our constant UnitDomainType = UnitDomain;
    our constant WeightDomainType = WeightDomain;
    our constant InterfaceIdType = InterfaceId;
    our constant SemiringValueType = SemiringValue;
    our constant UnicodeDomain = UNICODE-SCALAR;
    our constant TropicalDomain = TROPICAL-F64;
    our constant ImmutableFlag = WFST-FLAG-IMMUTABLE;
    our constant LazyFlag = WFST-FLAG-LAZY;
    our constant ParallelFlag = WFST-FLAG-PARALLEL-REENTRANT;
    our constant AcyclicFlag = WFST-FLAG-ACYCLIC;
    our constant SemiringThreadBound = SEMIRING-FLAG-THREAD-BOUND;
    our constant SemiringParallel = SEMIRING-FLAG-PARALLEL-REENTRANT;
    our constant SemiringStable = SEMIRING-FLAG-STABLE-BYTES;
    our constant SemiringBatch = SEMIRING-FLAG-BATCH;

    our sub adopt(RawResource:D $raw --> Resource:D) { adopt-resource($raw) }
    our sub wrap(Resource:D $resource --> Wfst:D) { wfst($resource, :take) }
    our sub domain-id(Str:D $name --> InterfaceId:D) { interface-id($name) }
}

our constant UnitDomain is export = InteropAccess::UnitDomainType;
our constant WeightDomain is export = InteropAccess::WeightDomainType;

class X::Lling::Llang is Exception is export {
    has Status:D $.status is required;
    has Str:D $.operation is required;
    has Str:D $.detail = '';

    method message(--> Str:D) {
        my $base = "lling-llang operation '$!operation' failed with $!status";
        $!detail.chars ?? "$base: $!detail" !! $base
    }
}

sub native-library(--> Str:D) {
    return %*ENV<LLING_LLANG_LIBRARY> if %*ENV<LLING_LLANG_LIBRARY>:exists;
    $*DISTRO.is-win ?? 'lling_llang.dll' !!
        $*KERNEL.name eq 'darwin' ?? 'liblling_llang.dylib' !!
        'liblling_llang.so'
}

sub provider-library(--> Str:D) {
    return %*ENV<LLING_LLANG_RAKU_PROVIDER_LIB>
        if %*ENV<LLING_LLANG_RAKU_PROVIDER_LIB>:exists;
    %?RESOURCES<libraries/lling_llang_raku_provider>.IO.Str
}

sub lling-abi-version(--> uint32)
    is native(&native-library) is symbol('lling_abi_version') { * }
sub lling-api-revision(--> uint32)
    is native(&native-library) is symbol('lling_llang_api_revision') { * }
sub lling-last-error-message(--> Str)
    is native(&native-library) is symbol('lling_last_error_message') { * }
sub lling-abi-v2-validate-header(Pointer, uint32, uint64 --> uint32)
    is native(&native-library) is symbol('lling_abi_v2_validate_header') { * }
sub lling-abi-v2-validate-descriptor(Pointer, uint8 is rw --> uint32)
    is native(&native-library) is symbol('lling_abi_v2_validate_descriptor') { * }
sub lling-abi-v2-validate-budget(Pointer --> uint32)
    is native(&native-library) is symbol('lling_abi_v2_validate_budget') { * }
sub lling-abi-v2-validate-outcome(Pointer, uint8, uint8, uint8 is rw --> uint32)
    is native(&native-library) is symbol('lling_abi_v2_validate_outcome') { * }
sub lling-abi-v2-identity-matches(Pointer, Pointer, uint8 is rw --> uint32)
    is native(&native-library) is symbol('lling_abi_v2_identity_matches') { * }
sub lling-cancellation-v2-new(Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_cancellation_v2_new') { * }
sub lling-cancellation-v2-request(Pointer, uint32 --> uint32)
    is native(&native-library) is symbol('lling_cancellation_v2_request') { * }
sub lling-cancellation-v2-reason(Pointer, uint32 is rw --> uint32)
    is native(&native-library) is symbol('lling_cancellation_v2_reason') { * }
sub lling-cancellation-v2-free(Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_cancellation_v2_free') { * }
sub lling-wfst-builder-new(Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_wfst_builder_new') { * }
sub lling-wfst-builder-free(Pointer)
    is native(&native-library) is symbol('lling_wfst_builder_free') { * }
sub lling-wfst-builder-reserve-states(Pointer, size_t --> uint32)
    is native(&native-library) is symbol('lling_wfst_builder_reserve_states') { * }
sub lling-wfst-builder-add-state(Pointer, uint32 is rw --> uint32)
    is native(&native-library) is symbol('lling_wfst_builder_add_state') { * }
sub lling-wfst-builder-set-start(Pointer, uint32 --> uint32)
    is native(&native-library) is symbol('lling_wfst_builder_set_start') { * }
sub lling-wfst-builder-set-final(Pointer, uint32, num64 --> uint32)
    is native(&native-library) is symbol('lling_wfst_builder_set_final') { * }
sub lling-wfst-builder-clear-final(Pointer, uint32 --> uint32)
    is native(&native-library) is symbol('lling_wfst_builder_clear_final') { * }
sub lling-wfst-builder-add-arc(
    Pointer, uint32, uint64, uint8, uint64, uint8, uint32, num64 --> uint32
) is native(&native-library) is symbol('lling_wfst_builder_add_arc') { * }
sub lling-wfst-builder-build(Pointer, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_wfst_builder_build') { * }
sub lling-wfst-free(Pointer)
    is native(&native-library) is symbol('lling_wfst_free') { * }
sub lling-wfst-import-ref(InteropAccess::RawResourceType, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_wfst_import_ref') { * }
sub lling-wfst-compose-refs(
    InteropAccess::RawResourceType,
    InteropAccess::RawResourceType,
    Pointer is rw,
    --> uint32
) is native(&native-library) is symbol('lling_wfst_compose_refs') { * }
sub lling-wfst-resource(Pointer, InteropAccess::RawResourceType --> uint32)
    is native(&native-library) is symbol('lling_wfst_resource') { * }
sub lling-semiring-open(InteropAccess::RawResourceType, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_open') { * }
sub lling-semiring-free(Pointer)
    is native(&native-library) is symbol('lling_semiring_free') { * }
sub lling-semiring-weight-free(Pointer)
    is native(&native-library) is symbol('lling_semiring_weight_free') { * }
sub lling-semiring-properties(Pointer, uint64 is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_properties') { * }
sub lling-semiring-zero(Pointer, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_zero') { * }
sub lling-semiring-one(Pointer, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_one') { * }
sub lling-semiring-weight-clone(Pointer, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_weight_clone') { * }
sub lling-semiring-plus(Pointer, Pointer, Pointer, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_plus') { * }
sub lling-semiring-times(Pointer, Pointer, Pointer, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_times') { * }
sub lling-semiring-equal(Pointer, Pointer, Pointer, uint8 is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_equal') { * }
sub lling-semiring-approx-equal(
    Pointer, Pointer, Pointer, num64, uint8 is rw --> uint32
) is native(&native-library) is symbol('lling_semiring_approx_equal') { * }
sub lling-semiring-natural-order(
    Pointer, Pointer, Pointer, int32 is rw --> uint32
) is native(&native-library) is symbol('lling_semiring_natural_order') { * }
sub lling-semiring-divide(
    Pointer, Pointer, Pointer, Pointer is rw, uint8 is rw --> uint32
) is native(&native-library) is symbol('lling_semiring_divide') { * }
sub lling-semiring-left-divide(
    Pointer, Pointer, Pointer, Pointer is rw, uint8 is rw --> uint32
) is native(&native-library) is symbol('lling_semiring_left_divide') { * }
sub lling-semiring-star(
    Pointer, Pointer, Pointer is rw, uint8 is rw --> uint32
) is native(&native-library) is symbol('lling_semiring_star') { * }
sub lling-semiring-numerical-value(Pointer, Pointer, num64 is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_numerical_value') { * }
sub lling-semiring-quantize(Pointer, Pointer, num64, int64 is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_quantize') { * }
sub lling-semiring-to-probability(Pointer, Pointer, num64 is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_to_probability') { * }
sub lling-semiring-closure-bound(Pointer, size_t is rw, uint8 is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_closure_bound') { * }
sub lling-semiring-stable-bytes(
    Pointer, Pointer, Pointer, size_t, size_t is rw, size_t is rw --> uint32
) is native(&native-library) is symbol('lling_semiring_stable_bytes') { * }
sub lling-semiring-diagnostic(
    Pointer, Pointer, Pointer, size_t, size_t is rw, size_t is rw --> uint32
) is native(&native-library) is symbol('lling_semiring_diagnostic') { * }
sub lling-semiring-plus-many(Pointer, Pointer, size_t, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_plus_many') { * }
sub lling-semiring-times-many(Pointer, Pointer, size_t, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_semiring_times_many') { * }
sub lling-semiring-validate-laws(
    Pointer, Pointer, size_t, num64 --> uint32
) is native(&native-library) is symbol('lling_semiring_validate_laws') { * }
sub lling-lattice-open(InteropAccess::RawResourceType, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_lattice_open') { * }
sub lling-lattice-free(Pointer)
    is native(&native-library) is symbol('lling_lattice_free') { * }
sub lling-lattice-domain-id(
    Pointer, InteropAccess::InterfaceIdType --> uint32
) is native(&native-library) is symbol('lling_lattice_domain_id') { * }
sub lling-lattice-flags(Pointer, uint64 is rw --> uint32)
    is native(&native-library) is symbol('lling_lattice_flags') { * }
sub lling-lattice-join(Pointer, Pointer, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_lattice_join') { * }
sub lling-lattice-meet(Pointer, Pointer, Pointer is rw --> uint32)
    is native(&native-library) is symbol('lling_lattice_meet') { * }
sub lling-lattice-equal(Pointer, Pointer, uint8 is rw --> uint32)
    is native(&native-library) is symbol('lling_lattice_equal') { * }
sub lling-lattice-stable-bytes(
    Pointer, Pointer, size_t, size_t is rw, size_t is rw --> uint32
) is native(&native-library) is symbol('lling_lattice_stable_bytes') { * }
sub lling-lattice-diagnostic(
    Pointer, Pointer, size_t, size_t is rw, size_t is rw --> uint32
) is native(&native-library) is symbol('lling_lattice_diagnostic') { * }
sub lling-lattice-join-many(
    Pointer, Pointer, size_t, Pointer is rw --> uint32
) is native(&native-library) is symbol('lling_lattice_join_many') { * }
sub lling-lattice-meet-many(
    Pointer, Pointer, size_t, Pointer is rw --> uint32
) is native(&native-library) is symbol('lling_lattice_meet_many') { * }
sub lling-lattice-validate-laws(Pointer, size_t --> uint32)
    is native(&native-library) is symbol('lling_lattice_validate_laws') { * }

sub abi-version(--> UInt:D) is export { lling-abi-version() }
sub api-revision(--> UInt:D) is export { lling-api-revision() }

sub check-status(Int:D $code, Str:D $operation --> Nil) {
    my $status = Status($code);
    return if $status == OK;
    X::Lling::Llang.new(
        :$status,
        :$operation,
        detail => (try lling-last-error-message()) // '',
    ).throw;
}

class AbiV2Header is repr('CStruct') is export {
    has uint32 $.struct-size is rw;
    has uint32 $.abi-version is rw;
    has uint64 $.flags is rw;
    has uint64 $.reserved is rw;
    multi method new(UInt:D :$struct-size!, UInt:D :$flags = 0 --> AbiV2Header:D) {
        self.bless(:$struct-size, abi-version => TYPED-ABI-VERSION,
            :$flags, reserved => 0)
    }
}

class BudgetV2 is repr('CStruct') is export {
    HAS AbiV2Header $.header is rw;
    has uint64 $.max-states is rw;
    has uint64 $.max-arcs is rw;
    has uint64 $.max-bytes is rw;
    has uint64 $.max-work is rw;
    has uint64 $.reserved0 is rw;
    has uint64 $.reserved1 is rw;
    multi method new(
        UInt:D :$max-states = 0, UInt:D :$max-arcs = 0,
        UInt:D :$max-bytes = 0, UInt:D :$max-work = 0,
        --> BudgetV2:D
    ) {
        my $flags = ($max-states ?? BUDGET-STATES !! 0) +|
            ($max-arcs ?? BUDGET-ARCS !! 0) +|
            ($max-bytes ?? BUDGET-BYTES !! 0) +|
            ($max-work ?? BUDGET-WORK !! 0);
        my $value = self.bless;
        $value.header.struct-size = nativesizeof(BudgetV2);
        $value.header.abi-version = TYPED-ABI-VERSION;
        $value.header.flags = $flags;
        $value.header.reserved = 0;
        $value.max-states = $max-states;
        $value.max-arcs = $max-arcs;
        $value.max-bytes = $max-bytes;
        $value.max-work = $max-work;
        $value.reserved0 = 0;
        $value.reserved1 = 0;
        $value
    }
}

class OutcomeV2 is repr('CStruct') is export {
    HAS AbiV2Header $.header is rw;
    has uint32 $.precision is rw;
    has uint32 $.completeness is rw;
    has uint32 $.applicability is rw;
    has uint32 $.termination is rw;
    has uint32 $.evidence is rw;
    has uint32 $.reserved0 is rw;
    has uint64 $.states is rw;
    has uint64 $.arcs is rw;
    has uint64 $.bytes is rw;
    has uint64 $.work is rw;
    has uint64 $.limitations is rw;
    has uint64 $.reserved1 is rw;
    multi method new(
        UInt:D :$precision!, UInt:D :$completeness!,
        UInt:D :$applicability!, UInt:D :$termination!,
        UInt:D :$evidence!, UInt:D :$states = 0, UInt:D :$arcs = 0,
        UInt:D :$bytes = 0, UInt:D :$work = 0,
        UInt:D :$limitations = 0, --> OutcomeV2:D
    ) {
        my $value = self.bless;
        $value.header.struct-size = nativesizeof(OutcomeV2);
        $value.header.abi-version = TYPED-ABI-VERSION;
        $value.header.flags = 0;
        $value.header.reserved = 0;
        $value.precision = $precision;
        $value.completeness = $completeness;
        $value.applicability = $applicability;
        $value.termination = $termination;
        $value.evidence = $evidence;
        $value.reserved0 = 0;
        $value.states = $states;
        $value.arcs = $arcs;
        $value.bytes = $bytes;
        $value.work = $work;
        $value.limitations = $limitations;
        $value.reserved1 = 0;
        $value
    }
}

sub validate-abi-v2-header(
    AbiV2Header:D $header, UInt:D $required-size, UInt:D $known-flags = 0,
    --> AbiV2Header:D
) is export {
    check-status(lling-abi-v2-validate-header(
        nativecast(Pointer, $header), $required-size, $known-flags),
        'abi-v2-validate-header');
    $header
}
sub validate-budget-v2(BudgetV2:D $budget --> BudgetV2:D) is export {
    check-status(lling-abi-v2-validate-budget(nativecast(Pointer, $budget)),
        'abi-v2-validate-budget');
    $budget
}
sub authoritative-exact(
    OutcomeV2:D $outcome, Bool:D :$resource-present!,
    Bool:D :$evidence-present!, --> Bool:D
) is export {
    my uint8 $result = 0;
    check-status(lling-abi-v2-validate-outcome(
        nativecast(Pointer, $outcome), $resource-present ?? 1 !! 0,
        $evidence-present ?? 1 !! 0, $result), 'abi-v2-validate-outcome');
    $result.so
}
sub typed-evidence-allowed(Pointer:D $descriptor --> Bool:D) is export {
    my uint8 $result = 0;
    check-status(lling-abi-v2-validate-descriptor($descriptor, $result),
        'abi-v2-validate-descriptor');
    $result.so
}
sub identity-matches(
    Pointer:D $expected, Pointer:D $observed --> Bool:D
) is export {
    my uint8 $result = 0;
    check-status(lling-abi-v2-identity-matches($expected, $observed, $result),
        'abi-v2-identity-matches');
    $result.so
}

class CancellationV2 is export {
    has Pointer $!handle is required;
    has Bool $!closed = False;
    submethod BUILD(Pointer:D :$handle!) { $!handle = $handle }
    multi method new(--> CancellationV2:D) {
        my Pointer $handle .= new;
        check-status(lling-cancellation-v2-new($handle), 'cancellation-v2-new');
        self.bless(:$handle)
    }
    method !handle(--> Pointer) {
        X::Lling::Llang.new(status => CLOSED, operation => 'cancellation',
            detail => 'cancellation handle is closed').throw if $!closed;
        $!handle
    }
    method request(CancellationReasonV2:D $reason --> CancellationV2:D) {
        check-status(lling-cancellation-v2-request(self!handle, $reason.Int),
            'cancellation-v2-request');
        self
    }
    method reason(--> CancellationReasonV2) {
        my uint32 $reason = 0;
        check-status(lling-cancellation-v2-reason(self!handle, $reason),
            'cancellation-v2-reason');
        $reason ?? CancellationReasonV2($reason) !! CancellationReasonV2
    }
    method close(--> Nil) {
        return if $!closed;
        check-status(lling-cancellation-v2-free($!handle),
            'cancellation-v2-free');
        $!closed = True;
    }
    submethod DESTROY { try self.close }
}

sub valid-weight(Real:D $weight --> Num:D) {
    my $value = $weight.Num;
    die 'tropical weights must be finite or +Inf'
        if $value != $value || $value == -Inf;
    $value
}

sub wire-label(Mu $label --> List:D) {
    return 0, 0 unless $label.defined;
    my $value = $label ~~ Str
        ?? ($label.chars == 1 ?? $label.ord
            !! die 'a WFST label is one Unicode scalar')
        !! $label.Int;
    die 'a WFST label must be a Unicode scalar'
        if $value < 0 || $value > 0x10ffff || $value ~~ 0xd800..0xdfff;
    $value, 1
}

class WfstBuilder is export {
    has Pointer $!handle is required;
    has Bool $!closed = False;

    submethod BUILD(Pointer:D :$handle!) { $!handle = $handle }

    multi method new(UInt:D :$size-hint = 0 --> WfstBuilder:D) {
        my Pointer $handle .= new;
        check-status(lling-wfst-builder-new($handle), 'wfst-builder-new');
        my $builder = self.bless(:$handle);
        $builder.reserve-states($size-hint) if $size-hint;
        $builder
    }

    method !handle(--> Pointer) {
        X::Lling::Llang.new(
            status => CLOSED,
            operation => 'builder',
            detail => 'builder is closed',
        ).throw if $!closed;
        $!handle
    }

    method reserve-states(UInt:D $additional --> WfstBuilder:D) {
        check-status(lling-wfst-builder-reserve-states(
            self!handle, $additional), 'reserve-states');
        self
    }

    method add-state(--> UInt:D) {
        my uint32 $state = 0;
        check-status(lling-wfst-builder-add-state(self!handle, $state),
            'add-state');
        $state
    }

    method set-start(UInt:D $state --> WfstBuilder:D) {
        check-status(lling-wfst-builder-set-start(self!handle, $state),
            'set-start');
        self
    }

    method set-final(UInt:D $state, Real:D $weight = 0e0 --> WfstBuilder:D) {
        check-status(lling-wfst-builder-set-final(self!handle, $state,
            valid-weight($weight)), 'set-final');
        self
    }

    method clear-final(UInt:D $state --> WfstBuilder:D) {
        check-status(lling-wfst-builder-clear-final(self!handle, $state),
            'clear-final');
        self
    }

    method add-arc(
        UInt:D $from,
        Mu $input,
        Mu $output,
        UInt:D $target,
        Real:D $weight = 0e0,
        --> WfstBuilder:D
    ) {
        my ($input-value, $has-input) = wire-label($input);
        my ($output-value, $has-output) = wire-label($output);
        check-status(lling-wfst-builder-add-arc(self!handle, $from,
            $input-value, $has-input, $output-value, $has-output, $target,
            valid-weight($weight)), 'add-arc');
        self
    }

    method build(--> InteropAccess::WfstType:D) {
        my Pointer $result .= new;
        check-status(lling-wfst-builder-build(self!handle, $result), 'build');
        self.close;
        adopt-native-wfst($result)
    }

    method close(--> Nil) {
        return if $!closed;
        lling-wfst-builder-free($!handle);
        $!closed = True;
        $!handle = Pointer;
    }

    method opened(--> Bool:D) { !$!closed }
    submethod DESTROY { try self.close }
}

sub adopt-native-wfst(Pointer:D $handle --> InteropAccess::WfstType:D) {
    LEAVE lling-wfst-free($handle);
    my $raw = InteropAccess::RawResourceType.new;
    check-status(lling-wfst-resource($handle, $raw), 'wfst-resource');
    InteropAccess::wrap(InteropAccess::adopt($raw))
}

multi sub raw-resource(InteropAccess::ResourceType:D $resource
    --> InteropAccess::RawResourceType:D) {
    $resource.raw
}
multi sub raw-resource(InteropAccess::WfstType:D $wfst
    --> InteropAccess::RawResourceType:D) {
    $wfst.resource.raw
}

sub resource(InteropAccess::WfstType:D $wfst
    --> InteropAccess::ResourceType:D) is export {
    $wfst.resource.retain
}

sub import-wfst(Mu:D $source --> InteropAccess::WfstType:D) is export {
    my Pointer $result .= new;
    check-status(lling-wfst-import-ref(raw-resource($source), $result),
        'wfst-import');
    adopt-native-wfst($result)
}

sub compose(Mu:D $first, Mu:D $second --> InteropAccess::WfstType:D) is export {
    my Pointer $result .= new;
    check-status(lling-wfst-compose-refs(raw-resource($first), raw-resource($second),
        $result), 'wfst-compose');
    adopt-native-wfst($result)
}

# Dynamic-semiring consumer -------------------------------------------------

class DynamicLatticeValue is export {
    has Pointer $!handle is required;
    has Bool $!closed = False;

    submethod BUILD(Pointer:D :$handle!) { $!handle = $handle }

    method native-handle(--> Pointer:D) {
        X::Lling::Llang.new(
            status => CLOSED,
            operation => 'lattice-value',
            detail => 'dynamic lattice value is closed',
        ).throw if $!closed;
        $!handle
    }

    method domain-id(--> InteropAccess::InterfaceIdType:D) {
        my $domain = InteropAccess::InterfaceIdType.new;
        check-status(lling-lattice-domain-id(self.native-handle, $domain),
            'lattice-domain-id');
        $domain
    }

    method flags(--> UInt:D) {
        my uint64 $flags = 0;
        check-status(lling-lattice-flags(self.native-handle, $flags),
            'lattice-flags');
        $flags
    }

    method !binary(
        DynamicLatticeValue:D $other,
        &operation,
        Str:D $name,
        --> DynamicLatticeValue:D
    ) {
        my Pointer $result .= new;
        check-status(operation(self.native-handle, $other.native-handle, $result),
            $name);
        DynamicLatticeValue.new(handle => $result)
    }

    method join(DynamicLatticeValue:D $other --> DynamicLatticeValue:D) {
        self!binary($other, &lling-lattice-join, 'lattice-join')
    }

    method meet(DynamicLatticeValue:D $other --> DynamicLatticeValue:D) {
        self!binary($other, &lling-lattice-meet, 'lattice-meet')
    }

    method equivalent(DynamicLatticeValue:D $other --> Bool:D) {
        my uint8 $equal = 0xff;
        check-status(lling-lattice-equal(self.native-handle,
            $other.native-handle, $equal), 'lattice-equal');
        $equal == 1
    }

    method !bytes(&operation, Str:D $name --> Blob:D) {
        my size_t $written = 0;
        my size_t $required = 0;
        check-status(operation(self.native-handle, Pointer, 0,
            $written, $required), "{$name}-size");
        my $storage = CArray[uint8].new;
        $storage[$_] = 0 for ^$required;
        check-status(operation(self.native-handle,
            nativecast(Pointer, $storage), $required, $written, $required),
            $name);
        Blob.new((^$written).map({ $storage[$_] }))
    }

    method stable-bytes(--> Blob:D) {
        self!bytes(&lling-lattice-stable-bytes, 'lattice-stable-bytes')
    }

    method diagnostic(--> Str:D) {
        self!bytes(&lling-lattice-diagnostic, 'lattice-diagnostic').decode('utf8')
    }

    method !many(
        Positional:D $others,
        &operation,
        Str:D $name,
        --> DynamicLatticeValue:D
    ) {
        my $pointers = CArray[Pointer].new;
        for $others.list.kv -> $index, $value {
            die 'lattice operand is not a DynamicLatticeValue'
                unless $value ~~ DynamicLatticeValue;
            $pointers[$index] = $value.native-handle;
        }
        my Pointer $result .= new;
        check-status(operation(self.native-handle,
            nativecast(Pointer, $pointers), $others.elems, $result), $name);
        DynamicLatticeValue.new(handle => $result)
    }

    method join-many(Positional:D $others --> DynamicLatticeValue:D) {
        self!many($others, &lling-lattice-join-many, 'lattice-join-many')
    }

    method meet-many(Positional:D $others --> DynamicLatticeValue:D) {
        self!many($others, &lling-lattice-meet-many, 'lattice-meet-many')
    }

    method close(--> Nil) {
        return if $!closed;
        lling-lattice-free($!handle);
        $!handle = Pointer;
        $!closed = True;
    }

    method opened(--> Bool:D) { !$!closed }
    submethod DESTROY { try self.close }
}

sub dynamic-lattice-value(InteropAccess::ResourceType:D $resource
    --> DynamicLatticeValue:D) is export {
    my Pointer $handle .= new;
    check-status(lling-lattice-open($resource.raw, $handle), 'lattice-open');
    DynamicLatticeValue.new(:$handle)
}

sub validate-lattice-laws(Positional:D $values --> Nil) is export {
    my $pointers = CArray[Pointer].new;
    for $values.list.kv -> $index, $value {
        die 'law sample is not a DynamicLatticeValue'
            unless $value ~~ DynamicLatticeValue;
        $pointers[$index] = $value.native-handle;
    }
    check-status(lling-lattice-validate-laws(nativecast(Pointer, $pointers),
        $values.elems), 'lattice-validate-laws');
}

class SemiringWeight is export {
    has Mu:D $.context is required;
    has Pointer $!handle is required;
    has Bool $!closed = False;

    submethod BUILD(Mu:D :$!context!, Pointer:D :$handle!) {
        $!handle = $handle
    }

    method native-handle(--> Pointer:D) {
        X::Lling::Llang.new(
            status => CLOSED,
            operation => 'semiring-weight',
            detail => 'semiring weight is closed',
        ).throw if $!closed;
        $!context.native-handle;
        $!handle
    }

    method clone(--> SemiringWeight:D) {
        my Pointer $result .= new;
        check-status(lling-semiring-weight-clone(self.native-handle, $result),
            'semiring-weight-clone');
        SemiringWeight.new(context => $!context, handle => $result)
    }

    method plus(SemiringWeight:D $right --> SemiringWeight:D) {
        $!context.plus(self, $right)
    }

    method times(SemiringWeight:D $right --> SemiringWeight:D) {
        $!context.times(self, $right)
    }

    method close(--> Nil) {
        return if $!closed;
        lling-semiring-weight-free($!handle);
        $!handle = Pointer;
        $!closed = True;
    }

    method opened(--> Bool:D) { !$!closed }
    submethod DESTROY { try self.close }
}

class SemiringContext is export {
    has Pointer $!handle is required;
    has Bool $!closed = False;

    submethod BUILD(Pointer:D :$handle!) { $!handle = $handle }

    method native-handle(--> Pointer:D) {
        X::Lling::Llang.new(
            status => CLOSED,
            operation => 'semiring-context',
            detail => 'semiring context is closed',
        ).throw if $!closed;
        $!handle
    }

    method properties(--> UInt:D) {
        my uint64 $result = 0;
        check-status(lling-semiring-properties(self.native-handle, $result),
            'semiring-properties');
        $result
    }

    method zero(--> SemiringWeight:D) {
        my Pointer $result .= new;
        check-status(lling-semiring-zero(self.native-handle, $result),
            'semiring-zero');
        SemiringWeight.new(context => self, handle => $result)
    }

    method one(--> SemiringWeight:D) {
        my Pointer $result .= new;
        check-status(lling-semiring-one(self.native-handle, $result),
            'semiring-one');
        SemiringWeight.new(context => self, handle => $result)
    }

    method !same(SemiringWeight:D $weight --> Pointer:D) {
        die 'semiring weight belongs to another operation context'
            unless $weight.context === self;
        $weight.native-handle
    }

    method !binary(
        SemiringWeight:D $left,
        SemiringWeight:D $right,
        &operation,
        Str:D $name,
        --> SemiringWeight:D
    ) {
        my Pointer $result .= new;
        check-status(operation(self.native-handle, self!same($left),
            self!same($right), $result), $name);
        SemiringWeight.new(context => self, handle => $result)
    }

    method plus(SemiringWeight:D $left, SemiringWeight:D $right
        --> SemiringWeight:D) {
        self!binary($left, $right, &lling-semiring-plus, 'semiring-plus')
    }

    method times(SemiringWeight:D $left, SemiringWeight:D $right
        --> SemiringWeight:D) {
        self!binary($left, $right, &lling-semiring-times, 'semiring-times')
    }

    method !many(Positional:D $weights, &operation, Str:D $name
        --> SemiringWeight:D) {
        my $pointers = CArray[Pointer].new;
        for $weights.list.kv -> $index, $weight {
            die 'semiring operand is not a SemiringWeight'
                unless $weight ~~ SemiringWeight;
            $pointers[$index] = self!same($weight);
        }
        my Pointer $result .= new;
        check-status(operation(self.native-handle,
            nativecast(Pointer, $pointers), $weights.elems, $result), $name);
        SemiringWeight.new(context => self, handle => $result)
    }

    method plus-many(Positional:D $weights --> SemiringWeight:D) {
        self!many($weights, &lling-semiring-plus-many, 'semiring-plus-many')
    }

    method times-many(Positional:D $weights --> SemiringWeight:D) {
        self!many($weights, &lling-semiring-times-many, 'semiring-times-many')
    }

    method equal(SemiringWeight:D $left, SemiringWeight:D $right --> Bool:D) {
        my uint8 $result = 255;
        check-status(lling-semiring-equal(self.native-handle, self!same($left),
            self!same($right), $result), 'semiring-equal');
        $result == 1
    }

    method approx-equal(
        SemiringWeight:D $left,
        SemiringWeight:D $right,
        Real:D $epsilon,
        --> Bool:D
    ) {
        my uint8 $result = 255;
        check-status(lling-semiring-approx-equal(self.native-handle,
            self!same($left), self!same($right), $epsilon.Num, $result),
            'semiring-approx-equal');
        $result == 1
    }

    method natural-order(
        SemiringWeight:D $left,
        SemiringWeight:D $right,
        --> Int:D
    ) {
        my int32 $result = -2147483648;
        check-status(lling-semiring-natural-order(self.native-handle,
            self!same($left), self!same($right), $result),
            'semiring-natural-order');
        $result
    }

    method !partial-binary(
        SemiringWeight:D $left,
        SemiringWeight:D $right,
        &operation,
        Str:D $name,
        --> SemiringWeight
    ) {
        my Pointer $result .= new;
        my uint8 $defined = 255;
        check-status(operation(self.native-handle, self!same($left),
            self!same($right), $result, $defined), $name);
        $defined == 1 ?? SemiringWeight.new(context => self, handle => $result)
            !! SemiringWeight
    }

    method divide(SemiringWeight:D $dividend, SemiringWeight:D $divisor
        --> SemiringWeight) {
        self!partial-binary($dividend, $divisor, &lling-semiring-divide,
            'semiring-divide')
    }

    method left-divide(SemiringWeight:D $value, SemiringWeight:D $divisor
        --> SemiringWeight) {
        self!partial-binary($value, $divisor, &lling-semiring-left-divide,
            'semiring-left-divide')
    }

    method star(SemiringWeight:D $value --> SemiringWeight) {
        my Pointer $result .= new;
        my uint8 $defined = 255;
        check-status(lling-semiring-star(self.native-handle, self!same($value),
            $result, $defined), 'semiring-star');
        $defined == 1 ?? SemiringWeight.new(context => self, handle => $result)
            !! SemiringWeight
    }

    method numerical-value(SemiringWeight:D $value --> Num:D) {
        my num64 $result = NaN;
        check-status(lling-semiring-numerical-value(self.native-handle,
            self!same($value), $result), 'semiring-numerical-value');
        $result
    }

    method quantize(SemiringWeight:D $value, Real:D $epsilon --> Int:D) {
        my int64 $result = 0;
        check-status(lling-semiring-quantize(self.native-handle,
            self!same($value), $epsilon.Num, $result), 'semiring-quantize');
        $result
    }

    method probability(SemiringWeight:D $value --> Num:D) {
        my num64 $result = NaN;
        check-status(lling-semiring-to-probability(self.native-handle,
            self!same($value), $result), 'semiring-probability');
        $result
    }

    method closure-bound(--> Int) {
        my size_t $result = 0;
        my uint8 $known = 255;
        check-status(lling-semiring-closure-bound(self.native-handle, $result,
            $known), 'semiring-closure-bound');
        $known == 1 ?? $result.Int !! Int
    }

    method !bytes(Pointer $value, &operation, Str:D $name --> Blob:D) {
        my size_t $written = 0;
        my size_t $required = 0;
        check-status(operation(self.native-handle, $value, Pointer, 0,
            $written, $required), "{$name}-size");
        my $storage = CArray[uint8].new;
        $storage[$_] = 0 for ^$required;
        check-status(operation(self.native-handle, $value,
            nativecast(Pointer, $storage), $required, $written, $required),
            $name);
        Blob.new((^$written).map({ $storage[$_] }))
    }

    method stable-bytes(SemiringWeight:D $value --> Blob:D) {
        self!bytes(self!same($value), &lling-semiring-stable-bytes,
            'semiring-stable-bytes')
    }

    multi method diagnostic(--> Str:D) {
        self!bytes(Pointer, &lling-semiring-diagnostic,
            'semiring-diagnostic').decode('utf8')
    }

    multi method diagnostic(SemiringWeight:D $value --> Str:D) {
        self!bytes(self!same($value), &lling-semiring-diagnostic,
            'semiring-diagnostic').decode('utf8')
    }

    method validate-laws(
        Positional:D $weights,
        Real:D :$epsilon = 0e0,
        --> Nil
    ) {
        my $pointers = CArray[Pointer].new;
        for $weights.list.kv -> $index, $weight {
            die 'law sample is not a SemiringWeight'
                unless $weight ~~ SemiringWeight;
            $pointers[$index] = self!same($weight);
        }
        check-status(lling-semiring-validate-laws(self.native-handle,
            nativecast(Pointer, $pointers), $weights.elems, $epsilon.Num),
            'semiring-validate-laws');
    }

    method close(--> Nil) {
        return if $!closed;
        lling-semiring-free($!handle);
        $!handle = Pointer;
        $!closed = True;
    }

    method opened(--> Bool:D) { !$!closed }
    submethod DESTROY { try self.close }
}

sub semiring-context(InteropAccess::ResourceType:D $resource
    --> SemiringContext:D) is export {
    my Pointer $handle .= new;
    check-status(lling-semiring-open($resource.raw, $handle), 'semiring-open');
    SemiringContext.new(:$handle)
}

sub semiring-domain-id(Str:D $name --> InteropAccess::InterfaceIdType:D)
    is export {
    InteropAccess::domain-id($name)
}

# Host-semiring provider ----------------------------------------------------

role SemiringProvider is export {
    method zero(--> Mu) { ... }
    method one(--> Mu) { ... }
    method plus(Mu:D, Mu:D --> Mu) { ... }
    method times(Mu:D, Mu:D --> Mu) { ... }
    method equal(Mu:D $left, Mu:D $right --> Bool:D) { $left eqv $right }
    method approx-equal(Mu:D $left, Mu:D $right, Real:D $epsilon --> Bool:D) {
        self.equal($left, $right)
    }
    method natural-order(Mu:D, Mu:D --> Int:D) { ... }
    method stable-bytes(Mu:D --> Blob:D) { ... }
    method diagnostic(Mu $value = Mu --> Str:D) {
        $value.defined ?? $value.raku !! self.^name
    }
    method divide(Mu:D, Mu:D --> Mu) { Mu }
    method left-divide(Mu:D, Mu:D --> Mu) { Mu }
    method star(Mu:D --> Mu) { Mu }
    method numerical-value(Mu:D --> Mu) { Mu }
    method quantize(Mu:D, Real:D --> Mu) { Mu }
    method probability(Mu:D --> Mu) { Mu }
    method properties(--> UInt:D) { 0 }
    method closure-bound(--> Mu) { Mu }
}

class SemiringSlot {
    has Mu $.value is rw;
    has UInt:D $.generation is rw = 1;
    has Int:D $.references is rw = 0;
    has Bool:D $.occupied is rw = False;
}

class SemiringProviderContext {
    has SemiringProvider:D $.implementation is required;
    has Lock:D $.arena-lock .= new;
    has SemiringSlot @.slots;
    has UInt @.free-slots;
    has Str:D $.diagnostic is rw = '';
    has CArray[uint64] $.host-context is required;
    has UInt $.closure-bound;

    method allocate(Mu:D $value --> List:D) {
        $!arena-lock.protect: {
            my UInt $index;
            if @!free-slots {
                $index = @!free-slots.pop;
            } else {
                @!slots.push(SemiringSlot.new);
                $index = @!slots.elems;
            }
            my $slot = @!slots[$index - 1];
            $slot.value = $value;
            $slot.references = 1;
            $slot.occupied = True;
            ($index, $slot.generation)
        }
    }

    method resolve(UInt:D $index, UInt:D $generation --> Mu:D) {
        $!arena-lock.protect: {
            die 'semiring token slot is out of range'
                unless 1 <= $index <= @!slots.elems;
            my $slot = @!slots[$index - 1];
            die 'semiring token is stale or already released'
                unless $slot.occupied && $slot.generation == $generation;
            $slot.value
        }
    }

    method clone-token(UInt:D $index, UInt:D $generation --> Nil) {
        $!arena-lock.protect: {
            die 'semiring token slot is out of range'
                unless 1 <= $index <= @!slots.elems;
            my $slot = @!slots[$index - 1];
            die 'semiring token is stale or already released'
                unless $slot.occupied && $slot.generation == $generation;
            $slot.references++;
        }
    }

    method release-token(UInt:D $index, UInt:D $generation --> Nil) {
        $!arena-lock.protect: {
            die 'semiring token slot is out of range'
                unless 1 <= $index <= @!slots.elems;
            my $slot = @!slots[$index - 1];
            die 'semiring token is stale or already released'
                unless $slot.occupied && $slot.generation == $generation;
            die 'semiring token reference count underflow'
                unless $slot.references > 0;
            $slot.references--;
            if $slot.references == 0 {
                $slot.value = Mu;
                $slot.occupied = False;
                $slot.generation = $slot.generation == 0xffffffffffffffff
                    ?? 1 !! $slot.generation + 1;
                @!free-slots.push($index);
            }
        }
    }
}

my Lock $SEMIRINGS-LOCK .= new;
my UInt $NEXT-SEMIRING-ID = 1;
my %SEMIRINGS;

sub configure-semiring-lifecycle(
    &drop (Pointer),
    &zero (Pointer, Pointer --> int32),
    &one (Pointer, Pointer --> int32),
    &clone-value (Pointer, Pointer, Pointer --> int32),
    &release-values (Pointer, Pointer, size_t --> int32),
    --> int32
) is native(&provider-library)
    is symbol('lling_raku_semiring_configure_lifecycle') { * }
sub configure-semiring-algebra(
    &plus (Pointer, Pointer, Pointer, Pointer --> int32),
    &times (Pointer, Pointer, Pointer, Pointer --> int32),
    &equal (Pointer, Pointer, Pointer, Pointer --> int32),
    &approx (Pointer, Pointer, Pointer, num64, Pointer --> int32),
    &order (Pointer, Pointer, Pointer, Pointer --> int32),
    --> int32
) is native(&provider-library)
    is symbol('lling_raku_semiring_configure_algebra') { * }
sub configure-semiring-buffers(
    &stable (Pointer, Pointer, Pointer, size_t, Pointer, Pointer --> int32),
    &diagnostic (Pointer, Pointer, Pointer, size_t, Pointer, Pointer --> int32),
    &plus-many (Pointer, Pointer, size_t, Pointer --> int32),
    &times-many (Pointer, Pointer, size_t, Pointer --> int32),
    --> int32
) is native(&provider-library)
    is symbol('lling_raku_semiring_configure_buffers') { * }
sub configure-semiring-optional(
    &divide (Pointer, Pointer, Pointer, Pointer --> int32),
    &left-divide (Pointer, Pointer, Pointer, Pointer --> int32),
    &star (Pointer, Pointer, Pointer --> int32),
    &numerical (Pointer, Pointer, Pointer --> int32),
    &probability (Pointer, Pointer, Pointer --> int32),
    --> int32
) is native(&provider-library)
    is symbol('lling_raku_semiring_configure_optional') { * }
sub configure-semiring-metadata(
    &quantize (Pointer, Pointer, num64, Pointer --> int32),
    &closure (Pointer, Pointer, Pointer --> int32),
    --> int32
) is native(&provider-library)
    is symbol('lling_raku_semiring_configure_metadata') { * }

sub create-semiring-provider(
    uint64,
    InteropAccess::InterfaceIdType,
    uint64,
    uint8,
    uint8,
    uint8,
    Pointer,
    InteropAccess::RawResourceType,
    --> int32
) is native(&provider-library) is symbol('lling_raku_semiring_create') { * }

sub semiring-provider-id(Pointer:D $pointer --> UInt:D) {
    nativecast(CArray[uint64], $pointer)[0]
}

sub semiring-provider-context(Pointer:D $pointer --> SemiringProviderContext:D) {
    $SEMIRINGS-LOCK.protect: {
        %SEMIRINGS{semiring-provider-id($pointer)} //
            die 'closed semiring provider context'
    }
}

sub semiring-provider-drop(Pointer:D $pointer --> Nil) {
    try $SEMIRINGS-LOCK.protect: {
        %SEMIRINGS{semiring-provider-id($pointer)}:delete
    }
}

sub semiring-provider-failure(
    SemiringProviderContext:D $context,
    Mu:D $error,
    --> int32
) {
    $context.diagnostic = $error.message;
    Vinary::Tree::Interop::PROVIDER-ERROR
}

sub token-words(Pointer:D $token --> List:D) {
    my $words = nativecast(CArray[uint64], $token);
    ($words[0], $words[1])
}

sub write-token(Pointer:D $output, UInt:D $index, UInt:D $generation --> Nil) {
    my $words = nativecast(CArray[uint64], $output);
    $words[0] = $index;
    $words[1] = $generation;
}

sub semiring-identity-callback(
    Pointer:D $raw-context,
    Pointer:D $output,
    Str:D $operation,
    --> int32
) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $value = $operation eq 'zero'
            ?? $context.implementation.zero !! $context.implementation.one;
        write-token($output, |$context.allocate($value));
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-zero-provider(Pointer:D $context, Pointer:D $output --> int32) {
    semiring-identity-callback($context, $output, 'zero')
}
sub semiring-one-provider(Pointer:D $context, Pointer:D $output --> int32) {
    semiring-identity-callback($context, $output, 'one')
}

sub semiring-clone-provider(
    Pointer:D $raw-context,
    Pointer:D $value,
    Pointer:D $output,
    --> int32
) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my ($index, $generation) = token-words($value);
        $context.clone-token($index, $generation);
        write-token($output, $index, $generation);
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-release-provider(
    Pointer:D $raw-context,
    Pointer $values,
    size_t $count,
    --> int32
) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $words = nativecast(CArray[uint64], $values);
        for ^$count -> $index {
            $context.release-token($words[$index * 2], $words[$index * 2 + 1]);
        }
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-binary-provider(
    Pointer:D $raw-context,
    Pointer:D $left,
    Pointer:D $right,
    Pointer:D $output,
    Str:D $operation,
    Bool:D :$partial = False,
    --> int32
) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $left-value = $context.resolve(|token-words($left));
        my $right-value = $context.resolve(|token-words($right));
        my $implementation = $context.implementation;
        my $result = do given $operation {
            when 'plus' { $implementation.plus($left-value, $right-value) }
            when 'times' { $implementation.times($left-value, $right-value) }
            when 'divide' { $implementation.divide($left-value, $right-value) }
            when 'left-divide' {
                $implementation.left-divide($left-value, $right-value)
            }
            default { die "unknown semiring binary operation $operation" }
        };
        return Vinary::Tree::Interop::END if $partial && !$result.defined;
        write-token($output, |$context.allocate($result));
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-plus-provider(Pointer:D $c, Pointer:D $l, Pointer:D $r,
    Pointer:D $o --> int32) { semiring-binary-provider($c, $l, $r, $o, 'plus') }
sub semiring-times-provider(Pointer:D $c, Pointer:D $l, Pointer:D $r,
    Pointer:D $o --> int32) { semiring-binary-provider($c, $l, $r, $o, 'times') }
sub semiring-divide-provider(Pointer:D $c, Pointer:D $l, Pointer:D $r,
    Pointer:D $o --> int32) {
    semiring-binary-provider($c, $l, $r, $o, 'divide', :partial)
}
sub semiring-left-divide-provider(Pointer:D $c, Pointer:D $l, Pointer:D $r,
    Pointer:D $o --> int32) {
    semiring-binary-provider($c, $l, $r, $o, 'left-divide', :partial)
}

sub semiring-equal-provider(Pointer:D $raw-context, Pointer:D $left,
    Pointer:D $right, Pointer:D $output --> int32) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $result = $context.implementation.equal(
            $context.resolve(|token-words($left)),
            $context.resolve(|token-words($right)));
        nativecast(CArray[uint8], $output)[0] = $result ?? 1 !! 0;
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-approx-provider(Pointer:D $raw-context, Pointer:D $left,
    Pointer:D $right, num64 $epsilon, Pointer:D $output --> int32) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $result = $context.implementation.approx-equal(
            $context.resolve(|token-words($left)),
            $context.resolve(|token-words($right)), $epsilon);
        nativecast(CArray[uint8], $output)[0] = $result ?? 1 !! 0;
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-order-provider(Pointer:D $raw-context, Pointer:D $left,
    Pointer:D $right, Pointer:D $output --> int32) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $order = $context.implementation.natural-order(
            $context.resolve(|token-words($left)),
            $context.resolve(|token-words($right))).Int;
        die 'natural order must be -1, 0, 1, or 2'
            unless $order == any(-1, 0, 1, 2);
        nativecast(CArray[int32], $output)[0] = $order;
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub write-provider-bytes(Blob:D $bytes, Pointer $output, size_t $capacity,
    Pointer:D $written, Pointer:D $required --> Nil) {
    nativecast(CArray[size_t], $required)[0] = $bytes.elems;
    my $count = $capacity min $bytes.elems;
    if $count {
        my $target = nativecast(CArray[uint8], $output);
        $target[$_] = $bytes[$_] for ^$count;
    }
    nativecast(CArray[size_t], $written)[0] = $count;
}

sub semiring-bytes-provider(Pointer:D $raw-context, Pointer $value,
    Pointer $output, size_t $capacity, Pointer:D $written, Pointer:D $required,
    Bool:D :$diagnostic = False, --> int32) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $resolved = $value.defined
            ?? $context.resolve(|token-words($value)) !! Mu;
        my Blob $bytes = $diagnostic
            ?? $context.implementation.diagnostic($resolved).encode('utf8')
            !! $context.implementation.stable-bytes($resolved);
        write-provider-bytes($bytes, $output, $capacity, $written, $required);
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-stable-provider(Pointer:D $c, Pointer:D $v, Pointer $o,
    size_t $n, Pointer:D $w, Pointer:D $r --> int32) {
    semiring-bytes-provider($c, $v, $o, $n, $w, $r)
}
sub semiring-diagnostic-provider(Pointer:D $c, Pointer $v, Pointer $o,
    size_t $n, Pointer:D $w, Pointer:D $r --> int32) {
    semiring-bytes-provider($c, $v, $o, $n, $w, $r, :diagnostic)
}

sub semiring-many-provider(Pointer:D $raw-context, Pointer $values,
    size_t $count, Pointer:D $output, Str:D $operation --> int32) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $implementation = $context.implementation;
        my $result = $operation eq 'plus'
            ?? $implementation.zero !! $implementation.one;
        for ^$count -> $index {
            my $token = Pointer.new($values.Int +
                $index * nativesizeof(InteropAccess::SemiringValueType));
            my $value = $context.resolve(|token-words($token));
            $result = $operation eq 'plus'
                ?? $implementation.plus($result, $value)
                !! $implementation.times($result, $value);
        }
        write-token($output, |$context.allocate($result));
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-plus-many-provider(Pointer:D $c, Pointer $v, size_t $n,
    Pointer:D $o --> int32) { semiring-many-provider($c, $v, $n, $o, 'plus') }
sub semiring-times-many-provider(Pointer:D $c, Pointer $v, size_t $n,
    Pointer:D $o --> int32) { semiring-many-provider($c, $v, $n, $o, 'times') }

sub semiring-star-provider(Pointer:D $raw-context, Pointer:D $value,
    Pointer:D $output --> int32) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $result = $context.implementation.star(
            $context.resolve(|token-words($value)));
        return Vinary::Tree::Interop::END unless $result.defined;
        write-token($output, |$context.allocate($result));
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-numeric-provider(Pointer:D $raw-context, Pointer:D $value,
    Pointer:D $output, Str:D $operation --> int32) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $resolved = $context.resolve(|token-words($value));
        my $result = $operation eq 'numerical'
            ?? $context.implementation.numerical-value($resolved)
            !! $context.implementation.probability($resolved);
        return Vinary::Tree::Interop::UNSUPPORTED unless $result.defined;
        nativecast(CArray[num64], $output)[0] = $result.Num;
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-numerical-provider(Pointer:D $c, Pointer:D $v,
    Pointer:D $o --> int32) { semiring-numeric-provider($c, $v, $o, 'numerical') }
sub semiring-probability-provider(Pointer:D $c, Pointer:D $v,
    Pointer:D $o --> int32) { semiring-numeric-provider($c, $v, $o, 'probability') }

sub semiring-quantize-provider(Pointer:D $raw-context, Pointer:D $value,
    num64 $epsilon, Pointer:D $output --> int32) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        my $result = $context.implementation.quantize(
            $context.resolve(|token-words($value)), $epsilon);
        return Vinary::Tree::Interop::UNSUPPORTED unless $result.defined;
        nativecast(CArray[int64], $output)[0] = $result.Int;
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

sub semiring-closure-provider(Pointer:D $raw-context, Pointer:D $output,
    Pointer:D $known --> int32) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = semiring-provider-context($raw-context);
        nativecast(CArray[size_t], $output)[0] = $context.closure-bound // 0;
        nativecast(CArray[uint8], $known)[0] = $context.closure-bound.defined
            ?? 1 !! 0;
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = semiring-provider-failure($context, $_) } }
    }
    $status
}

my Lock $SEMIRING-CONFIGURE-LOCK .= new;
my Bool $SEMIRING-CONFIGURED = False;

sub ensure-semiring-configured(--> Nil) {
    $SEMIRING-CONFIGURE-LOCK.protect: {
        return if $SEMIRING-CONFIGURED;
        my @statuses;
        @statuses.push(configure-semiring-lifecycle(
            &semiring-provider-drop, &semiring-zero-provider,
            &semiring-one-provider, &semiring-clone-provider,
            &semiring-release-provider));
        @statuses.push(configure-semiring-algebra(&semiring-plus-provider,
            &semiring-times-provider, &semiring-equal-provider,
            &semiring-approx-provider, &semiring-order-provider));
        @statuses.push(configure-semiring-buffers(&semiring-stable-provider,
            &semiring-diagnostic-provider, &semiring-plus-many-provider,
            &semiring-times-many-provider));
        @statuses.push(configure-semiring-optional(&semiring-divide-provider,
            &semiring-left-divide-provider,
            &semiring-star-provider, &semiring-numerical-provider,
            &semiring-probability-provider));
        @statuses.push(configure-semiring-metadata(&semiring-quantize-provider,
            &semiring-closure-provider));
        my $failure = @statuses.first({
            Vinary::Tree::Interop::Status($_) != Vinary::Tree::Interop::OK
        });
        die "failed to configure Raku semiring provider shim: $failure"
            if $failure.defined;
        die 'Raku semiring provider configuration did not complete'
            unless @statuses.elems == 5 && @statuses.all ==
                Vinary::Tree::Interop::OK;
        $SEMIRING-CONFIGURED = True;
    }
}

sub semiring-provider(
    SemiringProvider:D $implementation,
    InteropAccess::InterfaceIdType:D :$domain-id!,
    Bool:D :$division = False,
    Bool:D :$star = False,
    Bool:D :$numeric = False,
    Bool:D :$stable-bytes = False,
    Bool:D :$batch = True,
    Bool:D :$parallel = False,
    Bool:D :$thread-bound = True,
    --> InteropAccess::ResourceType:D
) is export {
    die 'a provider cannot be both thread-bound and parallel-reentrant'
        if $parallel && $thread-bound;
    ensure-semiring-configured;
    my $storage = CArray[uint64].new;
    my $identifier = $SEMIRINGS-LOCK.protect: { $NEXT-SEMIRING-ID++ };
    $storage[0] = $identifier;
    my $bound = $implementation.closure-bound;
    die 'closure bound cannot be negative' if $bound.defined && $bound < 0;
    my $context = SemiringProviderContext.new(:$implementation,
        host-context => $storage,
        closure-bound => ($bound.defined ?? $bound.Int !! UInt));
    $SEMIRINGS-LOCK.protect: { %SEMIRINGS{$identifier} = $context };
    my $flags = ($thread-bound ?? InteropAccess::SemiringThreadBound !! 0) +|
        ($parallel ?? InteropAccess::SemiringParallel !! 0) +|
        ($stable-bytes ?? InteropAccess::SemiringStable !! 0) +|
        ($batch ?? InteropAccess::SemiringBatch !! 0);
    my $raw = InteropAccess::RawResourceType.new;
    my $status = create-semiring-provider($flags, $domain-id,
        $implementation.properties, $division ?? 1 !! 0, $star ?? 1 !! 0,
        $numeric ?? 1 !! 0, nativecast(Pointer, $storage), $raw);
    unless Vinary::Tree::Interop::Status($status) == Vinary::Tree::Interop::OK {
        $SEMIRINGS-LOCK.protect: { %SEMIRINGS{$identifier}:delete };
        die "failed to create Raku semiring provider: $status";
    }
    InteropAccess::adopt($raw)
}

# Host-provider API ----------------------------------------------------------

class ProviderArc is export {
    has UInt $.input;
    has UInt $.output;
    has UInt:D $.target is required;
    has Real:D $.weight = 0e0;

    submethod TWEAK {
        valid-weight($!weight);
        die 'input is not a Unicode scalar' if $!input.defined &&
            ($!input > 0x10ffff || $!input ~~ 0xd800..0xdfff);
        die 'output is not a Unicode scalar' if $!output.defined &&
            ($!output > 0x10ffff || $!output ~~ 0xd800..0xdfff);
    }
}

class ProviderState is export {
    has Bool:D $.valid = True;
    has Bool:D $.final = False;
    has Real:D $.final-weight = Inf;
    has ProviderArc:D @.arcs;

    submethod TWEAK {
        valid-weight($!final-weight);
        $!final = False unless $!valid;
        $!final-weight = Inf unless $!valid && $!final;
    }
}

role WfstProvider is export {
    method start-state(--> UInt:D) { ... }
    method state-count(--> Int) { Int }
    method state(UInt:D --> ProviderState:D) { ... }
}

class ProviderContext {
    has WfstProvider:D $.implementation is required;
    has Lock:D $.cache-lock .= new;
    has %.states;
    has Str:D $.diagnostic is rw = '';
    has CArray[uint64] $.host-context is required;

    method state(UInt:D $identifier --> ProviderState:D) {
        my $cached = $!cache-lock.protect: { %!states{$identifier} };
        return $cached if $cached.defined;
        # Customer code deliberately runs outside the cache lock.
        my $expanded = $!implementation.state($identifier);
        $!cache-lock.protect: {
            %!states{$identifier} //= $expanded
        }
    }
}

my Lock $PROVIDERS-LOCK .= new;
my UInt $NEXT-PROVIDER-ID = 1;
my %PROVIDERS;
my Lock $PROVIDER-ERROR-LOCK .= new;
my Str $LAST-PROVIDER-ERROR = '';

sub provider-last-error(--> Str:D) is export {
    $PROVIDER-ERROR-LOCK.protect: { $LAST-PROVIDER-ERROR }
}

sub configure-provider(
    &drop (Pointer),
    &start (Pointer, Pointer --> int32),
    &count (Pointer, Pointer, Pointer --> int32),
    &state-info (Pointer, uint64, Pointer, Pointer, Pointer --> int32),
    &state-arcs (Pointer, uint64, size_t, Pointer, size_t, Pointer, Pointer --> int32),
    --> int32
) is native(&provider-library) is symbol('lling_raku_provider_configure') { * }

sub create-provider(
    uint32,
    uint32,
    uint64,
    Pointer,
    InteropAccess::RawResourceType,
    --> int32
) is native(&provider-library) is symbol('lling_raku_provider_create') { * }

sub memcpy(Pointer, Pointer, size_t --> Pointer) is native { * }

sub provider-id(Pointer:D $context --> UInt:D) {
    nativecast(CArray[uint64], $context)[0]
}

sub provider-context(Pointer:D $context --> ProviderContext:D) {
    $PROVIDERS-LOCK.protect: {
        %PROVIDERS{provider-id($context)} //
            die 'closed WFST provider context'
    }
}

sub provider-drop(Pointer:D $context --> Nil) {
    try $PROVIDERS-LOCK.protect: { %PROVIDERS{provider-id($context)}:delete }
}

sub callback-failure(ProviderContext:D $context, Mu $error --> int32) {
    $context.diagnostic = $error.message;
    $PROVIDER-ERROR-LOCK.protect: { $LAST-PROVIDER-ERROR = $error.message };
    Vinary::Tree::Interop::PROVIDER-ERROR
}

sub provider-start(Pointer:D $raw-context, Pointer:D $output --> int32) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = provider-context($raw-context);
        nativecast(CArray[uint64], $output)[0] =
            $context.implementation.start-state;
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = callback-failure($context, $_) } }
    }
    $status
}

sub provider-count(
    Pointer:D $raw-context,
    Pointer:D $output,
    Pointer:D $known,
    --> int32
) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = provider-context($raw-context);
        my $count = $context.implementation.state-count;
        if $count.defined {
            nativecast(CArray[size_t], $output)[0] = $count;
            nativecast(CArray[uint8], $known)[0] = 1;
        } else {
            nativecast(CArray[size_t], $output)[0] = 0;
            nativecast(CArray[uint8], $known)[0] = 0;
        }
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = callback-failure($context, $_) } }
    }
    $status
}

sub provider-state-info(
    Pointer:D $raw-context,
    uint64 $identifier,
    Pointer:D $valid,
    Pointer:D $final,
    Pointer:D $weight,
    --> int32
) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = provider-context($raw-context);
        my $state = $context.state($identifier);
        nativecast(CArray[uint8], $valid)[0] = $state.valid ?? 1 !! 0;
        nativecast(CArray[uint8], $final)[0] = $state.final ?? 1 !! 0;
        nativecast(CArray[num64], $weight)[0] = $state.final-weight.Num;
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = callback-failure($context, $_) } }
    }
    $status
}

sub provider-state-arcs(
    Pointer:D $raw-context,
    uint64 $identifier,
    size_t $start,
    Pointer $output,
    size_t $capacity,
    Pointer:D $written,
    Pointer:D $total,
    --> int32
) {
    my int32 $status = Vinary::Tree::Interop::PROVIDER-ERROR;
    try {
        my $context = provider-context($raw-context);
        my @arcs = $context.state($identifier).arcs;
        die 'arc offset exceeds total' if $start > @arcs.elems;
        my $count = $capacity min (@arcs.elems - $start);
        for ^$count -> $index {
            my $arc = @arcs[$start + $index];
            my $raw = InteropAccess::WfstArcType.new(
                input-label => $arc.input // 0,
                output-label => $arc.output // 0,
                target-state => $arc.target,
                weight => $arc.weight.Num,
                has-input => $arc.input.defined ?? 1 !! 0,
                has-output => $arc.output.defined ?? 1 !! 0,
            );
            my $target = Pointer.new($output.Int +
                $index * nativesizeof(InteropAccess::WfstArcType));
            memcpy($target, nativecast(Pointer, $raw),
                nativesizeof(InteropAccess::WfstArcType));
        }
        nativecast(CArray[size_t], $written)[0] = $count;
        nativecast(CArray[size_t], $total)[0] = @arcs.elems;
        $status = Vinary::Tree::Interop::OK;
        CATCH { default { $status = callback-failure($context, $_) } }
    }
    $status
}

my Lock $CONFIGURE-LOCK .= new;
my Bool $CONFIGURED = False;

sub ensure-provider-configured(--> Nil) {
    $CONFIGURE-LOCK.protect: {
        return if $CONFIGURED;
        my $status = configure-provider(
            &provider-drop,
            &provider-start,
            &provider-count,
            &provider-state-info,
            &provider-state-arcs,
        );
        die "failed to configure Raku WFST provider shim: $status"
            unless Vinary::Tree::Interop::Status($status) ==
                Vinary::Tree::Interop::OK;
        $CONFIGURED = True;
    }
}

sub provider(
    WfstProvider:D $implementation,
    UnitDomain:D :$unit-domain = InteropAccess::UnicodeDomain,
    WeightDomain:D :$weight-domain = InteropAccess::TropicalDomain,
    Bool:D :$parallel = False,
    Bool:D :$acyclic = False,
    --> InteropAccess::WfstType:D
) is export {
    ensure-provider-configured;
    my $storage = CArray[uint64].new;
    my $identifier = $PROVIDERS-LOCK.protect: { $NEXT-PROVIDER-ID++ };
    $storage[0] = $identifier;
    my $context = ProviderContext.new(:$implementation,
        host-context => $storage);
    $PROVIDERS-LOCK.protect: { %PROVIDERS{$identifier} = $context };
    my $flags = InteropAccess::ImmutableFlag +| InteropAccess::LazyFlag +|
        ($parallel ?? InteropAccess::ParallelFlag !! 0) +|
        ($acyclic ?? InteropAccess::AcyclicFlag !! 0);
    my $raw = InteropAccess::RawResourceType.new;
    my $status = create-provider($unit-domain, $weight-domain, $flags,
        nativecast(Pointer, $storage), $raw);
    unless Vinary::Tree::Interop::Status($status) == Vinary::Tree::Interop::OK {
        $PROVIDERS-LOCK.protect: { %PROVIDERS{$identifier}:delete };
        die "failed to create Raku WFST provider: $status";
    }
    InteropAccess::wrap(InteropAccess::adopt($raw))
}

INIT {
    die "lling-llang ABI mismatch: native {abi-version()} / facade {ABI-VERSION}"
        unless abi-version() == ABI-VERSION;
    die "lling-llang API revision {api-revision()} is older than {API-REVISION}"
        unless api-revision() >= API-REVISION;
}
