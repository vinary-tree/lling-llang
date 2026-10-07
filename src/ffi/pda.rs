//! Versioned, bounded weighted-PDA resource ABI for Julia and other consumers.

use super::{
    boundary, decode_unit_domain, decode_weight_domain, required_mut, set_error, LlingLlangStatus,
};
use crate::bindings::{valid_scalar_label, valid_scalar_weight};
use crate::pushdown::{
    BuildError, PdaAcceptMode, PdaBuilder, PdaConfiguration, PdaDecodeLimit, PdaDecoder,
    StackAction, StackSymbol, VectorPda,
};
use crate::semiring::{
    ArcticWeight, BoolWeight, CountWeight, LogWeight, ProbabilityWeight, Semiring,
    SignedTropicalWeight, TropicalWeight,
};
use std::sync::Arc;
use vinary_tree_interop::{VtUnitDomain, VtWeightDomain};

/// Pop the matched stack symbol.
pub const LLING_PDA_POP: u32 = 0;
/// Pop the matched symbol, then push the supplied sequence.
pub const LLING_PDA_PUSH: u32 = 1;
/// Replace the matched symbol with the supplied sequence.
pub const LLING_PDA_REPLACE: u32 = 2;
/// Leave the matched symbol in place.
pub const LLING_PDA_NOOP: u32 = 3;

/// One native weighted legal-next terminal.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LlingPdaChoice {
    /// Valid scalar in the PDA's declared unit domain.
    pub label: u64,
    /// Value in the PDA's declared scalar semiring carrier.
    pub weight: f64,
}

trait WireWeight: Semiring {
    fn decode(raw: f64) -> Self;
    fn encode(self) -> Result<f64, PdaDecodeLimit>;
}

macro_rules! float_weight {
    ($($weight:ty => $domain:expr),+ $(,)?) => {
        $(impl WireWeight for $weight {
            fn decode(raw: f64) -> Self { Self::from(raw) }
            fn encode(self) -> Result<f64, PdaDecodeLimit> {
                let value = f64::from(self);
                valid_scalar_weight($domain, value)
                    .then_some(value)
                    .ok_or(PdaDecodeLimit::Weight)
            }
        })+
    };
}

float_weight!(
    TropicalWeight => VtWeightDomain::TropicalF64,
    LogWeight => VtWeightDomain::LogF64,
    ProbabilityWeight => VtWeightDomain::ProbabilityF64,
    ArcticWeight => VtWeightDomain::ArcticF64,
    SignedTropicalWeight => VtWeightDomain::SignedTropicalF64,
);

impl WireWeight for CountWeight {
    fn decode(raw: f64) -> Self {
        Self::new(raw as u64)
    }
    fn encode(self) -> Result<f64, PdaDecodeLimit> {
        (self.0 <= (1_u64 << 53))
            .then_some(self.0 as f64)
            .ok_or(PdaDecodeLimit::Weight)
    }
}

impl WireWeight for BoolWeight {
    fn decode(raw: f64) -> Self {
        Self::new(raw == 1.0)
    }
    fn encode(self) -> Result<f64, PdaDecodeLimit> {
        Ok(f64::from(self.0))
    }
}

trait BuilderOps {
    fn add_state(&mut self) -> u32;
    fn set_start(&mut self, state: u32);
    fn add_stack_symbol(&mut self) -> u32;
    fn set_initial_stack(&mut self, symbol: u32);
    fn set_final(&mut self, state: u32, weight: f64);
    fn add_transition(
        &mut self,
        from: u32,
        input: Option<u64>,
        stack_top: u32,
        to: u32,
        action: StackAction,
        weight: f64,
    );
    fn validate(&self) -> Result<(), BuildError>;
    fn build(self: Box<Self>) -> Result<Box<dyn RuntimeOps>, BuildError>;
}

