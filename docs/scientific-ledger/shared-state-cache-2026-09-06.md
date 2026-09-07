# Retained-state cache investigation — 2026-09-06

Status: implementation and performance qualification in progress. This entry
does not claim that the cache task, binding campaign or performance goals are complete.

## Observed defects

At lling-llang commit `d18a1236`, `ProviderResource` retains state expansions in
an unbounded `RwLock<HashMap<u64, Arc<StateData>>>`. Duallity commit `63eb8b8`
clones a native wrapper cache inside each provider callback. The exporter
already serves warm calls; the defect is split policy ownership, not a complete
absence of reuse. The outer cache hides bounded/no-cache adapter semantics.

It also retains `valid: false` responses. A raw-ABI regression first probes ID
1, expands ID 0 to discover ID 1, then probes ID 1 again. The old exporter
incorrectly keeps returning invalid. The baseline test run had one pass
(warm reuse) and one failure (dynamic discovery).

## Hypotheses and current evidence

| Hypothesis | Intervention | Evidence | Decision |
|---|---|---|---|
| One owner can expose honest policy controls without changing the language | Generic immutable-state cache in the exporter; direct-source duallity conversion | 421/421 duallity all-features debug tests passed, including all nine exported families through policy changes, eviction, clear and source drop | Correctness supported; broader qualification continues |
| Immutable root publication can preserve exact LRU and clear without a cache mutex | Persistent resident and ordered recency indexes; one generation-aware CAS | Differential histories, paused publication races, raw-ABI fault and eviction checks, and four finite models plus negative controls pass | Supported for tested cases; not a memory-model proof |
| Ordered resident lookup is fast enough for the warm path | First candidate uses two ordered persistent maps | Warm info slowed from approximately 24.7 ns to 46 ns; perf attributed 66% of sampled cycles to resident `OrdMap::get` | Rejected as the final performance selection |
| Generation reference counting contributes avoidable warm cost | Clone the marker only when publication may be needed | Isolated warm-info point estimate approximately 44.1 ns; preflight load varied | Retain semantic simplification; do not claim it resolves the regression |
| Unordered persistent lookup reduces the observed search cost | Replace only the resident index with a hash-array mapped trie; preserve ordered recency and atomic publication | Warm-info point estimate 31.563 ns; counted one-thread kernels are near the old lookup, and eight-thread throughput is about 4.7 times higher | Retain for broader workload qualification; not an end-to-end speedup claim |

## Baseline and first candidate

Workload: 4,096 warmed, sequentially visited state IDs, with 0, 8 or 128 arcs
per state. The synthetic provider isolates exporter/cache overhead from
dictionary computation. `state_info` and `state_arcs` are the actual resource
ABI callbacks. Criterion used 20 samples, one second warm-up and two seconds
measurement per case. Construction and initial cache population are outside
the timed iterations.

| Callback / degree | Old exporter point estimate | First ordered-index candidate |
|---|---:|---:|
| info / 0 | 24.677 ns | 45.876 ns |
| arcs / 0 | 25.040 ns | 46.813 ns |
| info / 8 | 24.752 ns | 46.491 ns |
| arcs / 8 | 36.088 ns | 56.649 ns |
| info / 128 | 24.602 ns | 46.659 ns |
| arcs / 128 | 151.32 ns | 278.27 ns |

The candidate adds cumulative statistics absent from the baseline. This is an
end-to-end cost comparison, not an isolated comparison of two map algorithms.
The larger degree-128 difference cannot be attributed entirely to lookup:
allocation layout and memory-working-set effects remain to be examined.

Host: AMD Ryzen Threadripper PRO 5975WX, 32 online CPUs, Linux
`7.2.3-arch1-2`. Both runs were pinned to CPU 4. The governor was
`performance` with boost enabled; frequency was not fixed. Three-second
preflight CPU-4 idle averages were 94.02% before baseline and 87.09% before
the first candidate. Other work continued on the host. These are exploratory
measurements, not release-level speedup claims or Java comparisons.

Whole-harness maximum RSS was 52,764 KiB for the baseline and 52,596 KiB for the
candidate; those figures include Criterion and fixtures, not just the cache.
No swaps were recorded. User-mode perf context-switch counters reported zero,
but GNU time recorded 391 and 491 involuntary switches respectively. Do not
interpret the user-only counters as proof of an interference-free run.

## Profiling and failed tool invocations

`perf record --call-graph lbr` was rejected with `EINVAL` on this host.
The DWARF fallback collected 3,681 samples with zero lost samples. Self-cost
was 66.00% in `OrdMap::get`, 19.15% in `ResourceContext::state` and 7.51% in
`wfst_state_info`. Symbols include inlined work; these percentages are not
independent operation latencies.

AMD uProf ran headlessly using user-only event `pmcx76` and its own
`--affinity 4` option. The first invocation incorrectly used `taskset -c`
after the tool's argument separator, which uProf parsed as its own option.
That attempt produced no profile; the corrected invocation collected data.
Kernel samples are unavailable at `perf_event_paranoid=2`. The report also
warned about kernel-symbol addresses. Do not infer kernel lock-wait behavior
from this user-only profile.

The uProf CSV attributes 39,676 sampled cycle events to `OrdMap::get` and
11,962 to `ResourceContext::state`, corroborating the ordered lookup hotspot.
It lacks source-line debug information. A later candidate compilation overlapped
this profiling pass; it is exploratory function-level evidence, not a clean,
immutable-source release comparison. The uninstrumented timing passes are
separate from profiler runs.

## HAMT lookup and equal-statistics comparison

Replacing only resident lookup gives warm `state_info` / degree-zero latency
31.563 ns, with a 95% Criterion interval of 31.464–31.636 ns. CPU 4 was 99.01%
idle during the preceding three-second sample. That remains slower than the
old approximately 24.7 ns ABI call, which did not increment a hit counter.

To separate observability from lookup, the `counted_cache_kernel` group gives
both the old `RwLock<HashMap>` control and the persistent candidate the same
saturating atomic hit counter and reference-counted payload. Each has 4,096
warmed IDs. Sparse IDs multiply the dense IDs by 1,000,000,007; this tests an
actual sparse namespace rather than a dense-array special case.

| Namespace / callers | Counted RwLock control | Persistent HAMT candidate |
|---|---:|---:|
| Dense / 1 | 21.938 ns per lookup | 22.873 ns per lookup |
| Sparse / 1 | 23.041 ns per lookup | 22.766 ns per lookup |
| Dense / 8 | 6.8844 million lookups/s | 32.275 million lookups/s |
| Sparse / 8 | 6.8197 million lookups/s | 31.785 million lookups/s |

The eight-caller throughput ratios are approximately 4.69 and 4.66. These
measure the cache kernel, not the provider ABI, dictionary traversal, a foreign
binding, or a Java comparison. At one caller the dense candidate is about 4%
slower, while the sparse result is similar; no universal latency win is claimed.

Criterion uses 20 samples, one-second warm-up and two-second measurement.
Each timed sample creates and joins its worker threads and includes the start
barrier; those costs are amortized across the requested iterations, not excluded.
Each worker executes the iteration count, so the throughput unit is the number
of workers per group iteration. Reported eight-worker batch latency must not
be presented as single-lookup latency.

The run allowed CPUs 4–11 under an eight-CPU quota, with a 4 GiB memory limit
and no swap. Preflight per-core idle ranged from 91.03% to 98.67%. Whole-harness
maximum RSS was 53,764 KiB; elapsed time 26.83 seconds; no swaps were recorded.
GNU time reported 3,572 voluntary and 2,879 involuntary context switches.
Frequency was not fixed. Paired release qualification and other workload
shapes remain necessary.

## Strengthened correctness evidence

The focused raw-ABI suite now passes nine tests, including exact capacity-one
and capacity-two retention/recomputation counts; original provider error
statuses on both callbacks; successful retry after error; failing outer calls
after successful same-ID reentry; nine concurrent policy-replacement cases;
and multiple arc pages separated by clear and policy replacement.

