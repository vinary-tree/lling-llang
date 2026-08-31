# Lling-Llang for Raku

Composable weighted finite-state transducers and host-defined automata for
Raku. A **weighted finite-state transducer** (WFST) is a directed graph whose
arcs consume an input label, produce an output label, and carry a weight. This
package builds Unicode/tropical WFSTs, imports Vinary Tree resources, composes
immutable snapshots lazily, and lets Raku classes implement custom providers.
Raku classes can also implement weight semirings that native lling-llang
algorithms consume through the shared capability ABI.

## Install

The feature branch is a build-only `4.0.0-rc.5` candidate. Build the native
engine and the small C17 callback shim locally:

```sh
cargo build --no-default-features --features raku-bindings
mkdir -p target/raku-provider
raku bindings/raku/build-provider.raku \
  --output="$PWD/target/raku-provider/liblling_llang_raku_provider.so" \
  --interop-include="$PWD/../vinary-tree-interop/include"
export LLING_LLANG_LIBRARY="$PWD/target/debug/liblling_llang.so"
export LLING_LLANG_RAKU_PROVIDER_LIB="$PWD/target/raku-provider/liblling_llang_raku_provider.so"
```

The Zef build hook performs the shim build during package installation and
uses the platform's dynamic-library suffix.

## Quickstart

```raku
use Lling::Llang;

my $builder = WfstBuilder.new(size-hint => 2);
my $source = $builder.add-state;
my $target = $builder.add-state;
$builder.set-start($source).set-final($target);
$builder.add-arc($source, 'a', 'b', $target, 0.25);
my $graph = $builder.build;

say $graph.arcs($graph.start).head.output;
$graph.close;
```

Composition connects equal middle-tape labels. With tropical weights,
multiplication is addition, so matching weights $`w_1`$ and $`w_2`$ produce
$`w_1 \otimes w_2 = w_1 + w_2`$.

### Implement a lazy Raku provider

```raku
class RewriteAB does WfstProvider {
    method start-state(--> UInt:D) { 0 }
    method state-count(--> Int:D) { 2 }
    method state(UInt:D $id --> ProviderState:D) {
        return ProviderState.new(arcs => [ProviderArc.new(
            input => 'a'.ord, output => 'b'.ord, target => 1,
        )]) if $id == 0;
        return ProviderState.new(final => True, final-weight => 0e0)
            if $id == 1;
        ProviderState.new(valid => False)
    }
}

my $graph = provider(RewriteAB.new, :acyclic);
$graph.close;
```

`state` returns one complete immutable state. An undefined input or output
label denotes epsilon. Return the `Int` type object from `state-count` when a
lazy graph does not know its final size.

### Implement a Raku semiring

```raku
class Tropical does SemiringProvider {
    method zero(--> Mu) { Inf }
    method one(--> Mu) { 0e0 }
    method plus(Mu:D $a, Mu:D $b --> Mu) { min($a, $b) }
    method times(Mu:D $a, Mu:D $b --> Mu) { $a + $b }
    method natural-order(Mu:D $a, Mu:D $b --> Int:D) {
        $a < $b ?? -1 !! $a > $b ?? 1 !! 0
    }
    method stable-bytes(Mu:D $value --> Blob:D) {
        $value.Num.Str.encode('utf8')
    }
}

my $host = semiring-provider(Tropical.new,
    domain-id => semiring-domain-id('demo.tropical.v1'), :stable-bytes);
my $algebra = semiring-context($host);
$host.close;
my $zero = $algebra.zero;
my $one = $algebra.one;
my $best = $one.plus($zero);
die 'bad algebra' unless $algebra.equal($best, $one);
.close for $zero, $one, $best;
$algebra.close;
```

The domain identifier is exactly 16 bytes. Enable `:division`, `:star`, or
`:numeric` only when the corresponding provider methods are meaningful;
undefined division and star return the `SemiringWeight` type object. Declared
properties and `closure-bound` come from provider methods. Call
`validate-laws` on representative weights before relying on those claims.

## Ownership & memory model

`WfstBuilder.build` consumes its native builder on success. Every returned
`Vinary::Tree::Interop::Wfst` owns one resource retain. `compose` snapshots and
retains both inputs during construction; closing the inputs afterward does not
invalidate the product. Call `.close` deterministically. `DESTROY` is a
leak-safety fallback.