impl<W: WireWeight> BuilderOps for PdaBuilder<u64, W> {
    fn add_state(&mut self) -> u32 {
        self.add_state()
    }
    fn set_start(&mut self, state: u32) {
        self.set_start(state);
    }
    fn add_stack_symbol(&mut self) -> u32 {
        self.add_stack_symbol().id()
    }
    fn set_initial_stack(&mut self, symbol: u32) {
        self.set_initial_stack(StackSymbol::new(symbol));
    }
    fn set_final(&mut self, state: u32, weight: f64) {
        self.set_final(state, W::decode(weight));
    }
    fn add_transition(
        &mut self,
        from: u32,
        input: Option<u64>,
        stack_top: u32,
        to: u32,
        action: StackAction,
        weight: f64,
    ) {
        self.add_transition(
            from,
            input,
            StackSymbol::new(stack_top),
            to,
            action,
            W::decode(weight),
        );
    }
    fn validate(&self) -> Result<(), BuildError> {
        self.validate()
    }
    fn build(self: Box<Self>) -> Result<Box<dyn RuntimeOps>, BuildError> {
        Ok(Box::new(Runtime {
            pda: Arc::new((*self).try_build()?),
        }))
    }
}

trait RuntimeOps {
    fn session(&self, max_stack_depth: usize) -> Box<dyn SessionOps>;
}

struct Runtime<W: WireWeight> {
    pda: Arc<VectorPda<u64, W>>,
}

impl<W: WireWeight> RuntimeOps for Runtime<W> {
    fn session(&self, max_stack_depth: usize) -> Box<dyn SessionOps> {
        let decoder = PdaDecoder::with_max_stack_depth(&self.pda, max_stack_depth);
        Box::new(Session {
            pda: Arc::clone(&self.pda),
            configuration: decoder.initial_config(),
            max_stack_depth,
        })
    }
}

trait SessionOps {
    fn frontier(&self, max_work: usize) -> Result<Vec<LlingPdaChoice>, PdaDecodeLimit>;
    fn advance(&mut self, label: u64, max_work: usize) -> Result<bool, PdaDecodeLimit>;
    fn acceptance(&self, max_work: usize) -> Result<(bool, f64), PdaDecodeLimit>;
    fn state(&self) -> u32;
    fn stack(&self) -> &[StackSymbol];
}

struct Session<W: WireWeight> {
    pda: Arc<VectorPda<u64, W>>,
    configuration: PdaConfiguration<u64>,
    max_stack_depth: usize,
}

impl<W: WireWeight> Session<W> {
    fn decoder(&self) -> PdaDecoder<'_, u64, W> {
        PdaDecoder::with_max_stack_depth(&self.pda, self.max_stack_depth)
    }
}

