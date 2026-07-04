# Topological Sort

Topological sorting orders the nodes of a directed acyclic graph (DAG) such that every edge goes from an earlier node to a later node. lling-llang uses topological order as a prerequisite for efficient path extraction algorithms ([Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009)).

## Terms & symbols

Defined centrally in [`../NOTATION.md`](../NOTATION.md); repeated locally for the terms this doc uses.

| Symbol | Meaning |
|---|---|
| `∣V∣` / `∣E∣` | number of nodes / edges of the DAG (cardinality bar `∣` = U+2223). |
| `u → v` | a directed edge; `u` must precede `v` in any topological order. |
| in-degree | the number of incoming edges of a node. |
| DAG | **D**irected **A**cyclic **G**raph — a graph with no directed cycle. |

## Concepts

### What is Topological Sort?

Given a DAG, a **topological order** is a linear sequence of nodes where for every edge u→v, node u appears before node v in the sequence.

```
Graph:          Topological Orders (multiple valid):
    A → B       A, B, C, D
    ↓   ↓       A, C, B, D
    C → D

A must come before B, C (it points to them)
B must come before D
C must come before D
```

### Why Topological Sort?

Topological order enables **dynamic programming** on DAGs:
- Process nodes in dependency order
- When visiting a node, all predecessors already processed
- Allows single-pass O(V+E) algorithms

Key algorithms requiring topological order:
- **Viterbi**: Shortest/best path
- **N-best**: Top-k paths
- **Path counting**: Number of paths from start to end
- **Forward-backward**: Probability computation

### Cycle Detection

Topological sort fails if the graph contains cycles. A cycle means no valid ordering exists (a node would need to come before itself).

```
     A → B
     ↑   ↓    ← Cycle: no valid topological order
     C ← D
```

Lattices built with `LatticeBuilder` are guaranteed acyclic (edges only go from lower to higher positions).

### Core Functions

| Function | Description |
|----------|-------------|
| `topological_sort()` | Kahn's algorithm for DAG ordering |
| `is_acyclic()` | Check for cycles using DFS |
| `count_paths()` | Count paths using topological DP |
| `reachable_nodes()` | Find all reachable nodes (BFS) |
| `path_exists()` | Check path existence |

## Usage

### Lattice Topological Order

```rust
use lling_llang::lattice::LatticeBuilder;
use lling_llang::backend::HashMapBackend;
use lling_llang::semiring::TropicalWeight;
use lling_llang::lattice::EdgeMetadata;

let backend = HashMapBackend::new();
let mut builder = LatticeBuilder::new(backend);

builder.add_correction(0, 1, "the", TropicalWeight::one(), EdgeMetadata::default());
builder.add_correction(1, 2, "dog", TropicalWeight::one(), EdgeMetadata::default());
builder.add_correction(1, 2, "cat", TropicalWeight::one(), EdgeMetadata::default());
builder.add_correction(2, 3, "runs", TropicalWeight::one(), EdgeMetadata::default());

let mut lattice = builder.build(3);

// Get topological order (computed and cached on first call)
if let Some(order) = lattice.topological_order() {
    println!("Order: {:?}", order);  // [NodeId(0), NodeId(1), NodeId(2), NodeId(3)]
}
```

### Checking for Cycles

```rust
use lling_llang::lattice::algorithms::is_acyclic;

if is_acyclic(lattice.nodes(), lattice.edges()) {
    println!("Graph is acyclic");
} else {
    println!("Graph contains a cycle");
}

// Or via topological_order (returns None if cyclic)
match lattice.topological_order() {
    Some(order) => println!("Acyclic, {} nodes", order.len()),
    None => println!("Contains cycle"),
}
```

### Counting Paths

```rust
use lling_llang::lattice::algorithms::count_paths;

// Count all paths from start to end
match count_paths(&mut lattice) {
    Some(n) => println!("Found {} paths", n),
    None => println!("Overflow or cycle"),
}
```

### Finding Reachable Nodes

```rust
use lling_llang::lattice::algorithms::{reachable_nodes, path_exists};

// All nodes reachable from start
let reachable = reachable_nodes(&lattice, lattice.start());
println!("Reachable: {} nodes", reachable.len());

// Check if specific path exists
if path_exists(&lattice, lattice.start(), lattice.end()) {
    println!("Path from start to end exists");
}
```

## Kahn's Algorithm

