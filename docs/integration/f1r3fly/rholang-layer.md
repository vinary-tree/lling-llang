# Rholang Concurrency Layer

The Rholang Concurrency Layer enables distributed, concurrent lattice processing using Rholang's process calculus.

## Overview

**Rholang** (Reflective Higher-Order Language) is a concurrent programming language based on the rho-calculus. The `RholangLayer` enables:

- **Par composition**: Process lattice regions in parallel
- **Channels**: Communicate between processing stages
- **Joins**: Synchronize parallel computations
- **Unforgeable names**: Secure inter-process communication

In the integration flow, Rholang is the **concurrency substrate** wrapping the
pipeline: it partitions the lattice, runs the [MeTTaIL](mettail-layer.md),
[MORK](mork-layer.md), and [MeTTaTron](mettatron-layer.md) passes across regions
and nodes, and joins the results — with [PathMap](pathmap-backend.md) sharing
large structures by content hash.

![F1R3FLY layer integration overview: a candidate lattice is type-filtered by the MeTTaIL layer, rule-filtered by MORK, rewritten by MeTTaTron-compiled specs, and parallelized by the Rholang layer, with PathMap as the content-addressed substrate beneath; the output is a pruned, reweighted lattice.](../../diagrams/integration/mettail-mork-rholang.svg)

*Green = the candidate lattice; amber = the MeTTaIL/MORK type-and-rule filters;
purple = MeTTaTron compilation and Rholang concurrency; blue = the PathMap
substrate; grey = the final lattice. All four layers are forward-looking
integration **targets**. Dotted edges are cross-layer dependencies.*

<details><summary>Text view</summary>

```text
candidate lattice
      │  all paths
      ▼
[ MeTTaIL type layer ]  ── type-consistent paths ──▶ [ MORK rule layer ]
                                                            │ rule-valid paths
                                                            ▼
   pruned + reweighted  ◀── merged regions ── [ Rholang ] ◀── [ MeTTaTron ]
        lattice                                    │  compiled pass
                                                   ▼
                                  PathMap (persist · share by hash)
```

</details>

## Rholang Basics

### Process Calculus

Rholang is based on process algebra:

```rholang
// Parallel composition
P | Q                    // P and Q run concurrently

// Send on channel
channel!(value)          // Send value on channel

// Receive from channel
for (x <- channel) { P } // Receive x, then run P

// New channel
new channel in { P }     // Create fresh channel, run P
```

### Pattern Matching

```rholang
// Match on received value
for (@{pattern} <- channel) {
  // pattern matched
}

// Multiple patterns
for (@x <- ch1; @y <- ch2) {
  // Both received
}
```

## Lattice Processing Patterns

### Parallel Partitioning

Process lattice partitions concurrently:

```rholang
// Partition lattice and process in parallel
new result in {
  // Split lattice into regions
  for (@lattice <- input) {
    new r1, r2, r3 in {
      // Process each region in parallel
      region1!(process(partition(lattice, 0, 3))) |
      region2!(process(partition(lattice, 1, 3))) |
      region3!(process(partition(lattice, 2, 3))) |

      // Join results
      for (@res1 <- r1; @res2 <- r2; @res3 <- r3) {
        result!(merge(res1, res2, res3))
      }
    }
  }
}
```

### Pipeline Parallelism

Run pipeline stages concurrently on different inputs:

```rholang
// Three-stage pipeline
new stage1Out, stage2Out in {
  // Stage 1: Spelling
  for (@input <- inputs) {
    stage1Out!(spelling_correct(input))
  } |

  // Stage 2: Grammar (consumes stage1 output)
  for (@spelled <- stage1Out) {
    stage2Out!(grammar_correct(spelled))
  } |

  // Stage 3: Semantic (consumes stage2 output)
  for (@grammared <- stage2Out) {
    output!(semantic_correct(grammared))
  }
}
```

### Work Stealing

Dynamic load balancing:

```rholang
// Work queue pattern
new workQueue, results in {
  // Submit work items
  for (@items <- input) {
    for (item <- items) {
      workQueue!(item)
    }
  } |

  // Worker pool
  new worker in {
    contract worker(@_) = {
      for (@item <- workQueue) {
        results!(process(item)) |
        worker!(Nil)  // Request more work
      }
    } |
    // Start workers
    worker!(Nil) | worker!(Nil) | worker!(Nil) | worker!(Nil)
  }
}
```

## Using the Layer

### Basic Usage

```rust
// Future API
use lling_llang::layers::RholangLayer;

let program = r#"
    new result in {
        for (@lattice <- input) {
            new r1, r2 in {
                // Process halves in parallel
                r1!(process_first_half(lattice)) |
                r2!(process_second_half(lattice)) |

                for (@half1 <- r1; @half2 <- r2) {
                    result!(merge(half1, half2))
                }
            }
        }
    }
"#;

let layer = RholangLayer::new(program)?;
let processed = layer.apply_parallel(&lattice)?;
```

### Parallel Apply

```rust
// Apply layer to multiple lattices in parallel
let lattices: Vec<Lattice<W, B>> = load_lattices()?;

let layer = RholangLayer::parallel_map(process_fn);
let results = layer.apply_all(&lattices)?;
```

### Distributed Processing

```rust
// Distribute across nodes
let layer = RholangLayer::builder()
    .program(program)
    .nodes(&["node1:8080", "node2:8080", "node3:8080"])
    .partition_strategy(PartitionStrategy::ByPosition)
    .build()?;

// Process distributes work automatically
let result = layer.apply(&large_lattice)?;
```

## Layer Properties

```rust
impl<W: Semiring, B: LatticeBackend> CorrectionLayer<W, B> for RholangLayer {
    fn name(&self) -> &str {
        "rholang-concurrent"
    }

    fn estimated_reduction(&self) -> f64 {
        1.0  // Doesn't reduce paths, just parallelizes
    }

    fn can_apply(&self, lattice: &Lattice<W, B>) -> bool {
        // Check lattice is partitionable
        lattice.num_nodes() >= self.min_partition_size()
    }

    // Additional method for parallel application
    fn supports_parallel(&self) -> bool {
        true
    }
}
```

## Communication Patterns

### Request-Reply

```rholang
// Client
new reply in {
  server!((request, *reply)) |
  for (@response <- reply) {
    // Handle response
  }
}

// Server
contract server(@(req, replyTo)) = {
  replyTo!(process(req))
}
```

### Scatter-Gather

```rholang
new gather in {
  // Scatter: send to multiple workers
  worker1!(task1, *gather) |
  worker2!(task2, *gather) |
  worker3!(task3, *gather) |

  // Gather: collect all results
  for (@r1 <- gather; @r2 <- gather; @r3 <- gather) {
    output!(combine(r1, r2, r3))
  }
}
```

### Broadcast

```rholang
// Broadcast to all subscribers
contract broadcast(@message) = {
  for (@subscriber <- subscribers) {
    subscriber!(message)
  }
}
```

## Fault Tolerance

### Supervision

```rholang
// Supervisor pattern
new supervisor in {
  contract supervisor(@worker, @task) = {
    new result, timeout in {
      // Start worker with timeout
      worker!(task, *result) |
      delay(5000, timeout!(Nil)) |

      // Wait for result or timeout
      select {
        @res <- result => output!(res)
        _ <- timeout => {
          // Restart worker
          supervisor!(spawn_worker(), task)
        }
      }
    }
  }
}
```

### Checkpointing

```rholang
// Periodic checkpointing
new checkpoint in {
  contract process_with_checkpoint(@state, @remaining) = {
    match remaining {
      [] => output!(state)
      [item | rest] => {
        new newState in {
          newState!(update(state, item)) |
          for (@s <- newState) {
            checkpoint!(s) |  // Save checkpoint
            process_with_checkpoint!(s, rest)
          }
        }
      }
    }
  }
}
```

## Integration with PathMap

### Distributed State