impl<W: WireWeight> SessionOps for Session<W> {
    fn frontier(&self, max_work: usize) -> Result<Vec<LlingPdaChoice>, PdaDecodeLimit> {
        self.decoder()
            .try_legal_next_weighted(&self.configuration, max_work)?
            .into_iter()
            .map(|(label, weight)| {
                Ok(LlingPdaChoice {
                    label,
                    weight: weight.encode()?,
                })
            })
            .collect()
    }
    fn advance(&mut self, label: u64, max_work: usize) -> Result<bool, PdaDecodeLimit> {
        let next = self
            .decoder()
            .try_advance(&self.configuration, &label, max_work)?;
        if let Some(next) = next {
            self.configuration = next;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn acceptance(&self, max_work: usize) -> Result<(bool, f64), PdaDecodeLimit> {
        let decoder = self.decoder();
        let accepted = decoder.try_is_accepting(&self.configuration, max_work)?;
        let weight = decoder
            .try_acceptance_weight(&self.configuration, max_work)?
            .encode()?;
        Ok((accepted, weight))
    }
    fn state(&self) -> u32 {
        self.configuration.state
    }
    fn stack(&self) -> &[StackSymbol] {
        &self.configuration.stack
    }
}

/// Mutable builder that owns one native scalar-semiring PDA specialization.
pub struct LlingPdaBuilder {
    inner: Option<Box<dyn BuilderOps>>,
    unit_domain: VtUnitDomain,
    weight_domain: VtWeightDomain,
    state_count: u32,
    stack_count: u32,
}

/// Immutable compiled PDA; sessions retain the engine after this handle closes.
pub struct LlingPda {
    inner: Box<dyn RuntimeOps>,
    unit_domain: VtUnitDomain,
    weight_domain: VtWeightDomain,
}

/// Mutable incremental decode configuration and bounded frontier page state.
pub struct LlingPdaSession {
    inner: Box<dyn SessionOps>,
    unit_domain: VtUnitDomain,
    frontier: Vec<LlingPdaChoice>,
    next_frontier: usize,
}

fn invalid(message: impl Into<String>) -> LlingLlangStatus {
    set_error(message);
    LlingLlangStatus::InvalidArgument
}

fn limit(message: impl Into<String>) -> LlingLlangStatus {
    set_error(message);
    LlingLlangStatus::LimitExceeded
}

fn decode_limit(error: PdaDecodeLimit) -> LlingLlangStatus {
    limit(error.to_string())
}

fn builder(value: *mut LlingPdaBuilder) -> Result<&'static mut LlingPdaBuilder, LlingLlangStatus> {
    let builder = required_mut(value, "PDA builder")?;
    if builder.inner.is_none() {
        return Err(invalid("PDA builder was consumed"));
    }
    Ok(builder)
}

fn known_state(builder: &LlingPdaBuilder, state: u32) -> Result<(), LlingLlangStatus> {
    if state >= builder.state_count {
        return Err(invalid("PDA state was not allocated"));
    }
    Ok(())
}

fn known_stack(builder: &LlingPdaBuilder, symbol: u32) -> Result<(), LlingLlangStatus> {
    if symbol >= builder.stack_count {
        return Err(invalid("PDA stack symbol was not allocated"));
    }
    Ok(())
}

fn known_weight(builder: &LlingPdaBuilder, weight: f64) -> Result<(), LlingLlangStatus> {
    if !valid_scalar_weight(builder.weight_domain, weight) {
        return Err(invalid("weight is outside the declared scalar semiring"));
    }
    Ok(())
}

/// Allocate a typed PDA builder. Acceptance modes are final-state=0,
/// empty-stack=1, or either=2. Inputs and outputs use the scalar family ABI.
#[no_mangle]
pub extern "C" fn lling_pda_builder_open(
    unit_domain: u32,
    weight_domain: u32,
    acceptance_mode: u32,
    out_builder: *mut *mut LlingPdaBuilder,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_builder, "out_builder")?;
        *output = std::ptr::null_mut();
        let unit_domain = decode_unit_domain(unit_domain)?;
        let weight_domain = decode_weight_domain(weight_domain)?;
        let mode = match acceptance_mode {
            0 => PdaAcceptMode::FinalState,
            1 => PdaAcceptMode::EmptyStack,
            2 => PdaAcceptMode::Both,
            _ => return Err(invalid("unknown PDA acceptance mode")),
        };
        let inner: Box<dyn BuilderOps> = match weight_domain {
            VtWeightDomain::TropicalF64 => {
                Box::new(PdaBuilder::<u64, TropicalWeight>::with_accept_mode(mode))
            }
            VtWeightDomain::LogF64 => {
                Box::new(PdaBuilder::<u64, LogWeight>::with_accept_mode(mode))
            }
            VtWeightDomain::ProbabilityF64 => {
                Box::new(PdaBuilder::<u64, ProbabilityWeight>::with_accept_mode(mode))
            }
            VtWeightDomain::ArcticF64 => {
                Box::new(PdaBuilder::<u64, ArcticWeight>::with_accept_mode(mode))
            }
            VtWeightDomain::SignedTropicalF64 => Box::new(
                PdaBuilder::<u64, SignedTropicalWeight>::with_accept_mode(mode),
            ),
            VtWeightDomain::CountF64 => {
                Box::new(PdaBuilder::<u64, CountWeight>::with_accept_mode(mode))
            }
            VtWeightDomain::BooleanF64 => {
                Box::new(PdaBuilder::<u64, BoolWeight>::with_accept_mode(mode))
            }
        };
        *output = Box::into_raw(Box::new(LlingPdaBuilder {
            inner: Some(inner),
            unit_domain,
            weight_domain,
            state_count: 0,
            stack_count: 1,
        }));
        Ok(())
    })
}

