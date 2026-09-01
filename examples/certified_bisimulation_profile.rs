//! Single-run profiling harness for certified strong bisimulation.
//!
//! Usage: `cargo run --release --example certified_bisimulation_profile --
//! <chain|wide|sparse|dense> <states>`.

use std::env;
use std::process::ExitCode;
use std::time::Instant;

use lling_llang::symbolic::bisimulation::{CertifiedBisimulation, Lts};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    let shape = arguments
        .next()
        .ok_or_else(|| "missing workload shape".to_owned())?;
    let states = arguments
        .next()
        .ok_or_else(|| "missing state count".to_owned())?
        .parse::<usize>()
        .map_err(|error| format!("invalid state count: {error}"))?;
    if arguments.next().is_some() {
        return Err("unexpected trailing argument".to_owned());
    }
    if states == 0 {
        return Err("state count must be positive".to_owned());
    }

    let (lts, colors) = match shape.as_str() {
        "chain" => chain(states),
        "wide" => wide(states),
        "sparse" => sparse(states),
        "dense" => dense(states)?,
        _ => return Err(format!("unknown workload shape: {shape}")),
    };
    let input_transitions = lts.transitions.len();
    let started = Instant::now();
    let result = CertifiedBisimulation::compute(&lts, &colors)
        .map_err(|error| format!("certified analysis failed: {error}"))?;
    let elapsed = started.elapsed();
    let resources = result.resources();
    println!(
        "shape={shape} states={states} input_transitions={input_transitions} \
         canonical_transitions={} blocks={} splits={} charged_work={} \
         core_heap_cells={} witness_dag_cells={} native_frames={} elapsed_ns={}",
        resources.transition_charge_counts().len(),
        result
            .blocks()
            .iter()
            .copied()
            .max()
            .map_or(0, |block| block + 1),
        result.certificate().split_count(),
        resources.charged_work(),
        resources.core_heap_cells(),
        resources.witness_dag_cells(),
        resources.maximum_native_frames(),
        elapsed.as_nanos(),
    );
    Ok(())
}

fn chain(states: usize) -> (Lts, Vec<usize>) {
    let transitions = (0..states - 1)
        .map(|source| (source, 0, source + 1))
        .collect();
    (Lts::new(states, transitions), vec![0; states])
}

fn wide(states: usize) -> (Lts, Vec<usize>) {
    let transitions = (1..states).map(|target| (0, 7, target)).collect();
    (Lts::new(states, transitions), vec![0; states])
}

fn sparse(states: usize) -> (Lts, Vec<usize>) {
    let transitions = (0..states - 1)
        .step_by(17)
        .map(|source| (source, (source % 5) as u32, source + 1))
        .collect();
    let colors = (0..states).map(|state| state % 3).collect();
    (Lts::new(states, transitions), colors)
}

fn dense(states: usize) -> Result<(Lts, Vec<usize>), String> {
    let capacity = states
        .checked_mul(states)
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| "dense workload size overflows usize".to_owned())?;
    let mut transitions = Vec::new();
    transitions
        .try_reserve_exact(capacity)
        .map_err(|error| format!("cannot allocate dense workload: {error}"))?;
    for source in 0..states {
        for action in 0..4 {
            for target in 0..states {
                transitions.push((source, action, target));
            }
        }
    }
    Ok((Lts::new(states, transitions), vec![0; states]))
}