The earlier model had two one-shot workers and could not evict a third distinct
ID from capacity two. It was not sufficient evidence for that path. The revised
model permits two requests per worker and checks independent admission
provenance. Its first revision exposed a TLA+ boolean-precedence error in the
ghost-variable assignments; parenthesized right-hand sides corrected the model.
That failed run is retained, not counted as a production-code defect.

The normal configurations exhaust 60,142 (LRU1), 69,192 (LRU2), 49,095 (CacheAll)
and 4,868 (NoCache) distinct states with no invariant violations. The eviction
witness reaches an actual capacity-two eviction. The stale-generation mutant
fails specifically at `NoStaleOrFailedAdmission`, as required. Model limits and
the runtime mapping are documented in the
[architecture guide](../architecture/shared-state-cache.md#finite-concurrency-model-and-runtime-correspondence).

The full no-default-features, `bindings-core` debug nextest run passes
2,811/2,811 tests with no skips. This validates the current integration worktree,
not yet the final committed dependency graph. A compiled cache usage doctest
also passes. Final release and broader-feature gates are recorded separately
when complete.

The selected implementation's release cache tests pass 16/16, and its release
raw-ABI tests pass 9/9. Strict all-target, no-default-features `bindings-core`
Clippy passes. Duallity's expanded debug all-features run passes 421/421 tests,
including parameterized transposition and merge/split in the exported family
loop. The shared consumer walker now rejects unstable totals, truncated pages
and zero progress before the reported end.

### Build-output identity collision found during release validation

The first duallity release rerun did not reach its tests: rustc reported two
incompatible `vinary_tree_interop::VtResource` identities. Cargo metadata showed
only one interop package in the requested dependency graph. However, the
existing `libdictenstein.d` and `liblevenshtein.d` dependency files referred to
the earlier `target/generalized-source-graph` archive, and their fixed-name
release `.rlib` outputs had been built from that archive. Both crates emit
`rlib`, `cdylib` and `staticlib`; unlike the hashed interop artifacts, those
fixed-name outputs had collided across source graphs sharing one target root.

This was a validation-output namespace error, not evidence for changing a
public Rust type or ABI. Cleaning only the two packages' release artifacts
removed 102 generated files (92.4 MiB) before rebuilding. The failed build log
is retained as `shared-cache-duallity-all-features-release-final.log`, and the
rebuild has its own log. A failed build is never counted as a passed test run.

Every future archived-source graph must use its own `CARGO_TARGET_DIR`, separate
from the active integration graph. Set it to that archive's own `target`
directory; do not point archived and live manifests at the same build root.
This prevents fixed-name native outputs from silently replacing one another
while their graph-specific Cargo fingerprints remain independently fresh.

After the scoped cleanup and rebuild, duallity's release all-features nextest
run passes 421/421 tests with no skips; its all-target/all-features strict Clippy
run also passes. The rebuilt release log is
`shared-cache-duallity-all-features-release-rebuilt.log`.

## Matched-flag A–B–B–A and policy costs

The next comparison fixes the CPU flags at `-C target-feature=+aes,+sse2`,
matching the repository configuration. Invoking Cargo from another repository
with `--manifest-path` alone does not select that manifest's `.cargo` settings;
the candidate was rebuilt from the lling-llang working directory before timing.
Both benchmark fingerprint records confirm the matched flags and profile.

The baseline binary SHA-256 is
`ad6e990cc4152d56f8ca10556bf7f6000a19f17cb645166f11dc1bc5e9ee9d6d`;
the candidate SHA-256 is
`fb47578b954c8a86185bcf9c56ba0416fd2a08a4db33985bbfab1729463db40a`.
No builds ran during these four passes. Each pass ran on CPU 4 with a 4 GiB
memory cap and no swap. Preflight idle was 99.01%, 99.67%, 100.00% and 92.36%
in A1, B1, B2, A2 order. Raw logs and separate Criterion baselines are retained.

| Actual ABI callback / degree | A1 old | B1 candidate | B2 candidate | A2 old |
|---|---:|---:|---:|---:|
| info / 0 | 24.835 ns | 33.244 ns | 32.676 ns | 24.853 ns |
| arcs / 0 | 25.118 ns | 35.004 ns | 34.464 ns | 25.183 ns |
| info / 8 | 24.454 ns | 36.087 ns | 35.216 ns | 24.519 ns |
| arcs / 8 | 36.017 ns | 55.559 ns | 54.759 ns | 36.046 ns |
| info / 128 | 24.571 ns | 33.284 ns | 32.417 ns | 24.659 ns |
| arcs / 128 | 143.27 ns | 162.82 ns | 253.89 ns | 146.37 ns |

The small warm calls remain slower than the old uninstrumented exporter. The
degree-128 candidate result varies substantially between passes; neither the
lower result alone nor their average establishes a reliable effect. CPU idle
before a pass does not prove an interference-free measurement or exclude
allocation-layout and cache-working-set effects. No uniform speedup is claimed.

The policy workload then visits sparse IDs under honest CacheAll, NoCache and
LRU64 semantics. A cold batch clears the cache and visits 256 IDs, so its time
includes metadata publication, source allocation and reclamation. The hot set
cycles through 64 warmed IDs; one `state_info` call is one timed operation.

| Policy / outgoing arcs | Cold 256-state batch | Hot 64-state-set request |
|---|---:|---:|
| CacheAll / 0 | 121.59 microseconds | 27.143 ns |
| NoCache / 0 | 9.1358 microseconds | 39.981 ns |
| LRU64 / 0 | 172.40 microseconds | 542.65 ns |
| CacheAll / 128 | 187.76 microseconds | 27.263 ns |
| NoCache / 128 | 55.132 microseconds | 216.74 ns |
| LRU64 / 128 | 235.64 microseconds | 509.67 ns |

These synthetic expansions are deliberately cheap. Here exact LRU metadata
updates cost more than recomputing even the 128-arc fixture. This is a remaining
structural bottleneck, not a reason to silently replace exact LRU with approximate
eviction. The old exporter offered neither equivalent bounded residency nor
NoCache, so these rows characterize policy costs rather than compare identical
old/new contracts. Preflight idle was 99.67%; the run took 44.04 seconds with
51,396 KiB maximum whole-harness RSS and no swaps.

### Next falsifiable interventions

Most recently used (MRU) denotes the tail of the exact LRU access order.

A separate three-second `perf` pass of `hot_set/lru64/0` collected 3,067 user-cycle
samples, zero lost. Self-cost includes HAMT sparse-chunk cloning (11.87%), HAMT
entry destruction (8.44%), `publish` (8.25%), B-tree leaf copy-on-write (5.08%),
and ArcSwap debt accounting (5.77%). Inlined symbols and bounded DWARF unwinding
limit exact source-line attribution, but both payload-map copying and ordered
metadata updates are visible. No provider computation occurs on these warm hits.

The reviewed experiment sequence is:

1. Elide an already-most-recent hit's no-op publication, linearized at its
   snapshot read. This must preserve exact LRU and is expected to help paired
   `state_info`/`state_arcs` requests, not the 64-ID round-robin test.
2. Separate immutable payload ownership from frequently updated primitive
   recency metadata. This isolates the cost of repeatedly cloning payload
   reference counts during HAMT path copying.
3. If publication remains dominant, test a persistent indexed-slot LRU with
   internal residency slots, immutable payload storage and primitive linked-order
   metadata. Internal compact slots must not assume dense external state IDs.

These are hypotheses, not selected implementations or completion claims. Each
must retain the existing generation, error, reentry, exact-order and bounded-
metadata checks and be compared against this recorded implementation before
adoption. Logs are `provider-cache-qualified-{a1,b1,b2,a2}.log`,
`provider-cache-policy-qualified.log`, and `provider-cache-lru-profile-report.log`.

For source-cost interpretation, let $`H`$ be warmed LRU hit time, $`M`$ steady
miss/admission time, $`N`$ NoCache computation time and $`h`$ the hit fraction.
Expected LRU time is lower precisely when:

```math
hH + (1-h)M < N.
```

If $`M > H`$, this gives $`h > (M-N)/(M-H)`$. The cold-batch time above includes
clear and reclamation, so it must not be substituted for $`M`$ without a separate
steady-miss measurement. Real classic and generalized adapter workloads are
needed to establish relevant break-even points beyond the synthetic provider.

The structural experiment gate is a reproducible improvement of at least 25%
in non-MRU hot64 latency, with no unexplained regression above 10% in unchanged
CacheAll, steady-miss or reclamation paths. This is a predeclared experimental
selection criterion, not evidence that any candidate has already met it.
Report allocation/copy and retained-memory costs as well as time. The MRU-only
experiment has a different prediction: paired info/arc calls improve, while
non-MRU round-robin is unchanged within measured noise.

If internal slots are tested, the ID index must bijectively name occupied slots;
payload/metadata lengths must equal bounded residency; reverse IDs and both
links must agree; traversal from head must visit each slot exactly once and end
at tail. Slot reuse must never carry a stale slot across a failed root CAS.
Stable payload-vector ownership must not clone inline payload Arcs during hits.

### MRU no-op experiment: implementation and qualification

The implementation now returns an already-most-recent payload without root
publication, with read linearization as described in the architecture guide.
It also reuses a competing canonical MRU value after cold computation. The
deterministic publication-race fixture now uses a non-MRU hit in a capacity-two
cache, so its barrier still exercises an actual failed compare-and-swap.
A separate test requires identical root and payload identities for the no-op
cases. All 17 generic cache tests, nine raw-ABI tests and strict all-target
`bindings-core` Clippy pass after this change.

The revised model explores 38,451 LRU1, 48,933 LRU2, 42,891 CacheAll and 4,868
NoCache states with no invariant violations. Both negative checks still fail
at the required invariant. This is 135,143 positive states; the reduction from
the previous model reflects omitted no-op writes, not a reduced request bound.

The pre-change paired info/arc benchmark ran with only 81.73% preflight idle
on CPU 4; its results are exploratory and are not accepted for the improvement
claim. A subsequent A–B–B–A attempt required at least 95% idle before each pass.
Its first preflight was 82.95%, so it stopped before measuring any new samples.
The all-CPU check found other Java and Rust compilation work, with no sampled
core meeting the 95% threshold. Correctness qualification continues independently
of that timing gate; the speedup remains unclaimed.

Both experiment executables are retained independently of Cargo's replaceable
output filename. Pre-MRU SHA-256:
`778f3496423a4b7ace8fa7272256071fa63abc0c175529a4cc4b04eb73af9607`.
Post-MRU SHA-256:
`cee45035868308cc2bb662481ef544b100dcf66f45bc0e8bfeb4fe910962c041`.
They are `target/agent-logs/provider-cache-before-mru` and
`target/agent-logs/provider-cache-after-mru`; timing, preflight and model logs
use the `provider-cache-mru-` prefix.

Post-MRU duallity validation also passes all 421 all-features tests in debug
and all 421 in release, with zero skips in both runs. Its strict all-target,
all-features Clippy check passes. The logs are `shared-cache-duallity-mru-debug.log`,
`shared-cache-duallity-mru-release.log` and `shared-cache-duallity-mru-clippy.log`
under duallity's `target/agent-logs`. The focused read-only MathJax check passes
all seven touched Markdown/Rustdoc sources, and the diagram was regenerated
headlessly. These results do not close the remaining performance qualification.

### Payload/index split: diagnostic result, not selected

The second experiment separated payload ownership from the ID-to-stamp HAMT
and retained the ordered stamp-to-ID index. A scalar optional MRU ID avoided a
second lookup for a no-op hit. Tests retained old and new roots and required
identical payload-index ownership, including competing cold publication and
clock renumbering. All 19 generic and nine ABI tests passed, as did strict
all-target Clippy and the read-only MathJax check.

The first original/split pair ran on CPU 10 with preflight idle of 95.83% and
98.36%. These are Criterion point estimates, in nanoseconds per ABI request:

| Workload | Original with MRU fast return | Split |
|---|---:|---:|
| Hot64, zero arcs | 552.27 | 472.04 |
| Hot64, 128 arcs | 538.76 | 482.60 |
| Steady eviction, capacity64, zero arcs | 652.60 | 862.83 |
| Steady eviction, capacity64, 128 arcs | 930.91 | 1,155.8 |

The A–B–B–A sequence stopped before its second candidate pass because idle fell
to 92.88%. This is an initial pair, not a completed replicated comparison.
Its modest hot improvement and slower misses do not satisfy the preregistered
selection criteria. The experiment remains useful for isolating payload copying.

A separate profile collected 3,072 user-cycle samples, zero lost. Primitive
stamp-HAMT cloning accounted for 8.85% self cost, its entry destruction for
7.04%, publication for 8.37%, and ordered-tree leaf copying for 4.67%.
The payload-copy intervention exposed remaining structural metadata costs.

The registered allocation harness wraps the system allocator only in its own
executable. It counts successful allocation/reallocation and deallocation
requests, requested bytes, net live bytes, and interval peak live bytes. It
does not measure allocator usable sizes, retained free pages, or process RSS;
it is never used for timing. Payload fixtures contain either zero or 640
unsigned 64-bit units. They exercise the generic cache, not the entire ABI.

The following representative metadata-only runs use 10,000 requests. Filled
figures are the seed phase's live-byte increase, excluding the empty owner.
Keyed randomized hashing can vary trie shape across independent runs.

| Measure | Original with MRU | Split | Vector slots |
|---|---:|---:|---:|
| Hot64 allocated bytes | 26,085,840 | 22,813,920 | 12,240,000 |
| Steady eviction64 allocated bytes | 31,925,120 | 42,250,144 | 39,895,584 |
| Filled64 live increase | 17,144 | 20,304 | 14,536 |
| Hot1024 allocated bytes | 40,971,080 | 36,071,160 | 25,799,544 |
| Steady eviction1024 allocated bytes | 54,151,496 | 73,902,624 | 78,695,216 |
| Filled1024 live increase | 220,480 | 314,176 | 191,984 |

These data make the tradeoff visible: neither removing payload-reference
clones on hits nor lowering filled memory guarantees cheaper eviction.
Hot intervals had zero net live-byte growth in these runs. Individual eviction
intervals may change a small number of trie nodes as resident IDs change;
allocation volume must not be confused with unbounded retained memory.

### Compact vector slots: implementation and qualification

The third experiment uses one policy-specific representation. Exact LRU has a
persistent external-ID-to-slot HAMT, an Arc-owned persistent vector of immutable
payload rows, a persistent vector of primitive predecessor/successor links,
and head/tail slots. Full eviction reuses the head slot without shifting other
indices. Reverse IDs live with payloads, keeping copied link rows to two machine
words. CacheAll keeps its direct payload HAMT; NoCache has no resident index.

All 22 generic tests and nine raw-ABI tests pass. The new checks cover an ID
moving into a different reused slot before a real failed CAS retries, retained
payload validity, shuffled histories across vector boundaries through capacity
1024, lazy growth with maximum capacity, and counter saturation. An independent
source review found no correctness defect. The old recency-clock tests were
replaced with bounded slot-reuse tests because this representation has no clock.
The prior source snapshots are retained with the experiment evidence.

The root-publication model still checks 135,143 states across its four policies.
The separate slot-refinement model checks 19 capacity-one and 433 capacity-two
states. Both positive models pass; stale-slot and stale-reverse-ID mutations
fail at their specified invariants. All ten positive/negative gate invocations
pass. This is finite safety/refinement evidence, not an unbounded or Rust-memory-
model proof. Clear and policy replacement remain covered by the protocol model
and runtime tests rather than the separate slot-representation model.

Slot timing attempts on CPU 10 and CPU 1 stopped before any sample because
preflight idle was 92.60% and 81.37%, respectively. No slot speedup is claimed.
A separate capacity1024 eviction profile collected 3,081 user-cycle samples,
zero lost. ID-HAMT entry destruction/cloning accounted for 13.26%/11.09% self
cost, payload-vector leaf copying/destruction for 10.69%/8.72%, and payload-
vector parent copying for 4.38%. The loaded host does not supply a timing
comparison; these process-local samples guide the next structural hypothesis.

Experiment executable identities:

- Original with MRU, matching extended benchmark:
  `b931340043c42b5ffd00fabc6ecac8f2ca68fb3e0a50e64862f97acd8e44c37e`.
- Split candidate:
  `c51a397af0919500a3c547bb8065b9e0af70240b738202266412e7d83864c5bd`.
- Vector-slot candidate:
  `a8ab2a797e649a802c71bd51bc3977705fc148af6434ed18907fb406c984fc0f`.

Their executables, source hashes, allocation TSVs and profile reports use the
`provider-cache-{before-split,after-split,slots}` and corresponding allocation,
split and slot prefixes under `target/agent-logs`. A preliminary MRU wrapper
attempt omitted systemd's `--expand-environment=no`, allowing pass-label
expansion before Bash and reused log filenames. Its partial reused-name output
is excluded; corrected wrappers retain each pass independently. No compile or
model-checking work owned by this campaign overlapped the timing samples.

### Consolidated ownership map: fourth structural experiment

The vector-slot eviction profile motivated combining payload ownership with
the stable slot in one ID-keyed HAMT. The link vector now contains reverse IDs
alongside predecessor/successor links. Unlike the original ID-to-payload-and-
timestamp map, this ownership map is unchanged on hits because slots are stable.
The intervention removes the separate payload vector from admission/eviction;
its cost is increasing each link row from two to three machine words. CacheAll
retains its payload-only map, without an unused per-entry slot.

The implementation passes 22 generic tests and nine raw-ABI tests. Retained-root
tests require the entire ownership-map root to remain shared on non-MRU hits
and competing cold publication, with unchanged payload reference counts. The
real failed-CAS slot-relocation test and all bounded-order checks still pass.
These are targeted results; full-suite results for the preceding vector-slot
implementation are not evidence for this fourth implementation.

The same separate allocation harness produced the following requested-byte
counts over 10,000 metadata-only requests. Filled memory again excludes the
empty owner; randomized HAMT shapes differ across cache instances.

| Measure | Original with MRU | Vector slots | Consolidated ownership |
|---|---:|---:|---:|
| Hot64 allocated bytes | 26,085,840 | 12,240,000 | 17,280,000 |
| Steady eviction64 allocated bytes | 31,925,120 | 39,895,584 | 37,807,680 |
| Filled64 live increase | 17,144 | 14,536 | 16,936 |
| Hot1024 allocated bytes | 40,971,080 | 25,799,544 | 30,999,800 |
| Steady eviction1024 allocated bytes | 54,151,496 | 78,695,216 | 68,467,640 |
| Filled1024 live increase | 220,480 | 191,984 | 227,312 |

Consolidation reduces miss allocation volume relative to separate vector slots,
but remains above the original: about 18% at capacity64 and 26% at capacity1024.
Eviction allocation counts fall from 80,652 to 57,228 and from 111,402 to 86,713,
respectively. Fewer allocations therefore do not mean fewer allocated bytes.
Hot intervals again have zero net live-byte growth. The empty owner requests
176 bytes for every policy, versus 136 in the original. None of these byte
figures establishes a latency, allocator-retained-page, or process-RSS result.

The fourth benchmark executable is
`target/agent-logs/provider-cache-consolidated`, SHA-256
`f9d5b55d2ee06b7760c3797ddd5c9936ac04a4bfef91c8a8b77ef2642504ebe6`.
Its separate allocation executable has SHA-256
`9ec456039f935106b39be22fe4f9e4cdce334908a3d01bfe62346348b4f15a7d`.
Source hashes are in `provider-cache-consolidated-identities.log`; raw counts
are in `provider-cache-allocations-consolidated.tsv`. The earlier executable
and source snapshots remain available for comparisons against the original,
not merely against an intermediate regression.

The subsequent whole-host preflight found no core averaging the required 95%
idle threshold (maximum 94.23%). No comparative consolidated timing was started
from that preflight. The candidate is not selected: the predeclared time and
memory qualification remains open, and the miss-byte regression needs evaluation.

### Attribution and the bounded link-block experiment

The fourth implementation subsequently passed lling-llang's complete
`bindings-core` debug and release suites (2,817 tests each), strict all-target
Clippy, and all 421 duallity all-feature tests in both debug and release with
strict Clippy. A real dictionary-backed duallity benchmark now checks 48 cases
across classic, universal, generalized and FZF adapters. Its correctness-only
mode passes; no real-adapter timing result is inferred from that mode.

A separate allocation-size histogram repeated cyclic and deterministic shuffled
accesses three times with independently keyed maps. Capacity64 hot accesses
always requested one 160-byte snapshot and one 1,568-byte link chunk. At
capacity1024, link-chunk requests totalled 29,070,720 bytes for 10,000 cyclic hits
and 60,300,576 bytes for shuffled hits. Steady-miss link chunks requested
29,072,288 and 59,558,912 bytes, respectively. Exact hit, miss and eviction
counts were asserted; histogram collection performs no recursive allocation.
The hit-only control attributes this traffic to the link vector, not to its
leaf rows alone: internal vector chunks can use the same allocation size.
Size buckets on their own cannot distinguish those components. Repeating a
shuffled permutation tests physically scattered slots but still repeatedly
accesses the LRU head. A separate fixed-seed resident trace now exercises
arbitrary interior hits; all three traces belong in candidate qualification.

The consolidated capacity1024 eviction profile collected 3,028 user-cycle
samples with zero lost. Ownership-HAMT entry destruction and leaf copying were
17.55% and 16.90% self cost. Link-chunk `make_mut` was 3.10%; link-vector parent
copying was 1.97%. Therefore link copying is an allocation target, not evidence
that it dominates miss latency. The profile used a separate pinned, bounded
15-second process with DWARF call chains; no timing comparison was running.

The preregistered next intervention is limited to one 16-row primitive block
layout behind a persistent directory of reference-counted blocks. Keep the
ownership HAMT, publication protocol, policies and semantics unchanged. Tests
must verify old-root immutability, one staged copy per changed block, partial
block bounds, growth across block boundaries, and all existing slot-race tests.
Compare cyclic and shuffled allocation traffic and retained memory, not just
the favorable cyclic case. The directory may clone and later drop 64 block
references on a shared-leaf mutation at capacity1024, adding atomic traffic;
smaller copied blocks do not guarantee lower latency.

This is one conditional experiment, not a new default or a relaxed selection
criterion. Adoption still requires the original hot64 timing gate, unchanged-
path regression checks and explicit memory accounting against the original
baseline. The global `imbl/small-chunks` feature is not an isolated alternative:
it also changes HAMT and ordered-tree branching, including CacheAll behavior.
No global dependency configuration is changed for this experiment.

The block prototype passes 25 generic cache tests and nine raw-ABI tests. New
tests retain roots across modifications and growth, require sharing of exactly
the unchanged blocks, require reuse of the staged copy across repeated writes,
and reject indexing unused rows. End-to-end randomized-order and payload-sharing
tests also cover capacities 15/16/17 and 1023/1024/1025.

The first metadata-only allocation run reports:

| Measure | Original with MRU | Consolidated vector | 16-row blocks |
|---|---:|---:|---:|
| Hot64 requested bytes, 10,000 requests | 26,085,840 | 17,280,000 | 6,180,000 |
| Steady eviction64 requested bytes | 31,925,120 | 37,807,680 | 26,623,680 |
| Filled64 live increase | 17,144 | 16,936 | 15,848 |
| Hot1024 requested bytes, 10,000 requests | 40,971,080 | 30,999,800 | 11,620,000 |
| Steady eviction1024 requested bytes | 54,151,496 | 68,467,640 | 48,776,400 |
| Filled1024 live increase | 220,480 | 227,312 | 203,528 |

Independently keyed maps still vary in shape; the small filled-memory changes
are descriptive samples, not a paired statistical claim. The block candidate
also raises capacity-two hot traffic relative to the consolidated inline vector
(5,680,000 versus 1,600,000 requested bytes) and requests a 184-byte empty owner.
Small-capacity and unchanged-policy cases must remain in the final comparison.

Three-repeat histogram runs now include fixed-seed interior hits, not only
cyclic and physically shuffled LRU-head accesses. At capacity1024, interior-hit
requested bytes fall from 74,814,592 for the consolidated vector to 22,736,616
for blocks. Each block-directory copy contributes a 544-byte allocation that
can clone 64 block references. That potential atomic traffic remains a timing
risk; byte reductions alone do not select the candidate. Raw outputs use the
`provider-cache-blocks-` prefix under `target/agent-logs`.

The preserved original cache was then compiled into an isolated,
`publish = false` allocation harness with the same benchmark source and pinned
`imbl`, `arc-swap` and `ahash` versions. Its build directory is separate from the
live source graph. Three independently keyed repetitions produced these mean
requested-byte totals, rounded to the nearest byte, over 10,000 requests:

| Capacity and trace | Original with MRU | 16-row blocks |
|---|---:|---:|
| 64, cyclic hits | 26,114,773 | 6,180,000 |
| 64, physically shuffled hits | 26,202,133 | 11,180,800 |
| 64, interior hits | 24,938,325 | 12,527,352 |
| 64, cyclic misses | 31,237,067 | 26,738,853 |
| 64, physically shuffled misses | 31,381,173 | 31,136,933 |
| 1024, cyclic hits | 41,078,893 | 11,620,000 |
| 1024, physically shuffled hits | 41,148,253 | 18,979,200 |
| 1024, interior hits | 40,291,133 | 22,736,616 |
| 1024, cyclic misses | 54,458,944 | 48,537,995 |
| 1024, physically shuffled misses | 54,670,312 | 55,871,747 |

Scattered misses at capacity1024 request about 2.2% more bytes in these samples,
despite fewer allocations. This is not an across-the-board memory improvement;
the original comparison and both access patterns remain part of qualification.
These small repeated samples describe allocation behavior, not confidence
intervals for latency. The original fixture manifest, source snapshot and lock
file remain under `target/agent-logs/cache-allocation-reference`; outputs are
`provider-cache-original-attribution-all-traces.log` and
`provider-cache-original-matching-allocations.log`.
The standalone executable is retained as
`target/agent-logs/provider-cache-original-matching-allocation`, SHA-256
`7cc575b17af07651134cf7e8f16d4c132e482531ac8637072bc76ecaacbcdcc2`.
Its 70.5 MiB of rebuildable Cargo output was cleaned after preserving that
executable; the fixture manifest and lockfile remain for reproduction.

The block candidate subsequently passed all 2,820 `bindings-core` tests in
debug and release, strict all-target Clippy, and all 421 duallity all-feature
tests in each profile with strict Clippy. All 48 real-adapter benchmark smoke
cases still pass. The ten finite-model positive/negative checks and focused
read-only MathJax checks pass. These gates do not cover the wider feature or
WASM builds still in progress, or satisfy the pending latency criteria.

Retained timing executable SHA-256:
`a909c3ac3bf4fc4c4ac5597499a232e8191694760990c04c5fb9133718139b65`.
The allocation executable SHA-256 is
`24cfc67c077e76aeca31b21bad2eb627ef808741e5f5ce37dfc5191ddb4e5fbe`.
Full source identities are in `provider-cache-blocks-identities.log`.

The native all-features run found one stale integration assertion: a provider's
`LimitExceeded` was expected to become `ProviderError`. The already-committed
status contract in `d18a1236`, its direct-ABI unit tests and binding guide all
preserve limit and closed outcomes. The integration matrix now exercises all
nine non-success interop statuses through both information and arc callbacks,
with exact direct-ABI outcomes and error details. No production status behavior
was changed to make the test pass. The rerun passes all 3,127 tests, zero skips.

Both `wasm32-unknown-unknown` and `wasm32-wasip1` binding-core compilation checks
pass. These are compilation checks, not browser/WASI runtime or concurrency
tests. Rustdoc executes 47 examples successfully, including the new cache
example; 48 existing examples remain ignored, so this is not complete API-
example coverage. The raw gate outputs retain that distinction.

The first block A–B–B–A timing attempt selected CPU2 from the all-core preflight
and completed the original A1 pass. Its candidate B1 preflight then fell to
85.76% idle, so the wrapper stopped before any candidate sample. A1 alone does
not establish a comparison or speedup. The executable used the performance
governor and preference; timing/preflight files use the
`provider-cache-blocks-pair1-` prefix, with raw Criterion data in the corresponding
`target/criterion` directory. No campaign build or model-check process overlapped
that pass. The candidate remains unselected pending the remaining timing gates.

The final native all-features release suite also passes all 3,127 tests with
zero skips, followed by successful all-target/all-feature strict Clippy. The
dependency build reports two existing dead-code warning groups in libdictenstein's
persistent-ARTrie eviction implementation; this successful lling-llang gate is
not a claim that every dependency is warning-free. The logs are
`provider-cache-blocks-all-features-release.log` and
`provider-cache-blocks-all-features-clippy.log`.

### Frozen block candidate: CPU attribution and timing refusals

Two separate 15-second headless `perf` recordings sampled user-space cycles at
199 Hz, with DWARF call stacks, pinned to CPU27. The process scope limited memory
to 4 GiB with no swap and at most two CPUs of aggregate execution. The host was
loaded: these are within-process cost attributions, not qualified latency
comparisons. Neither recording lost samples.

| Workload | Samples | Dominant self costs |
|---|---:|---|
| Capacity64, cyclic resident hits, zero arcs | 3,016 | publication 17.51%; ArcSwap reader-debt handling 16.14%; LRU snapshot clone 7.36%; 16-row block copy-on-write 6.40% |
| Capacity1024, cyclic capacity-plus-one misses, zero arcs | 3,041 | ownership-map leaf clone 18.21%; map-entry destruction 17.21%; block-directory chunk copy-on-write 12.21%; directory chunk destruction 10.23% |

**Self cost** counts samples in a function itself, not its descendants. These
percentages describe different workloads and must not be added across rows.
The directory's 64 reference-counted block handles require cloning and releasing
their references when a shared directory chunk is copied. The latter two miss
costs total 22.44%, confirming the review's predicted reference-count overhead.
By contrast, copying the primitive 16-row block itself accounts for 1.45% of
that miss recording. Fewer allocated bytes therefore do not establish lower
latency: the representation trades some byte copying for reference-count work.
The candidate remains frozen pending paired timings; this profile alone does
not justify selecting it or starting another representation rewrite.

Raw recordings are `provider-cache-blocks-hot.perf` (24.356 MB) and
`provider-cache-blocks-miss.perf` (24.557 MB). Their corresponding
`-profile.log` files record collection, and `-profile-report.log` files retain
the symbol-level reports, under `target/agent-logs`.

The next B–A–A–B attempt stopped at the all-core preflight without timing either
binary: the highest three-second average idle fraction was 3.00% on CPU27.
A subsequent check reached only 76.82%; the next continuation's check reached
62.42% on CPU2 (47.50% overall). All fail the predeclared 95% threshold. Their
logs are `provider-cache-blocks-pair2-host.log`,
`provider-cache-blocks-resume-preflight.log` and
`provider-cache-blocks-turn-preflight.log`. They are refusal evidence, not
benchmark samples, and are not combined with the earlier original-only A1 pass
to manufacture a paired result.

After the canonical dependency-merge validation finished, the third attempt's
all-core check at 21:39:55 local time reached at most 94.55% idle on CPU1.
`provider-cache-blocks-pair3-host.log` preserves the three-second averages.
No campaign compilation or test process remained active, but this still misses
the 95% gate, so neither binary was timed. Both executable hashes remain the
previously recorded originals; no representation change occurred during the
intervening native and Julia correctness qualification.

The fourth check reached at most 78.36% idle on CPU0 and started no timing.
The fifth all-core check selected CPU6 at 95.19%, but its immediate candidate
B1 preflight fell to 92.86%. The B–A–A–B wrapper therefore exited with status75
before either executable ran. The evidence is retained in
`provider-cache-blocks-pair4-host.log`, `provider-cache-blocks-pair5-host.log`
and `provider-cache-blocks-pair5-b1-preflight.log`. A passing host scan is not
permission to bypass the per-pass gate; neither attempt supplies a timing
comparison.

### Expanded acceptance coverage on 7 September

The sixth timing attempt stopped before its first executable because CPU4 was
93.23% idle. The seventh selected CPU11 and completed candidate B1, original A1
and original A2, but refused candidate B2 at 92.72% idle. The completed passes
had preflights of 96.41%, 95.50% and 95.51%, respectively. The eighth all-core
check reached only 90.91%, so it collected no timing samples. None satisfies
the complete paired-selection gate; the block candidate remains unselected.
These attempts live under the canonical liblevenshtein-rust worktree's
`target/agent-logs/cache-blocks-pair6`, `cache-blocks-pair7` and
`cache-blocks-pair8`, not this integration worktree's log directory. The seventh
attempt's raw estimates are summarized in `partial-summary.tsv`, SHA-256
`ff9bbb31aab0c5906622fbfaaa8f6188d067da04280503711ba82475def8d772`.

After the expanded correctness gates completed, the ninth timing attempt
also stopped at its all-core preflight: its best three-second average was
94.08% idle on CPU3. Its `cache-blocks-pair9/host.preflight.log` is refusal
evidence only; no campaign build or test process overlapped that check.

The ABI lifecycle suite now adds eight independently retained readers over
sparse IDs, including the largest `u64` ID, while a controller clears and
replaces policy generations. Each reader checks information and three arc
pages, including ordered labels, targets, exact weight bits and reserved bytes.
Five policies cover CacheAll, NoCache and exact LRU capacities 1, 2, and 17.
The test checks quiescent hit/miss accounting against provider calls and verifies that
the provider is destroyed after the last resource retain, even while a cache
control remains alive. All ten lifecycle tests pass in debug and release with
strict Clippy. This complements the existing deterministic publication-race
tests; it does not claim a particular scheduler interleaving was forced.

The registered [residency benchmark](../../benches/shared_cache_residency.rs)
closes measurement-coverage gaps without changing the cache implementation.
It has 40 replay cases: capacities 1, 2, 64, and 1024; payloads of zero or 640 `u64`
elements; cyclic, shuffled and interior resident hits; and cyclic or shuffled
capacity-plus-one misses. Timing and allocation diagnostics use the same
[trace generator](../../benches/support/cache_trace.rs), tested by
[replay-contract regressions](../../tests/cache_benchmark_traces.rs).
The replay cursor persists across warmup and measured samples. Every case
checks exact hit, miss and eviction counts after measurement.

Another 72 cases isolate reclamation under CacheAll and LRU. Their setup seeds
one cache and retains zero, one or at most 32 returned payloads. Clear is timed
without refill or fixture destruction; last-reader release is timed separately
after an untimed clear. Criterion's per-iteration batching limits the fixture
to one populated cache at a time. Setup and untimed cleanup can dominate the
total harness wall time: that time is not the measured clear latency. The
ownership checks require cleared caches to relinquish every held payload and
held contents to remain unchanged.

Both preserved original-with-MRU and frozen block implementations pass all 112
correctness-only cases and strict benchmark Clippy. They are built from exact
source snapshots in separate, non-publishable fixture crates under the
canonical liblevenshtein-rust `target/agent-logs/cache-residency-reference` and
`cache-residency-blocks`. Their lockfiles differ only in fixture package name;
both use the same benchmark source and pinned dependencies. A first candidate
fixture import failed because a Rust path attribute changed child-module
resolution; byte-checked snapshots now preserve the normal module layout.
A first fixture Clippy invocation exposed a missing test-only `proptest`
dependency; both manifests now pin the same version as this source graph.
Neither diagnostic failure changed production code or supplied timing data.

The allocation benchmark's `--retirement` mode separately accounts for seed,
clear, last-reader release and empty-cache destruction. Each implementation
passes 44 fixtures, 176 accounting phases and 176 process-memory observations.
Summing the four measured live-byte deltas gives zero in every fixture. On
this 64-bit target, releasing 32 held 640-element payloads frees 165,376 requested
bytes in both implementations, including the holder-vector storage. The empty
post-clear root accounts for 136 requested bytes in the original and 184 in the
block candidate. These are allocator-requested sizes, not physical memory.

Linux process RSS ranged from 2,668 to 8,068 KiB for the reference run and 2,648 to
8,072 KiB for blocks. It includes the allocator, executable, runtime and procfs
observation buffer, and can remain high after all fixture allocations are
released. These small process-level differences do not establish a memory
speedup or regression. The combined validated summary is
`cache-residency-retirement-summary.tsv`, SHA-256
`7a1c69fcfcbd0d6b361ae1056ad46abb47aff773cac50cbe8b612161d71efadf`;
raw logs use `cache-residency-{reference,blocks}-shared-traces-*` in the same
canonical worktree log directory. No latency inference is drawn from these
instrumented allocation runs.

Reproduce the two diagnostic modes with registered Cargo targets:

```sh
cargo bench --offline --no-default-features --bench shared_cache_residency -- --test
cargo bench --offline --no-default-features --bench provider_cache_allocations -- --retirement
```

Run these inside the resource-limited, disk-backed execution scope described
below. Omit `--test` only when taking properly qualified uninstrumented timing
samples; the new cases have not yet supplied accepted paired latency results.

The expanded all-feature workspace passes 3,130 tests in both debug and release,
with zero skips, followed by strict all-target/all-feature Clippy. These results
include the ten ABI lifecycle tests and two trace-contract tests. The logs are
`provider-cache-acceptance-expanded-{debug,release,clippy}-qualified.log` in this
integration worktree. The dependency still reports the two previously observed
libdictenstein dead-code warning groups. The focused read-only MathJax audit
passes for both updated ledgers and the new Rust documentation; it does not use
`vinary-doc-lint` or modify any documentation.

### Completed paired measurements and tiny-cache regression

The prospectively reviewed `bounded-eligibility-v1` procedure retains the fixed
core, B1–A1–A2–B2 order, frozen executable identities and 95% idle threshold.
Before each pass it accepts the first eligible three-second window, with at
most ten windows and a 60-second monotonic deadline. Every attempted window,
wait and launch gap is retained; malformed telemetry and policy changes are
errors, not idle retries. No historical incomplete attempt is pooled with the
new runs. The original pair10 attempt also stopped before collecting samples:
its first pass had only 94.10% selected-core idle.

`cache-blocks-bounded-pair1` completed all four passes on CPU 7, with seven cases
and 20 samples per case per pass. Preflight idle was 99.67%, 99.67%, 98.01%, and
95.36%. Each first window qualified. Waiting took 3.00–3.01 seconds; consecutive
pass gaps were 3.02 seconds; qualification-to-executable gaps were 0.01–0.02
seconds. The table reports B1 relative to A1 and B2 relative to A2 separately.
Negative changes mean lower candidate latency, not increased throughput.

| Exported callback case | First latency change | Second latency change |
|---|---:|---:|
| CacheAll warm information, no arcs | -4.28% | -3.74% |
| LRU64 hot information, no arcs | -61.76% | -60.75% |
| LRU64 information plus arc page, no arcs | -58.58% | -59.19% |
| LRU64 hot information, 128 arcs | -64.04% | -63.31% |
| LRU64 information plus arc page, 128 arcs | -54.23% | -52.78% |
| LRU64 steady miss, no arcs | -18.44% | -19.06% |
| LRU64 steady miss, 128 arcs | -9.07% | -5.24% |

Whole-process maximum RSS was 50,572–50,968 KiB across these passes. GNU time
reported 564–597 involuntary context switches. These observations and the
preflight checks do not prove that every later sample was interference-free.
The reported changes are descriptive paired point estimates; they are not
confidence intervals for ratios or whole-query/Java speedups.

`cache-residency-bounded-replay1` then completed all 40 generic replay cases
in all four passes on CPU 9, producing 160 validated estimates of 20 samples
each. First-window idle was 98.01%, 96.04%, 99.34%, and 96.68%; wait and launch
gaps matched the preceding run's ranges. Across cyclic, shuffled and interior
hot access, capacity64 improved 61.84–69.68%, and capacity1024 improved
52.76–60.04%. Capacity-one hot results stayed within approximately 1.5%.

The broader controls exposed a real acceptance concern: capacity-two steady
misses regressed 3.37–8.79% in the first comparison and 10.91–15.88% in the
second. All four cyclic/shuffled and empty/640-element payload cases are
retained. The block candidate is therefore **not selected**; the favorable
larger-cache cases do not waive the unchanged regression gate. Reclamation
latency and real-adapter policy timing remain outstanding.

Separate five-second user-cycle profiles of `miss_cyclic/2/0` collected 2,565
reference samples and 2,568 block samples, with no lost samples. The candidate
profile includes link-vector indexing, link mutation, reference-counted
copy-on-write and allocation. This locates work; percentage differences do not
prove the isolated latency contribution of a function. No uninstrumented
measurement or compilation overlapped these profile runs.

Exact allocation diagnostics sharpen the hypothesis. Ten thousand empty-payload
capacity-two misses allocate 14,960,000 requested bytes in the reference and
16,560,000 in blocks, with 40,000 allocations in each. The reference's hot
capacity-two trace makes 30,000 allocations, versus 20,000 in blocks. Thus hot
and miss costs must be distinguished. Source inspection shows that every staged
tiny-cache update copies a 16-row link block, although only two rows are live:
48 of 384 primitive-row bytes on this target.

The next preregistered intervention keeps the first two primitive rows inline,
promoting once on the third resident to the existing block representation.
It predicts one fewer link-block allocation per tiny-cache update. Ownership
lookup, exact order, slot identity, generation checks and atomic publication
remain unchanged. New checks retain roots across promotion and force a failed
publication during promotion. Root layout must be measured because an added
representation discriminant could affect allocations under every policy.
This candidate must pass the full reference comparison and remaining controls;
its implementation alone is not evidence of a performance fix.

The [frozen input archive](evidence/shared-cache-inputs-2026-09-07.tar.zst)
contains the original and pre-inline block cache sources, pinned manifests and
lockfiles, shared diagnostic sources, Cargo configuration, toolchain declaration,
runner, strict complete-run summarizer and reproduction instructions. It is
36 KiB, SHA-256
`bcb91f88014483bc89c4fe1896373fb712aa8581c537b8855786afc9141db595`.
Its internal checksum manifest covers all 22 inputs, including both lockfiles.
The extracted files pass that manifest and the README passes the independent
read-only MathJax scan. Both relocated fixtures subsequently rebuilt from a
fresh extraction and passed all 112 correctness-only cases, with all source
checksums unchanged before and after. This nested extraction inherited a second
identical AES/SSE2 flag pair from its parent Cargo configuration. It proves
source and semantic reproduction, not byte-identical executable reproduction.
The archive deliberately contains no binaries or build outputs.
Its private fixture version `0.0.0` is non-publishable and unrelated to release
package versions. It reproduces generic-cache experiments, not the complete
provider-ABI or duallity graphs.

Raw timing, eligibility, resource and summary logs remain in the canonical
liblevenshtein-rust `target/agent-logs` directories named above. Targeted profiles
are `cache-tiny2-{reference,blocks}.perf` with corresponding `-report.log` files;
allocation tables are `cache-{reference,blocks}-steady-allocations.tsv`.
pgmcp progress10045 records the prospective protocol amendment, 10076 and10094
record completed comparisons, and10098 records the targeted experiment before
its implementation. The [raw measurement archive](evidence/shared-cache-measurements-2026-09-07.tar.zst)
preserves both complete comparisons, all Criterion samples and intervals,
eligibility windows, executable hashes, allocation tables and text profiles.
It is 140 KiB, SHA-256
`cf67b933c463b49f45c9bd4ce6ec251d6ba6d3f4fc37595afd1d669fbcef8b08`.
No package publication or primary-branch promotion occurred.

### Inline-link implementation checks

The inline candidate passed all 27 focused cache tests, including the new
retained-root promotion and failed-publication regressions. On this 64-bit
target, primitive links occupy 24 bytes, both old and new link containers occupy
72 bytes, LRU storage occupies 144 bytes and the snapshot occupies 152 bytes.
The full lling-llang all-feature workspace then passed 3,132 tests in debug and
3,132 in release, with no skips, followed by strict all-target/all-feature
Clippy. Duallity passed all 427 tests in each profile and strict Clippy against
the changed cache. These results supersede earlier counts for this source
candidate, not for arbitrary later dependency changes.

The isolated inline fixture passes all 112 benchmark correctness cases.
Capacity-two empty-payload misses now make 30,000 allocations and request
12,560,000 bytes over 10,000 requests: exactly one fewer allocation and 400
fewer bytes per request than blocks. Hot capacity-two requests make 10,000
allocations and request 1,680,000 bytes, half as many allocations as blocks.
The empty root still requests 184 bytes including reference-counting headers
and its generation marker. This supports the allocation hypothesis, but does
not alone prove the latency regression is fixed.

The actual lling-llang `bindings-core` graph compiles for
`wasm32-unknown-unknown` and `wasm32-wasip1`. These are compilation checks, not
WASM runtime or cross-target layout measurements. An earlier attempt to compile
the native-only fixture for browser WASM lacked the browser `getrandom` feature
already declared by the real library. That failed fixture command was retained;
the real graph was checked without a production-source workaround.

The first archive-rebuild attempts also found that an earlier extraction's
files were absent, although its directories and the source archive remained.
Both host and sandbox views agreed. No author or cleanup process was identified.
A new disk-backed extraction with current filesystem timestamps passed all
22 source checksums and rebuilt both fixtures successfully. Archive byte content
was not changed. The fresh path and logs are recorded in pgmcp progress10164.

Logs are `shared-cache-inline-{debug,release,clippy}.log` in each integration
worktree, plus `shared-cache-inline-{wasm32,wasip1}.log` in lling-llang.
The native fixture logs are `cache-inline-residency-smoke.log` and
`cache-inline-steady-allocations.log` in the canonical liblevenshtein-rust
log directory. The current generic timing executable has SHA-256
`b5307570ca06c2b914212c0e1e7e49d5d490ec81e2395250996ab66a2fe4bb32`.
Its complete paired replay comparison subsequently finished as described below.
The inline candidate remains unselected pending the remaining controls.

### Complete inline replay comparison

`cache-inline-bounded-replay1` completed all 40 cases in B1–A1–A2–B2 order
on CPU 9. Each pass used the first eligible window: selected-core idle was
98.34%, 98.67%, 99.67%, and 99.34%. Waits were 3.00–3.01 seconds, launch gaps
were 0.01–0.02 seconds, and all consecutive-pass gaps were 3.02 seconds.
The strict summarizer validated 160 estimates with 20 samples each.

The following ranges include both payload sizes and every applicable trace.
Negative changes indicate lower candidate latency relative to the frozen
original cache, not relative to the intermediate block candidate.

| Workload | First latency-change range | Second latency-change range |
|---|---:|---:|
| Capacity-two hot requests | -47.05% to -39.11% | -46.69% to -37.88% |
| Capacity-two steady misses | -1.06% to +6.35% | -15.86% to -6.90% |
| Capacity-64 hot requests | -67.65% to -61.34% | -67.70% to -61.48% |
| Capacity-1024 hot requests | -58.99% to -55.39% | -59.73% to -53.67% |
| Capacity-1024 shuffled misses | +2.50% to +3.54% | +4.10% to +5.67% |

The earlier greater-than-10% capacity-two miss regression is absent from this
complete comparison. No replay case exceeds that regression threshold, and
the capacity-64 hot cases exceed the preregistered 25% improvement threshold.
The allocation reduction and these timing observations support the inline
intervention, but do not imply that every miss became faster. The remaining
shuffled capacity-1024 overhead is explicitly retained above.

Whole-process maximum RSS was 50,976–51,392 KiB, with 3,584–4,071 involuntary
context switches. Some cases drifted between passes, particularly tiny-cache
misses. First-window eligibility is not evidence that every later sample was
interference-free. These are descriptive paired point-estimate changes, not
confidence intervals for ratios. All samples, individual estimate intervals,
unfavorable cases and timing gaps remain available; no cases were selectively
rerun or pooled with an earlier comparison.

The complete summary SHA-256 is
`d860f35382f97cc9a4d39b7c15b1c9b71402af548d7c937acfef7294e1727154`;
the paired-ratio table SHA-256 is
`649ecbeb74f5b9039f9dd5ba9c3e970b4d77c093cde7fe659813f467272bf046`.
pgmcp progress10175 records the terminal result and the next unchanged ABI
comparison. Reclamation, real-adapter policy timing and exact committed-graph
qualification still gate final selection.

The [inline input archive](evidence/shared-cache-inline-inputs-2026-09-07.tar.zst)
preserves this candidate and the original reference separately from the earlier
block experiment. Its SHA-256 is
`7bfbbff87a7b1cd50eb304832924d70a58f5085a97421e59dfe113aaa814612a`
and its size is 40 KiB. A fresh disk-backed extraction validated all 22 inputs,
including both dependency lockfiles. The [inline measurement archive](evidence/shared-cache-inline-measurements-2026-09-07.tar.zst)
has SHA-256
`0dcd5b20d1201a2ed05fb203a0fb0e1af0a46bc7e3b09f1639f0e37467211668`
and size 136 KiB. It retains the complete replay comparison, exact allocation
counts, and the explicitly incomplete ABI attempt described next.

`cache-inline-abi-bounded-pair1` collected B1, A1 and A2, but the selected core
failed every allowed eligibility window before B2. It exited 75 and is excluded
from acceptance. A1 had qualified on window eight and A2 on window nine; all
waits are retained. `cache-inline-abi-bounded-pair2` then exited 75 during its
initial all-core scan, before any sample: no core reached 95% idle. Neither
attempt is pooled with the successful replay run or counted as a completed
ABI comparison. The unchanged ABI and reclamation gates remain outstanding.

The finite shared-cache gate also ran again after the inline change: six
positive models and four expected-negative counterexamples passed in a 4 GiB,
no-swap, one-CPU systemd scope. The log
`shared-cache-inline-formal-20260907.log` has SHA-256
`0b33f22a76e0d9c1ee3060c72ad2e9d3a68849723cabc009ff2de7f63069bb5e`.
The logical models do not prove the new physical representation; retained-root,
promotion and failed-publication tests supply that separate evidence. This is
not a full Rocq or unbounded-state proof claim. pgmcp progress10189 and10193
record these outcomes and the matched real-adapter preparation.

### Canonical-worktree integration audit

A read-only comparison of the canonical worktrees, their committed heads,
local `origin/master` refs and the integration worktrees found unique pending
work. This used file-content hashes and Git tree entries, not just matching
filenames. No reset, stash, checkout, index update or source edit was performed
by the audit.

Here **divergent** means the live bytes match none of the compared committed or
integration copies. A reproducible reversion still expresses uncommitted intent
and cannot be silently discarded.

| Canonical worktree | Divergent tracked contents | Unique untracked contents | Unique deletion intents | Reversions reproducible from local `origin/master` |
|---|---:|---:|---:|---:|
| lling-llang | 54 | 36 | 6 | 1 |
| duallity | 3 | 0 | 0 | 1 |

The lling-llang deletion intents concern six
generated diagram artifacts; those files remain present in the compared Git
trees. The two reproducible reversions are lling-llang's
`src/semiring/signed/mod.rs` and duallity's `src/bindings.rs`.

Six paths overlap this cache work in lling-llang: `Cargo.toml`, `Cargo.lock`,
`docs/.mathlint-include.txt`, `docs/README.md`, `proofs/README.md` and
`proofs/verify.sh`. In duallity, `Cargo.toml` and `src/bindings.rs` overlap.
Every overlapping live pair differs. The committed canonical heads
`bc267978` and `09555556` are ancestors of the respective integration heads
`d18a1236` and `56bb0aa`, but that ancestry does not preserve these uncommitted
changes. Owner coordination is required before the canonical merges. Counts
describe this audit snapshot, not a guarantee that another agent has stopped
editing; recheck immediately before merging.

The subsequent authorship audit corrected an important interpretation of this
table: different whole-file hashes do not establish different semantic intent,
and dirty files do not establish ownership by a currently active peer. Recorded
edit calls show that this bindings campaign authored both canonical
release-workflow authentication changes on August25 and duallity's two
dictionary-cursor compatibility changes on August18. The current committed
duallity branch already preserves the authentication and cursor changes, while
also carrying newer version constraints, canonical dependency paths and release
guards. Reconciliation must retain that newer graph, not reintroduce the older
RC.4 constraints or nested interop path.

The modal-transition-system and weighted-pushdown implementations are different:
their historical authoring sessions and completion claims were located, but no
commit touching either implementation was found in the inspected local refs.
Their source, tests, proofs and documentation must therefore be preserved and
reviewed as complete change sets. Historical completion claims alone do not
establish current merge readiness. No canonical source was changed or discarded
during either audit.

## Reproduction and retained evidence

Register and build `provider_cache_benchmarks` before measuring it:

```sh
cargo bench --offline --no-default-features --features bindings-core \
  --bench provider_cache_benchmarks --no-run
```

Run the emitted executable under a memory-capped `systemd-run --user --scope`,
pin its CPU, capture host load and keep raw Criterion estimates. Use a
separate profiling pass: instrumentation is not part of the uninstrumented
timing comparison. Do not run competing compilation while measuring.

Local logs are under `target/agent-logs` in the integration worktree:

- `provider-cache-baseline-tests.log`, `provider-cache-before.log` and CPU-policy/preflight logs.
- `provider-cache-adversarial-tests.log` and `provider-cache-ownership-tests.log`.
- `provider-cache-candidate.log`, `provider-cache-candidate-dwarf.perf` and its text report.
- `provider-cache-uprof` and `provider-cache-uprof-report.log`.
- `provider-cache-generation-fastpath.log` and the HAMT experiment build logs.
- `provider-cache-counted-kernels.log` and its CPU preflight log.
- `provider-cache-abi-adversarial.log`, `provider-cache-lling-llang-debug-suite.log`
  and `provider-cache-doctest.log`.
- `provider-cache-formal-gate.log`; individual positive and negative model logs
  are under `target/formal-verification/logs`.

Duallity's corresponding all-features log is
`target/agent-logs/shared-cache-duallity-all-features-debug.log` in its integration
worktree. Temporary validation selects the integration dependency worktrees;
the cache must be qualified against an immutable committed implementation and
exact dependency revisions in a reproducible graph. Canonical manifest paths
and their named dependency checkouts must agree in that graph. Promotion to
the ordinary primary worktrees remains a separate downstream integration gate:
task 7784 must not wait for tasks 8156/8157, which themselves depend on 7784.
Current performance qualification is not yet that clean-commit graph gate.

## Remaining qualification within this task

Revalidate affected release and broader-feature suites, strict linting, native
examples and documentation against the selected committed implementation; the
earlier successful suites remain evidence for their recorded graphs. Compare cold,
warm and bounded-LRU workloads, sparse IDs and multithreaded access with
statistics overhead controlled. Record the final exact source graph and
benchmark limitations before claiming this task complete.