/// Free an unconsumed or consumed PDA builder; null is accepted.
///
/// # Safety
/// Non-null pointers must be live builder handles returned by this ABI.
#[no_mangle]
pub unsafe extern "C" fn lling_pda_builder_free(builder: *mut LlingPdaBuilder) {
    if !builder.is_null() {
        drop(unsafe { Box::from_raw(builder) });
    }
}

/// Append a state and return its zero-based identifier.
#[no_mangle]
pub extern "C" fn lling_pda_builder_add_state(
    builder_ptr: *mut LlingPdaBuilder,
    out_state: *mut u32,
) -> LlingLlangStatus {
    boundary(|| {
        let builder = builder(builder_ptr)?;
        let output = required_mut(out_state, "out_state")?;
        if builder.state_count == u32::MAX {
            return Err(limit("PDA state domain exhausted"));
        }
        *output = builder.inner.as_mut().expect("checked builder").add_state();
        builder.state_count += 1;
        Ok(())
    })
}

/// Select a previously allocated start state.
#[no_mangle]
pub extern "C" fn lling_pda_builder_set_start(
    builder_ptr: *mut LlingPdaBuilder,
    state: u32,
) -> LlingLlangStatus {
    boundary(|| {
        let builder = builder(builder_ptr)?;
        known_state(builder, state)?;
        builder
            .inner
            .as_mut()
            .expect("checked builder")
            .set_start(state);
        Ok(())
    })
}

/// Allocate a stack symbol. Zero is reserved for the bottom marker.
#[no_mangle]
pub extern "C" fn lling_pda_builder_add_stack_symbol(
    builder_ptr: *mut LlingPdaBuilder,
    out_symbol: *mut u32,
) -> LlingLlangStatus {
    boundary(|| {
        let builder = builder(builder_ptr)?;
        let output = required_mut(out_symbol, "out_symbol")?;
        if builder.stack_count == u32::MAX {
            return Err(limit("PDA stack-symbol domain exhausted"));
        }
        *output = builder
            .inner
            .as_mut()
            .expect("checked builder")
            .add_stack_symbol();
        builder.stack_count += 1;
        Ok(())
    })
}

/// Set an allocated initial stack symbol, including the bottom marker.
#[no_mangle]
pub extern "C" fn lling_pda_builder_set_initial_stack(
    builder_ptr: *mut LlingPdaBuilder,
    symbol: u32,
) -> LlingLlangStatus {
    boundary(|| {
        let builder = builder(builder_ptr)?;
        known_stack(builder, symbol)?;
        builder
            .inner
            .as_mut()
            .expect("checked builder")
            .set_initial_stack(symbol);
        Ok(())
    })
}

/// Mark an allocated state final with a semiring-carrier weight.
#[no_mangle]
pub extern "C" fn lling_pda_builder_set_final(
    builder_ptr: *mut LlingPdaBuilder,
    state: u32,
    weight: f64,
) -> LlingLlangStatus {
    boundary(|| {
        let builder = builder(builder_ptr)?;
        known_state(builder, state)?;
        known_weight(builder, weight)?;
        builder
            .inner
            .as_mut()
            .expect("checked builder")
            .set_final(state, weight);
        Ok(())
    })
}