```rholang
// Store intermediate results in PathMap
new pathmap in {
  for (@lattice <- input) {
    new intermediate in {
      // Process and store
      intermediate!(process_stage1(lattice)) |

      for (@result <- intermediate) {
        // Persist to PathMap
        pathmap!("put", hash(result), result) |

        // Continue processing
        output!(process_stage2(result))
      }
    }
  }
}
```

### Content-Addressed Sharing

```rholang
// Share large structures by hash
new share in {
  contract share(@data) = {
    new hash in {
      hash!(content_hash(data)) |
      for (@h <- hash) {
        pathmap!("put", h, data) |
        reference!(h)  // Return hash, not data
      }
    }
  }
}
```

## Performance Considerations

### Granularity

Choose appropriate task granularity:

| Granularity | Overhead | Parallelism | Use Case |
|-------------|----------|-------------|----------|
| Fine (per-edge) | High | Maximum | Small lattices |
| Medium (per-region) | Medium | Good | Most cases |
| Coarse (per-lattice) | Low | Limited | Large batches |

### Memory Locality

```rholang
// Partition by locality
new localize in {
  contract localize(@lattice, @nodeId) = {
    // Keep related edges together
    for (@partition <- partition_by_locality(lattice, nodeId)) {
      local_process!(partition)
    }
  }
}
```

### Backpressure

```rholang
// Bounded queue with backpressure
new queue, slots in {
  // Initialize slots
  slots!(Nil) | slots!(Nil) | slots!(Nil) |

  // Producer: wait for slot
  contract produce(@item) = {
    for (_ <- slots) {
      queue!(item)
    }
  } |

  // Consumer: return slot
  contract consume(@_) = {
    for (@item <- queue) {
      slots!(Nil) |
      process!(item)
    }
  }
}
```

## Advanced Features

### Unforgeable Names

Secure communication:

```rholang
// Create unforgeable channel for secure communication
new secret in {
  // Only this scope can use secret
  trusted_process!(*secret) |

  for (@message <- secret) {
    // Message definitely from trusted_process
  }
}
```

### Reflection

Introspect running processes:

```rholang
// Quote and unquote processes
@P               // Quote: turn process into name
*name            // Unquote: turn name into process

// Serialize process
for (@serialized <- serialize(*process)) {
  storage!(serialized)
}
```

### Namespaces

Organize code with namespaces:

```rholang
// Define namespace
new lling_llang in {
  contract lling_llang("viterbi", @lattice, return) = {
    return!(compute_viterbi(lattice))
  } |

  contract lling_llang("nbest", @lattice, @n, return) = {
    return!(compute_nbest(lattice, n))
  }
}

// Use namespace
lling_llang!("viterbi", my_lattice, *result)
```

## Debugging

### Trace Execution

```rust
let layer = RholangLayer::new(program)?
    .with_trace(true);

let result = layer.apply(&lattice)?;

// Print execution trace
for event in layer.execution_trace() {
    match event {
        Event::Send(ch, val) => println!("Send {} on {}", val, ch),
        Event::Receive(ch, val) => println!("Recv {} from {}", val, ch),
        Event::NewChannel(ch) => println!("New channel {}", ch),
        Event::Parallel(p1, p2) => println!("Par {} | {}", p1, p2),
    }
}
```

### Deadlock Detection

```rust
let layer = RholangLayer::new(program)?
    .with_deadlock_detection(true);

match layer.apply(&lattice) {
    Ok(result) => println!("Success"),
    Err(RholangError::Deadlock(waiting)) => {
        println!("Deadlock detected:");
        for (proc, channel) in waiting {
            println!("  {} waiting on {}", proc, channel);
        }
    }
    Err(e) => println!("Error: {}", e),
}
```

## Current Status

**Status**: Planned

The `RholangLayer` is planned but not yet implemented. Current blockers:

1. Rholang runtime integration not complete
2. Lattice serialization for message passing
3. Distributed coordination protocol

## Next Steps

- [Vision](vision.md): F1R3FLY.io integration overview
- [PathMap Backend](pathmap-backend.md): Distributed storage
- [MeTTaTron Layer](mettatron-layer.md): Compiled pipelines
- [Layers](../../architecture/layers.md): Layer architecture
