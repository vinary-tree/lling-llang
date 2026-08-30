unit module Lling::Llang;

use NativeCall;
need Lling::Llang::GeneratedAbi;
need Vinary::Tree::Interop;

our constant ABI-VERSION is export = Lling::Llang::GeneratedAbi::ABI-VERSION;
our constant API-REVISION is export = Lling::Llang::GeneratedAbi::API-REVISION;
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
    our constant UnicodeDomain = UNICODE-SCALAR;
    our constant TropicalDomain = TROPICAL-F64;
    our constant ImmutableFlag = WFST-FLAG-IMMUTABLE;
    our constant LazyFlag = WFST-FLAG-LAZY;
    our constant ParallelFlag = WFST-FLAG-PARALLEL-REENTRANT;
    our constant AcyclicFlag = WFST-FLAG-ACYCLIC;

    our sub adopt(RawResource:D $raw --> Resource:D) { adopt-resource($raw) }
    our sub wrap(Resource:D $resource --> Wfst:D) { wfst($resource, :take) }
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
    is native(&native-library) is symbol('lling_api_revision') { * }
sub lling-last-error-message(--> Str)
    is native(&native-library) is symbol('lling_last_error_message') { * }
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