/// Add one copied weighted transition. `has_input=0` denotes epsilon and
/// requires input zero; `has_input=1` validates the declared scalar domain.
///
/// # Safety
/// `action_symbols` must be readable for `action_len` elements when nonzero.
#[no_mangle]
pub unsafe extern "C" fn lling_pda_builder_add_transition(
    builder_ptr: *mut LlingPdaBuilder,
    from: u32,
    input: u64,
    has_input: u8,
    stack_top: u32,
    to: u32,
    action_kind: u32,
    action_symbols: *const u32,
    action_len: usize,
    weight: f64,
) -> LlingLlangStatus {
    boundary(|| {
        let builder = builder(builder_ptr)?;
        known_state(builder, from)?;
        known_state(builder, to)?;
        known_stack(builder, stack_top)?;
        known_weight(builder, weight)?;
        let input = match has_input {
            0 if input == 0 => None,
            1 if valid_scalar_label(builder.unit_domain, input) => Some(input),
            _ => return Err(invalid("input label is outside the declared scalar domain")),
        };
        if action_len > isize::MAX as usize / std::mem::size_of::<u32>() {
            return Err(limit("PDA action word exceeds addressable memory"));
        }
        let symbols = if action_len == 0 {
            &[][..]
        } else if action_symbols.is_null() {
            return Err(invalid("action_symbols is null with nonzero length"));
        } else {
            unsafe { std::slice::from_raw_parts(action_symbols, action_len) }
        };
        for &symbol in symbols {
            known_stack(builder, symbol)?;
        }
        let action = match action_kind {
            LLING_PDA_POP if symbols.is_empty() => StackAction::Pop,
            LLING_PDA_PUSH => {
                StackAction::Push(symbols.iter().copied().map(StackSymbol::new).collect())
            }
            LLING_PDA_REPLACE => {
                StackAction::Replace(symbols.iter().copied().map(StackSymbol::new).collect())
            }
            LLING_PDA_NOOP if symbols.is_empty() => StackAction::Noop,
            _ => return Err(invalid("unknown PDA action or unexpected action symbols")),
        };
        builder
            .inner
            .as_mut()
            .expect("checked builder")
            .add_transition(from, input, stack_top, to, action, weight);
        Ok(())
    })
}

/// Validate and freeze a PDA. On failure the builder remains intact.
#[no_mangle]
pub extern "C" fn lling_pda_builder_build(
    builder_ptr: *mut LlingPdaBuilder,
    out_pda: *mut *mut LlingPda,
) -> LlingLlangStatus {
    boundary(|| {
        let builder = builder(builder_ptr)?;
        let output = required_mut(out_pda, "out_pda")?;
        *output = std::ptr::null_mut();
        // Reject malformed machines before transferring ownership so callers
        // may correct a builder and retry the same freeze operation.
        if let Err(error) = builder.inner.as_ref().expect("checked builder").validate() {
            set_error(error.to_string());
            return Err(LlingLlangStatus::InvalidArgument);
        }
        let inner = builder.inner.take().expect("checked builder");
        let unit_domain = builder.unit_domain;
        let weight_domain = builder.weight_domain;
        let runtime = match inner.build() {
            Ok(runtime) => runtime,
            Err(error) => {
                set_error(error.to_string());
                return Err(LlingLlangStatus::InvalidArgument);
            }
        };
        *output = Box::into_raw(Box::new(LlingPda {
            inner: runtime,
            unit_domain,
            weight_domain,
        }));
        Ok(())
    })
}

/// Release a compiled PDA. Sessions retain their independent native owner.
///
/// # Safety
/// Non-null pointers must be live PDA handles returned by this ABI.
#[no_mangle]
pub unsafe extern "C" fn lling_pda_free(pda: *mut LlingPda) {
    if !pda.is_null() {
        drop(unsafe { Box::from_raw(pda) });
    }
}

