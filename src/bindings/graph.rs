//! Incremental, bounded capture of a reachable scalar WFST graph.
//!
//! Provider state counts are not consulted. Stable local state IDs are
//! assigned in breadth-first order, with each state's arcs kept in provider
//! order. One poll performs at most `work_per_call` provider callbacks; each
//! arc-page callback transfers at most `VT_RECOMMENDED_ARC_BATCH` arcs.

use super::{scalar_zero, BindingError, CapturedWfst, StateHeader, VT_RECOMMENDED_ARC_BATCH};
use std::collections::{HashMap, VecDeque};
use vinary_tree_interop::{VtResource, VtUnitDomain, VtWeightDomain, VtWfstArc};

/// Bounds for discovery, allocation, and provider callback scheduling.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScalarGraphConfig {
    pub max_states: usize,
    pub max_arcs: usize,
    pub max_work: usize,
    pub work_per_call: usize,
}

impl ScalarGraphConfig {
    pub fn validate(self) -> Result<Self, BindingError> {
        if self.max_states == 0 || self.max_work == 0 || self.work_per_call == 0 {
            return Err(BindingError::InvalidArgument(
                "graph capture requires positive state, total-work, and per-call-work bounds",
            ));
        }
        Ok(self)
    }
}

/// A complete reachable state; `arcs` preserve provider insertion order.
#[derive(Clone, Debug)]
pub(crate) struct ScalarGraphState {
    pub raw_id: u64,
    pub is_final: bool,
    pub final_weight: f64,
    pub arcs: Vec<VtWfstArc>,
}

/// An exact, finite graph copied from one immutable provider snapshot.
#[derive(Clone, Debug)]
pub(crate) struct ScalarGraph {
    pub unit_domain: VtUnitDomain,
    pub weight_domain: VtWeightDomain,
    pub states: Vec<ScalarGraphState>,
    pub local_ids: HashMap<u64, usize>,
}

impl ScalarGraph {
    /// The captured start state is always local state zero.
    pub fn start(&self) -> usize {
        0
    }

    pub fn arc_count(&self) -> usize {
        self.states.iter().map(|state| state.arcs.len()).sum()
    }
}

/// A single bounded graph-capture poll.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GraphPoll {
    Pending,
    Complete,
    Cancelled,
}

#[derive(Clone, Debug)]
enum TerminalOutcome {
    Complete,
    Cancelled,
    Failed(BindingError),
}

impl TerminalOutcome {
    fn poll(&self) -> Result<GraphPoll, BindingError> {
        match self {
            Self::Complete => Ok(GraphPoll::Complete),
            Self::Cancelled => Ok(GraphPoll::Cancelled),
            Self::Failed(error) => Err(error.clone()),
        }
    }
}

struct PendingState {
    local_id: usize,
    header_done: bool,
    total_arcs: Option<usize>,
    next_offset: usize,
}

/// Resumable graph capture retaining one immutable provider snapshot.
pub(crate) struct CapturedGraphCursor {
    captured: CapturedWfst,
    graph: ScalarGraph,
    config: ScalarGraphConfig,
    queue: VecDeque<usize>,
    current: Option<PendingState>,
    page: Vec<VtWfstArc>,
    reserved_arcs: usize,
    work: usize,
    terminal: Option<TerminalOutcome>,
}

impl CapturedGraphCursor {
    /// Capture the provider once without asking for a state count or expanding
    /// the start state. The source may close immediately afterward.
    ///
    /// # Safety
    /// `resource` must be a live `VtResource` for the duration of capture.
    pub(crate) unsafe fn capture(
        resource: VtResource,
        config: ScalarGraphConfig,
    ) -> Result<Self, BindingError> {
        let config = config.validate()?;
        let captured = unsafe { CapturedWfst::capture(resource)? };
        let start = captured.start;
        let graph = ScalarGraph {
            unit_domain: captured.unit_domain,
            weight_domain: captured.weight_domain,
            states: vec![ScalarGraphState {
                raw_id: start,
                is_final: false,
                final_weight: scalar_zero(captured.weight_domain),
                arcs: Vec::new(),
            }],
            local_ids: HashMap::from([(start, 0)]),
        };
        Ok(Self {
            captured,
            graph,
            config,
            queue: VecDeque::from([0]),
            current: None,
            page: vec![VtWfstArc::default(); VT_RECOMMENDED_ARC_BATCH],
            reserved_arcs: 0,
            work: 0,
            terminal: None,
        })
    }

    fn fail(&mut self, error: BindingError) -> Result<GraphPoll, BindingError> {
        self.terminal = Some(TerminalOutcome::Failed(error.clone()));
        Err(error)
    }

