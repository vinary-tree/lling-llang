# LlingLlang.jl

LlingLlang.jl builds and lazily composes weighted finite-state transducers and
lets Julia applications provide custom immutable transducers and weight
semirings through Vinary Tree's versioned resource ABI. It also consumes
host-defined lattice values published by LLattice.jl. Start with the package
[README](https://github.com/vinary-tree/lling-llang/tree/master/bindings/julia/LlingLlang#readme)
for ownership, concurrency, security, and complete examples.

## Consume a host-defined lattice

LLattice.jl owns the provider implementation; `DynamicLatticeValue` is
lling-llang's checked consumer. Import takes an independent retain, and every
join, meet, or fold produces another independently owned value.

```julia
using LlingLlang
import LLattice

encode(value) = Vector{UInt8}(codeunits(string(value.value)))
hosts = [LLattice.provider(LLattice.MaxMin(value);
    domain_id="demo.maxmin.v1..", encode=encode) for value in (2, 7, 4)]
values = [dynamic_lattice_value(host.resource) for host in hosts]
close.(hosts)

maximum = lattice_join_many(values[1], values[2:3])
minimum = lattice_meet(values[1], values[2])
@assert String(lattice_stable_bytes(maximum)) == "7"
@assert String(lattice_stable_bytes(minimum)) == "2"
validate_lattice_laws(values)

close(maximum); close(minimum); close.(values)
```

The 16-byte domain identifier names both the encoding and the algebra. Values
from different domains are rejected before a foreign callback runs. Law
validation checks representative samples and can falsify, but not prove, the
universal lattice laws. Julia handles are same-thread consumers; the Rust
adapter uses fail-fast atomic admission and does not hold a mutex while host
code executes.

## Public API

```@autodocs
Modules = [LlingLlang]
Private = false
```