/// Report the immutable label and scalar-weight domains of a compiled PDA.
///
/// # Safety
/// `pda_ptr` must be a live PDA handle returned by this ABI.
#[no_mangle]
pub unsafe extern "C" fn lling_pda_domains(
    pda_ptr: *const LlingPda,
    out_unit_domain: *mut u32,
    out_weight_domain: *mut u32,
) -> LlingLlangStatus {
    boundary(|| {
        let unit = required_mut(out_unit_domain, "out_unit_domain")?;
        let weight = required_mut(out_weight_domain, "out_weight_domain")?;
        if pda_ptr.is_null() {
            return Err(invalid("PDA handle is null"));
        }
        let pda = unsafe { &*pda_ptr };
        *unit = pda.unit_domain as u32;
        *weight = pda.weight_domain as u32;
        Ok(())
    })
}

/// Open an iterative decode session with an explicit positive stack-depth cap.
/// The returned session owns a retained reference to the compiled PDA.
///
/// # Safety
/// `pda_ptr` must be a live PDA handle returned by this ABI.
#[no_mangle]
pub unsafe extern "C" fn lling_pda_session_open(
    pda_ptr: *const LlingPda,
    max_stack_depth: usize,
    out_session: *mut *mut LlingPdaSession,
) -> LlingLlangStatus {
    boundary(|| {
        let output = required_mut(out_session, "out_session")?;
        *output = std::ptr::null_mut();
        if pda_ptr.is_null() {
            return Err(invalid("PDA handle is null"));
        }
        if max_stack_depth == 0 {
            return Err(invalid("max_stack_depth must be positive"));
        }
        let pda = unsafe { &*pda_ptr };
        *output = Box::into_raw(Box::new(LlingPdaSession {
            inner: pda.inner.session(max_stack_depth),
            unit_domain: pda.unit_domain,
            frontier: Vec::new(),
            next_frontier: 0,
        }));
        Ok(())
    })
}

/// Free an incremental session; null is accepted.
///
/// # Safety
/// Non-null pointers must be live session handles returned by this ABI.
#[no_mangle]
pub unsafe extern "C" fn lling_pda_session_free(session: *mut LlingPdaSession) {
    if !session.is_null() {
        drop(unsafe { Box::from_raw(session) });
    }
}

/// Capture the weighted legal terminal frontier within `max_work` native
/// operations. On work exhaustion no partial frontier is published. A later
/// call replaces any earlier frontier, and advancing invalidates it.
#[no_mangle]
pub extern "C" fn lling_pda_session_frontier_open(
    session_ptr: *mut LlingPdaSession,
    max_work: usize,
    out_count: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let session = required_mut(session_ptr, "PDA session")?;
        let output = required_mut(out_count, "out_count")?;
        *output = 0;
        session.frontier.clear();
        session.next_frontier = 0;
        if max_work == 0 {
            return Err(invalid("max_work must be positive"));
        }
        session.frontier = session.inner.frontier(max_work).map_err(decode_limit)?;
        *output = session.frontier.len();
        Ok(())
    })
}

/// Copy the next bounded page of the captured frontier. A zero `out_written`
/// denotes exhaustion; no result pointers remain borrowed after this call.
///
/// # Safety
/// `out_choices` must be writable for `capacity` elements when nonzero.
#[no_mangle]
pub unsafe extern "C" fn lling_pda_session_frontier_next(
    session_ptr: *mut LlingPdaSession,
    out_choices: *mut LlingPdaChoice,
    capacity: usize,
    out_written: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let session = required_mut(session_ptr, "PDA session")?;
        let written = required_mut(out_written, "out_written")?;
        *written = 0;
        if capacity == 0 {
            return Err(invalid("frontier page capacity must be positive"));
        }
        if out_choices.is_null() {
            return Err(invalid("out_choices is null with nonzero capacity"));
        }
        let remaining = session.frontier.len() - session.next_frontier;
        let count = remaining.min(capacity);
        if count != 0 {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    session.frontier.as_ptr().add(session.next_frontier),
                    out_choices,
                    count,
                );
            }
        }
        session.next_frontier += count;
        *written = count;
        Ok(())
    })
}