    fn apply_header(&mut self, local_id: usize, header: StateHeader) -> Result<(), BindingError> {
        if !header.valid {
            return Err(BindingError::InvalidProviderOutput(
                "a reachable state is invalid in the captured snapshot",
            ));
        }
        let state = &mut self.graph.states[local_id];
        state.is_final = header.is_final;
        state.final_weight = header.final_weight;
        Ok(())
    }

    fn finish_state(&mut self, local_id: usize) -> Result<(), BindingError> {
        // Iterate the completed source state's arcs by index: adding a newly
        // discovered target may grow `states`, but never reorders these arcs.
        for arc_index in 0..self.graph.states[local_id].arcs.len() {
            let target = self.graph.states[local_id].arcs[arc_index].target_state;
            if !self.graph.local_ids.contains_key(&target) {
                if self.graph.states.len() >= self.config.max_states {
                    return Err(BindingError::BudgetExceeded("states"));
                }
                let next_id = self.graph.states.len();
                self.graph.local_ids.insert(target, next_id);
                self.graph.states.push(ScalarGraphState {
                    raw_id: target,
                    is_final: false,
                    final_weight: scalar_zero(self.graph.weight_domain),
                    arcs: Vec::new(),
                });
                self.queue.push_back(next_id);
            }
        }
        Ok(())
    }

    /// Do no more than `work_per_call` provider callbacks and no more than
    /// `VT_RECOMMENDED_ARC_BATCH` arc transfers per callback. A provider's own
    /// callback latency cannot be bounded by this adapter.
    pub(crate) fn poll(&mut self, cancelled: impl Fn() -> bool) -> Result<GraphPoll, BindingError> {
        if let Some(terminal) = &self.terminal {
            return terminal.poll();
        }

        let mut call_work = 0usize;
        loop {
            if cancelled() {
                self.terminal = Some(TerminalOutcome::Cancelled);
                return Ok(GraphPoll::Cancelled);
            }
            if self.current.is_none() {
                if let Some(local_id) = self.queue.pop_front() {
                    self.current = Some(PendingState {
                        local_id,
                        header_done: false,
                        total_arcs: None,
                        next_offset: 0,
                    });
                } else {
                    self.terminal = Some(TerminalOutcome::Complete);
                    return Ok(GraphPoll::Complete);
                }
            }
            if call_work >= self.config.work_per_call {
                return Ok(GraphPoll::Pending);
            }
            if self.work >= self.config.max_work {
                return self.fail(BindingError::BudgetExceeded("work"));
            }
            self.work += 1;
            call_work += 1;

            let pending = self.current.as_ref().expect("current was initialized");
            let local_id = pending.local_id;
            let raw_id = self.graph.states[local_id].raw_id;
            if !pending.header_done {
                let header = match self.captured.state_header(raw_id) {
                    Ok(header) => header,
                    Err(error) => return self.fail(error),
                };
                if let Err(error) = self.apply_header(local_id, header) {
                    return self.fail(error);
                }
                self.current.as_mut().expect("current exists").header_done = true;
                continue;
            }

            let offset = pending.next_offset;
            let previous_total = pending.total_arcs;
            let (written, total) = match self.captured.arc_page(raw_id, offset, &mut self.page) {
                Ok(counts) => counts,
                Err(error) => return self.fail(error),
            };
            if previous_total.is_some_and(|expected| expected != total) {
                return self.fail(BindingError::InvalidProviderOutput(
                    "arc total changed across snapshot pages",
                ));
            }
            if previous_total.is_none() {
                self.reserved_arcs = match self.reserved_arcs.checked_add(total) {
                    Some(count) if count <= self.config.max_arcs => count,
                    _ => return self.fail(BindingError::BudgetExceeded("arcs")),
                };
            }
            self.graph.states[local_id]
                .arcs
                .extend_from_slice(&self.page[..written]);
            let next_offset = match offset.checked_add(written) {
                Some(next_offset) => next_offset,
                None => return self.fail(BindingError::RepresentationLimit),
            };
            if next_offset == total {
                if let Err(error) = self.finish_state(local_id) {
                    return self.fail(error);
                }
                self.current = None;
            } else {
                let pending = self.current.as_mut().expect("current exists");
                pending.total_arcs = Some(total);
                pending.next_offset = next_offset;
            }
        }
    }

    /// Take the exact reachable graph only after `Complete`; incomplete,
    /// cancelled, and failed captures cannot produce an analysis graph.
    pub(crate) fn into_graph(self) -> Result<ScalarGraph, BindingError> {
        match self.terminal {
            Some(TerminalOutcome::Complete) => Ok(self.graph),
            _ => Err(BindingError::InvalidArgument(
                "graph capture is not complete",
            )),
        }
    }