lling-llang uses **Kahn's algorithm** ([Kahn 1962](#references)) for topological sort.

![Kahn's algorithm on a diamond DAG: nodes 0,1,2,3 each annotated with their in-degree (0,1,1,2); node 0 has in-degree 0 and is ready first, node 3 is the double-ring sink; a bold green spine 0→1→3 marks one valid linearization and a record inset lists the emitted order 0,1,2,3](../diagrams/algorithms/topological-sort.svg)

*Each node is labelled with its in-degree; Kahn seeds the queue with the in-degree-0 node (green-filled `0`), then peels nodes off and decrements successors' in-degrees. The bold green arcs trace one valid linearization; grey arcs are the alternative diamond edges; the record inset is the emitted order. The diamond admits two orders (`0,1,2,3` and `0,2,1,3`) — topological order is not unique.*

<details><summary>Text view</summary>

```text
        A(in=0) ─► B(in=1) ─┐
           │                ├─► D(in=2)   (double-ring sink)
           └────► C(in=1) ──┘

  seed queue with in-degree-0 nodes → [A];  emitted order: 0, 1, 2, 3
```

</details>

### How It Works

Kahn's algorithm is a *peeling* process: the loop invariant is that a node is appended to the output only once **all** its predecessors have already been emitted, which is exactly when its remaining in-degree reaches `0`. The literate chunks below name the two phases — `` `⟨ seed the ready queue ⟩` `` and `` `⟨ peel a ready node ⟩` `` — and `` `⟨ kahn topological sort ⟩` `` assembles them with the cycle check.

1. **Count in-degrees**: For each node, count incoming edges
2. **Initialize queue**: Add nodes with in-degree 0 (no dependencies)
3. **Process**: Remove node from queue, add to result, decrement neighbors' in-degrees
4. **Repeat**: When neighbor's in-degree becomes 0, add to queue
5. **Check**: If result has all nodes, DAG is valid; otherwise, cycle exists

```text
⟨ seed the ready queue ⟩ ≡
    for v in V:  in_degree[v] ← ∣incoming(v)∣
    queue ← [ v ∈ V : in_degree[v] = 0 ]      // no unmet dependencies
    result ← [ ]
```

```text
⟨ peel a ready node ⟩ ≡
    v ← queue.pop();  result.push(v)
    for each edge v → u:                       // O(1) target lookup
        in_degree[u] ← in_degree[u] − 1
        if in_degree[u] = 0:  queue.push(u)    // u's predecessors all emitted
```

```text
⟨ kahn topological sort ⟩ ≡
    ⟨ seed the ready queue ⟩
    while queue not empty:  ⟨ peel a ready node ⟩
    if ∣result∣ = ∣V∣:  return result          // valid linear order
    else:               return ⊥ (cycle)       // some node never reached in-degree 0
```

Each node is pushed and popped exactly once, and each edge is relaxed exactly once when its source is peeled, giving `` `O(∣V∣ + ∣E∣)` `` time. If a cycle exists, every node on it keeps a positive in-degree forever, so it is never enqueued and `` `∣result∣ < ∣V∣` `` — that size check **is** the cycle detector.

```
Example: A→B, A→C, B→D, C→D

Step 1: in_degree = {A:0, B:1, C:1, D:2}
        queue = [A]

Step 2: Pop A, result = [A]
        Decrement B,C: in_degree = {B:0, C:0, D:2}
        queue = [B, C]

Step 3: Pop B, result = [A, B]
        Decrement D: in_degree = {D:1}
        queue = [C]

Step 4: Pop C, result = [A, B, C]
        Decrement D: in_degree = {D:0}
        queue = [D]

Step 5: Pop D, result = [A, B, C, D]
        queue = []

Result: [A, B, C, D] (valid topological order)
```

### Implementation

```rust
pub fn topological_sort<W: Semiring>(
    nodes: &[Node],
    edges: &[Edge<W>]
) -> Option<Vec<NodeId>> {
    if nodes.is_empty() {
        return Some(Vec::new());
    }

    let n = nodes.len();

    // Build edge_id -> target lookup table: O(E)
    let edge_targets: Vec<NodeId> = edges.iter().map(|e| e.target).collect();

    let mut in_degree: Vec<usize> = nodes.iter()
        .map(|node| node.incoming.len())
        .collect();

    let mut queue: Vec<NodeId> = Vec::with_capacity(n);
    let mut result: Vec<NodeId> = Vec::with_capacity(n);

    // Start with nodes that have no incoming edges
    for node in nodes {
        if node.incoming.is_empty() {
            queue.push(node.id);
        }
    }

    while let Some(node_id) = queue.pop() {
        result.push(node_id);

        // Decrease in-degree for all neighbors
        if let Some(node) = nodes.get(node_id.0 as usize) {
            for &edge_id in &node.outgoing {
                // O(1) lookup instead of O(V) scan
                let target = edge_targets[edge_id.0 as usize];
                let idx = target.0 as usize;
                in_degree[idx] -= 1;
                if in_degree[idx] == 0 {
                    queue.push(target);
                }
            }
        }
    }

    if result.len() == n {
        Some(result)
    } else {
        None  // Cycle detected
    }
}
```

### Complexity

- **Time**: `` `O(∣V∣ + ∣E∣)` `` — each node and edge visited exactly once
- **Space**: `` `O(∣V∣ + ∣E∣)` `` for the edge target lookup table

The `` `O(1)` `` edge target lookup is a key optimization. Without it, finding the target of each edge would require an `` `O(∣V∣)` `` scan, making the overall algorithm `` `O(∣V∣ × ∣E∣)` ``.

## Cycle Detection with DFS

An alternative approach uses **depth-first search** with three-coloring:

### Node Colors

| Color | Meaning |
|-------|---------|
| White (0) | Not yet visited |
| Gray (1) | Currently being processed (in recursion stack) |
| Black (2) | Completely processed |

### Cycle Detection Rule

A **back edge** (edge to a gray node) indicates a cycle:

```rust
pub fn is_acyclic(nodes: &[Node], edges: &[Edge<impl Semiring>]) -> bool {
    // Build adjacency list
    let mut adj: Vec<Vec<NodeId>> = vec![Vec::new(); nodes.len()];
    for edge in edges {
        adj[edge.source.0 as usize].push(edge.target);
    }

    // 0 = white, 1 = gray, 2 = black
    let mut color: Vec<u8> = vec![0; nodes.len()];

    fn dfs(node: usize, adj: &[Vec<NodeId>], color: &mut [u8]) -> bool {
        color[node] = 1;  // Gray

        for &neighbor in &adj[node] {
            let idx = neighbor.0 as usize;
            match color[idx] {
                1 => return false,  // Back edge - cycle!
                0 => {
                    if !dfs(idx, adj, color) {
                        return false;
                    }
                }
                _ => {}  // Already black, skip
            }
        }

        color[node] = 2;  // Black
        true
    }

    // Check all nodes (handles disconnected graphs)
    for i in 0..nodes.len() {
        if color[i] == 0 && !dfs(i, &adj, &mut color) {
            return false;
        }
    }

    true
}
```

## Path Counting

Given a DAG in topological order, count paths using dynamic programming:

### Algorithm

1. Set `count[start] = 1` (one path: the empty path)
2. For each node in topological order:
   - For each outgoing edge to neighbor:
   - `count[neighbor] += count[current]`
3. Return `count[end]`

```rust
pub fn count_paths<W: Semiring, B: LatticeBackend>(
    lattice: &mut Lattice<W, B>
) -> Option<usize> {
    let topo_order = lattice.topological_order()?.to_vec();

    let n = lattice.num_nodes();
    let mut path_count: Vec<usize> = vec![0; n];

    // Start node has 1 path
    path_count[lattice.start().0 as usize] = 1;

    // Process in topological order
    for node_id in topo_order {
        let current_count = path_count[node_id.0 as usize];
        if current_count == 0 {
            continue;
        }

        for edge in lattice.outgoing_edges(node_id) {
            let target_idx = edge.target.0 as usize;
            path_count[target_idx] = path_count[target_idx]
                .checked_add(current_count)?;
        }
    }

    Some(path_count[lattice.end().0 as usize])
}
```

### Complexity

- **Time**: `` `O(∣V∣ + ∣E∣)` `` after topological sort
- **Space**: `` `O(∣V∣)` `` for count array

### Example

```
Diamond lattice: 0 → 1 → 3
                   ↘ 2 ↗

Topological order: [0, 1, 2, 3]

count = [1, 0, 0, 0]  (initially)

Process 0: count[1] += 1, count[2] += 1
           count = [1, 1, 1, 0]

Process 1: count[3] += 1
           count = [1, 1, 1, 1]

Process 2: count[3] += 1
           count = [1, 1, 1, 2]

Process 3: (no outgoing)

Result: count[3] = 2 paths
```

## Caching

### Lattice Caching

The `Lattice` struct caches topological order:

```rust
pub struct Lattice<W: Semiring, B: LatticeBackend> {
    // ...
    topo_order: Option<Vec<NodeId>>,  // Cached order
}

impl<W, B> Lattice<W, B> {
    pub fn topological_order(&mut self) -> Option<&[NodeId]> {
        if self.topo_order.is_none() {
            // Compute and cache
            self.topo_order = topological_sort(&self.nodes, &self.edges);
        }
        self.topo_order.as_deref()
    }
}
```

First call computes the order; subsequent calls return cached result.

### Invalidation

The cache is invalidated if the lattice structure changes (adding edges). For immutable lattices (most common case), this is not a concern.

## Details

### Why Kahn Over DFS?

lling-llang uses Kahn's algorithm rather than DFS-based topological sort because:

1. **Non-recursive**: Avoids stack overflow on deep graphs
2. **Better locality**: Queue-based access patterns
3. **Simpler cycle detection**: Just check result size

DFS-based sort would reverse post-order, requiring an extra reversal step.

### Edge Target Lookup Optimization

A key optimization in the implementation:

```rust
// Build lookup table once: O(E)
let edge_targets: Vec<NodeId> = edges.iter().map(|e| e.target).collect();

// Later: O(1) lookup instead of O(V) scan
let target = edge_targets[edge_id.0 as usize];
```

Without this, each edge lookup would scan all nodes, degrading performance to O(V × E).

### Multiple Valid Orders

Topological sort is not unique. For the diamond graph:
- `[0, 1, 2, 3]` is valid (both paths work)
- `[0, 2, 1, 3]` is also valid

Kahn's algorithm returns one valid order based on queue processing order (LIFO in this implementation).

### Handling Disconnected Graphs

Both `topological_sort` and `is_acyclic` handle disconnected graphs:

```rust
// topological_sort: starts with ALL zero-in-degree nodes
for node in nodes {
    if node.incoming.is_empty() {
        queue.push(node.id);
    }
}

// is_acyclic: DFS from ALL unvisited nodes
for i in 0..nodes.len() {
    if color[i] == 0 {
        // Start new DFS from this component
    }
}
```

## Common Patterns

### Validate Before Processing

```rust
// Check lattice is valid before expensive operations
let order = lattice.topological_order();
if order.is_none() {
    return Err("Lattice contains cycle");
}

// Now safe to use path extraction
let result = viterbi(&mut lattice);
```

### Forward Pass with DP

```rust
let order = lattice.topological_order()
    .expect("lattice must be acyclic");

let mut dp = vec![initial_value; lattice.num_nodes()];

for &node_id in order {
    // Process in topological order
    for edge in lattice.outgoing_edges(node_id) {
        dp[edge.target] = combine(dp[node_id], edge.weight);
    }
}
```

### Backward Pass

For algorithms like backward probability:

```rust
let order = lattice.topological_order()
    .expect("acyclic")
    .to_vec();

let mut backward = vec![W::zero(); lattice.num_nodes()];
backward[lattice.end().0 as usize] = W::one();

// Reverse topological order
for &node_id in order.iter().rev() {
    for edge in lattice.outgoing_edges(node_id) {
        backward[node_id.0 as usize] = backward[node_id.0 as usize]
            .plus(&edge.weight.times(&backward[edge.target.0 as usize]));
    }
}
```

## Related Topics

- [Path Extraction](path-extraction.md): Viterbi, N-best, beam search
- [Lattices](../architecture/lattices.md): Lattice data structure
- [Composition](composition.md): Graph composition algorithms

## References

- [Mohri 2009](../BIBLIOGRAPHY.md#ref-mohri2009) — *Weighted Automata Algorithms*: shortest-distance and forward/backward computations on acyclic weighted graphs rely on a topological order so each state is settled in a single pass; the `` `O(∣V∣ + ∣E∣)` `` dynamic-programming pattern this doc enables.
- **[Kahn 1962]** Kahn, A. B. (1962). *Topological Sorting of Large Networks.* Communications of the ACM 5(11):558–562. [doi:10.1145/368996.369025](https://doi.org/10.1145/368996.369025) — the in-degree-peeling algorithm implemented here.