/// Advance by one validated terminal. `out_advanced=0` leaves the configuration
/// unchanged; a work-limit failure likewise leaves it unchanged.
#[no_mangle]
pub extern "C" fn lling_pda_session_advance(
    session_ptr: *mut LlingPdaSession,
    label: u64,
    max_work: usize,
    out_advanced: *mut u8,
) -> LlingLlangStatus {
    boundary(|| {
        let session = required_mut(session_ptr, "PDA session")?;
        let output = required_mut(out_advanced, "out_advanced")?;
        *output = 0;
        if !valid_scalar_label(session.unit_domain, label) {
            return Err(invalid("terminal is outside the PDA unit domain"));
        }
        if max_work == 0 {
            return Err(invalid("max_work must be positive"));
        }
        let advanced = session
            .inner
            .advance(label, max_work)
            .map_err(decode_limit)?;
        session.frontier.clear();
        session.next_frontier = 0;
        *output = u8::from(advanced);
        Ok(())
    })
}

/// Query acceptance and exact native semiring aggregate under a work cap.
///
/// # Safety
/// `session_ptr` must be a live session handle returned by this ABI.
#[no_mangle]
pub unsafe extern "C" fn lling_pda_session_acceptance(
    session_ptr: *const LlingPdaSession,
    max_work: usize,
    out_accepted: *mut u8,
    out_weight: *mut f64,
) -> LlingLlangStatus {
    boundary(|| {
        let accepted = required_mut(out_accepted, "out_accepted")?;
        let weight = required_mut(out_weight, "out_weight")?;
        *accepted = 0;
        *weight = 0.0;
        if session_ptr.is_null() {
            return Err(invalid("PDA session is null"));
        }
        if max_work == 0 {
            return Err(invalid("max_work must be positive"));
        }
        let session = unsafe { &*session_ptr };
        let (value, aggregate) = session.inner.acceptance(max_work).map_err(decode_limit)?;
        *accepted = u8::from(value);
        *weight = aggregate;
        Ok(())
    })
}

/// Report the current control state and logical stack depth.
///
/// # Safety
/// `session_ptr` must be a live session handle returned by this ABI.
#[no_mangle]
pub unsafe extern "C" fn lling_pda_session_info(
    session_ptr: *const LlingPdaSession,
    out_state: *mut u32,
    out_stack_depth: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let state = required_mut(out_state, "out_state")?;
        let depth = required_mut(out_stack_depth, "out_stack_depth")?;
        if session_ptr.is_null() {
            return Err(invalid("PDA session is null"));
        }
        let session = unsafe { &*session_ptr };
        *state = session.inner.state();
        *depth = session.inner.stack().len();
        Ok(())
    })
}

/// Copy a caller-bounded slice of the current stack, bottom first.
///
/// # Safety
/// `out_symbols` must be writable for `capacity` elements when nonzero.
#[no_mangle]
pub unsafe extern "C" fn lling_pda_session_stack_page(
    session_ptr: *const LlingPdaSession,
    offset: usize,
    out_symbols: *mut u32,
    capacity: usize,
    out_written: *mut usize,
) -> LlingLlangStatus {
    boundary(|| {
        let written = required_mut(out_written, "out_written")?;
        *written = 0;
        if session_ptr.is_null() {
            return Err(invalid("PDA session is null"));
        }
        if capacity != 0 && out_symbols.is_null() {
            return Err(invalid("out_symbols is null with nonzero capacity"));
        }
        let stack = unsafe { &*session_ptr }.inner.stack();
        if offset > stack.len() {
            return Err(invalid("stack page offset exceeds stack depth"));
        }
        let count = (stack.len() - offset).min(capacity);
        if count != 0 {
            for (index, symbol) in stack[offset..offset + count].iter().enumerate() {
                unsafe { out_symbols.add(index).write(symbol.id()) };
            }
        }
        *written = count;
        Ok(())
    })
}