    pub(crate) fn is_complete(&self) -> bool {
        matches!(self.terminal, Some(TerminalOutcome::Complete))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{OwnedWfstResource, ScalarWfstGraph};

    fn fixture() -> OwnedWfstResource {
        let mut graph = ScalarWfstGraph::new(VtUnitDomain::Byte, VtWeightDomain::TropicalF64);
        let root = graph.add_state().expect("root");
        let left = graph.add_state().expect("left");
        let right = graph.add_state().expect("right");
        assert!(graph.set_start(root));
        assert!(graph.set_final(left, 1.0));
        assert!(graph.set_final(right, 2.0));
        for index in 0..300 {
            assert!(graph.add_arc(
                root,
                VtWfstArc {
                    input_label: (index % 256) as u64,
                    output_label: (index % 256) as u64,
                    target_state: u64::from(if index % 2 == 0 { left } else { right }),
                    weight: index as f64,
                    has_input: 1,
                    has_output: 1,
                    reserved: [0; 6],
                },
            ));
        }
        OwnedWfstResource::from_scalar_wfst(graph)
    }

    fn config() -> ScalarGraphConfig {
        ScalarGraphConfig {
            max_states: 3,
            max_arcs: 300,
            max_work: 7,
            work_per_call: 1,
        }
    }

    #[test]
    fn pages_incrementally_and_assigns_deterministic_breadth_first_ids() {
        let owner = fixture();
        let mut cursor = unsafe { CapturedGraphCursor::capture(owner.as_raw(), config()) }
            .expect("capture snapshot");
        drop(owner);
        let mut pending = 0;
        loop {
            match cursor.poll(|| false).expect("bounded capture") {
                GraphPoll::Pending => pending += 1,
                GraphPoll::Complete => break,
                GraphPoll::Cancelled => panic!("unexpected cancellation"),
            }
        }
        assert_eq!(pending, 6);
        assert_eq!(cursor.poll(|| false).unwrap(), GraphPoll::Complete);
        let graph = cursor.into_graph().expect("complete graph");
        assert_eq!(graph.start(), 0);
        assert_eq!(graph.states.len(), 3);
        assert_eq!(graph.states[0].arcs.len(), 300);
        assert_eq!(graph.states[0].arcs[0].target_state, graph.states[1].raw_id);
        assert_eq!(graph.states[0].arcs[1].target_state, graph.states[2].raw_id);
        assert_eq!(graph.local_ids[&graph.states[1].raw_id], 1);
        assert_eq!(graph.local_ids[&graph.states[2].raw_id], 2);
        assert_eq!(graph.states[1].final_weight, 1.0);
        assert_eq!(graph.states[2].final_weight, 2.0);
    }

    #[test]
    fn limits_and_cancellation_never_masquerade_as_completion() {
        let owner = fixture();
        let mut too_few_arcs = config();
        too_few_arcs.max_arcs = 299;
        let mut cursor = unsafe { CapturedGraphCursor::capture(owner.as_raw(), too_few_arcs) }
            .expect("capture snapshot");
        assert_eq!(cursor.poll(|| false).unwrap(), GraphPoll::Pending);
        assert_eq!(
            cursor.poll(|| false),
            Err(BindingError::BudgetExceeded("arcs"))
        );
        assert_eq!(
            cursor.poll(|| false),
            Err(BindingError::BudgetExceeded("arcs"))
        );
        assert!(cursor.into_graph().is_err());

        let mut cursor = unsafe { CapturedGraphCursor::capture(owner.as_raw(), config()) }
            .expect("capture snapshot");
        assert_eq!(cursor.poll(|| true).unwrap(), GraphPoll::Cancelled);
        assert_eq!(cursor.poll(|| false).unwrap(), GraphPoll::Cancelled);
        assert!(cursor.into_graph().is_err());
    }

    #[test]
    fn cycles_reuse_one_local_state_instead_of_reexpanding() {
        let mut graph = ScalarWfstGraph::new(VtUnitDomain::U64, VtWeightDomain::BooleanF64);
        let root = graph.add_state().expect("root");
        assert!(graph.set_start(root));
        assert!(graph.set_final(root, 1.0));
        assert!(graph.add_arc(
            root,
            VtWfstArc {
                target_state: u64::from(root),
                weight: 1.0,
                ..VtWfstArc::default()
            },
        ));
        let owner = OwnedWfstResource::from_scalar_wfst(graph);
        let mut cursor = unsafe {
            CapturedGraphCursor::capture(
                owner.as_raw(),
                ScalarGraphConfig {
                    max_states: 1,
                    max_arcs: 1,
                    max_work: 2,
                    work_per_call: 1,
                },
            )
        }
        .expect("capture snapshot");
        assert_eq!(cursor.poll(|| false).unwrap(), GraphPoll::Pending);
        assert_eq!(cursor.poll(|| false).unwrap(), GraphPoll::Complete);
        assert_eq!(cursor.into_graph().unwrap().states.len(), 1);
    }
}