The provider shim owns an atomic retain count and roots the corresponding Raku
object until the last resource release. Provider snapshots are
identity-with-retain; changing the provider's visible graph after construction
breaks the immutable snapshot law.

Each semiring weight owns a generation-checked token in a recycling Raku
arena. `.clone` asks the provider to retain the token; copying its two raw
words is not ownership. The operation context retains the host resource
independently, and deterministic `.close` releases every token exactly once.

## Errors

Native failures throw `X::Lling::Llang` with a stable `Status`, operation, and
copied diagnostic. Raku provider exceptions are contained by `try` and become
the interop `PROVIDER-ERROR` status; no exception unwinds through C. The facade
rejects invalid Unicode labels, `NaN`, and negative-infinite tropical weights.

## Concurrency

Providers are serialized by lling-llang unless `:parallel` is declared. Use
that flag only if all provider methods are concurrently callable and
reentrant. The cache lock protects publication only; customer `state` methods
run outside it. The C shim uses atomics for resource lifetime and holds no lock
while invoking Raku.

Semiring providers default to `:thread-bound`. Their arena lock protects only
token lookup and publication; user algebra methods execute outside it. Use
`:parallel, :!thread-bound` only for a genuinely concurrent, reentrant
provider. The shim splits callback registration into bounded groups because
large NativeCall signatures are not portable across supported Rakudo/libffi
combinations.

## Zero-copy paths

`resource($graph)` returns a two-word retained handle. Import and composition
cross the boundary in constant time and preserve lazy state expansion. The
provider callback fills native caller-owned arc pages directly; the Raku
facade caches a complete immutable `ProviderState` so customer code runs at
most once per state in the uncontended case.

## Security and provider trust

The shared ABI negotiates the `vt.scalar-wfst.1` capability and version before
any traversal. Rust validates statuses, booleans, labels, weights, reserved
bytes, page counts, and ownership. Applications must still treat a foreign
provider as synchronous plugin code and validate its claimed immutability,
weight domain, acyclicity, and threading behavior.

## Troubleshooting

- Set `LLING_LLANG_LIBRARY` when the platform loader cannot locate the Rust
  library.
- Set `LLING_LLANG_RAKU_PROVIDER_LIB` when testing from a source tree where
  the Zef build hook has not staged the callback shim.
- `INCOMPATIBLE-RESOURCE` means an imported graph lacks the scalar-WFST
  capability or uses a non-Unicode/non-tropical domain.
- `PROVIDER-ERROR` means a provider method threw or produced malformed data.
  `provider-last-error()` returns the most recently copied provider diagnostic
  in the process. It is intended for test and troubleshooting output; call the
  corresponding Raku method directly when an application needs structured
  exception handling.

## Version compatibility

| Component | Required value |
|---|---:|
| Lling-Llang | `4.0.0-rc.5` |
| lling-llang C ABI | `1` |
| lling-llang API revision | at least `3` |
| Vinary-Tree-Interop | `4.0.0` compatible |
| Raku | language version `6.d` |

Module initialization checks the native ABI and API revision.

## Executable conformance evidence

[`t/01-conformance.rakutest`](t/01-conformance.rakutest) exercises ABI
negotiation, the eager builder, resource import, a Raku-defined provider,
snapshot survival, arc paging, and lazy tropical composition. It also sends a
Raku-defined semiring through Rust and tests optional operations, law
validation, stable bytes, token cloning, and deterministic release.

```sh
TMPDIR="$PWD/target/raku-tmp" \
RAKULIB="$PWD/bindings/raku/lib,$PWD/../vinary-tree-interop/bindings/raku/lib" \
raku bindings/raku/t/01-conformance.rakutest
```

## Maintainer workflow

1. Change the C ABI and `bindings/api.json` together.
2. Regenerate `GeneratedAbi.rakumod`; never hand-edit generated values.
3. Compile the C17 shim with warnings denied, then run Rust, Raku, binding,
   documentation, and pgmcp bug gates.
4. Commit source, generated surface, package docs, and evidence together.
5. Push only the approved feature branch; do not tag or publish this candidate.
