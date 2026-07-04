//! Context-dependency transducers for speech recognition.
//!
//! This module provides builders for constructing context-dependency transducers
//! that map context-independent phone sequences to context-dependent phone sequences.
//!
//! ## Triphone Construction
//!
//! A triphone stores one phone of left-context history and emits labels that
//! encode `(left-context, center-phone)`. For n phones:
//! - States: O(n) - representing the empty context plus one-phone histories
//! - Arcs: O(n²) - one arc per state and next phone
//!
//! ## Tetraphone Construction
//!
//! A tetraphone stores two phones of left-context history. For n phones:
//! - States: O(n²) - representing histories up to length two
//! - Arcs: O(n³) - one arc per state and next phone
//!
//! ## Deterministic vs Non-deterministic
//!
//! - **Non-deterministic**: Center phone as input label (simpler construction)
//! - **Deterministic**: Right phone as input label (no matching delay)
//!   - Requires final subsequential symbol ($) to pad context
//!
//! ## References
//!
//! - Mohri et al., "Speech Recognition with WFSTs" Section 4.3

use std::fmt::{self, Debug};
use std::hash::Hash;

use crate::semiring::Semiring;
use crate::wfst::{MutableWfst, StateId, VectorWfst, Wfst};

/// Phone identifier type.
pub type PhoneId = u32;

/// Epsilon label constant.
pub const EPSILON: Option<PhoneId> = None;

/// State in a context-dependency transducer.
///
/// Encodes the context history as a sequence of phones.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContextState {
    /// Left context (phones seen before current position).
    /// Order: [oldest, ..., most_recent]
    pub left_context: Vec<PhoneId>,
}

impl ContextState {
    /// Create initial state with empty context.
    pub fn initial() -> Self {
        Self {
            left_context: Vec::new(),
        }
    }

    /// Create state with given left context.
    pub fn with_context(context: Vec<PhoneId>) -> Self {
        Self {
            left_context: context,
        }
    }

    /// Extend context with a new phone, maintaining window size.
    pub fn extend(&self, phone: PhoneId, max_context: usize) -> Self {
        if max_context == 0 {
            return Self {
                left_context: Vec::new(),
            };
        }

        let keep = self.left_context.len().min(max_context - 1);
        let mut new_context = Vec::with_capacity(keep + 1);
        if keep > 0 {
            let start = self.left_context.len() - keep;
            new_context.extend_from_slice(&self.left_context[start..]);
        }
        new_context.push(phone);

        Self {
            left_context: new_context,
        }
    }
}

/// Configuration for context-dependency transducer construction.
#[derive(Clone, Debug, Default)]
pub struct ContextDependencyConfig {
    /// Whether to use deterministic construction (right phone as input).
    pub deterministic: bool,

    /// Final subsequential symbol for deterministic construction.
    /// Used to pad context at word boundaries.
    pub boundary_symbol: Option<PhoneId>,

    /// Whether to add self-loops for auxiliary symbols.
    pub auxiliary_self_loops: bool,

    /// Auxiliary symbol range (if any).
    pub auxiliary_symbols: Option<std::ops::Range<PhoneId>>,
}

/// A validation error detected while constructing a context-dependency WFST.
///
/// [`ContextDependencyBuilder::try_build`] reports these errors before eager
/// construction so invalid inventories and label spaces do not wrap in release
/// builds or panic in debug builds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextDependencyError {
    /// The configured phone count cannot be represented by [`PhoneId`].
    PhoneInventoryTooLarge {
        /// Requested number of phones.
        num_phones: usize,
        /// Maximum supported phone inventory size.
        max_supported: usize,
    },
    /// The mixed-radix label alphabet would require a base larger than
    /// [`PhoneId`] can represent.
    LabelAlphabetTooLarge {
        /// Largest direct symbol participating in the label alphabet.
        max_symbol: PhoneId,
    },
    /// The largest context-dependent label would exceed [`PhoneId`].
    LabelSpaceOverflow {
        /// Requested number of phones.
        num_phones: usize,
        /// Requested left-context size.
        context_size: usize,
        /// Mixed-radix base used for label encoding.
        label_base: PhoneId,
    },
    /// A supplied context state contains a phone outside the configured
    /// inventory.
    ContextPhoneOutOfRange {
        /// Offending context phone.
        phone: PhoneId,
        /// Configured number of phones.
        num_phones: usize,
    },
    /// The eager state set cannot be indexed by [`StateId`].
    StateSpaceOverflow {
        /// Requested number of phones.
        num_phones: usize,
        /// Requested left-context size.
        context_size: usize,
    },
    /// The configured transition set overflows `usize` accounting.
    ArcSpaceOverflow {
        /// Number of source context states.
        num_states: usize,
        /// Number of outgoing arcs per context state.
        arcs_per_state: usize,
    },
}

impl fmt::Display for ContextDependencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextDependencyError::PhoneInventoryTooLarge {
                num_phones,
                max_supported,
            } => write!(
                f,
                "phone inventory has {num_phones} phones, but at most {max_supported} are supported"
            ),
            ContextDependencyError::LabelAlphabetTooLarge { max_symbol } => write!(
                f,
                "context-dependent label alphabet with max symbol {max_symbol} exceeds PhoneId capacity"
            ),
            ContextDependencyError::LabelSpaceOverflow {
                num_phones,
                context_size,
                label_base,
            } => write!(
                f,
                "context labels for {num_phones} phones and context size {context_size} overflow PhoneId with base {label_base}"
            ),
            ContextDependencyError::ContextPhoneOutOfRange { phone, num_phones } => write!(
                f,
                "context phone {phone} is outside the configured inventory of {num_phones} phones"
            ),
            ContextDependencyError::StateSpaceOverflow {
                num_phones,
                context_size,
            } => write!(
                f,
                "context state space for {num_phones} phones and context size {context_size} exceeds StateId capacity"
            ),
            ContextDependencyError::ArcSpaceOverflow {
                num_states,
                arcs_per_state,
            } => write!(
                f,
                "context arc accounting overflows usize for {num_states} states and {arcs_per_state} arcs per state"
            ),
        }
    }
}

impl std::error::Error for ContextDependencyError {}

/// Builder for general context-dependency transducers.
pub struct ContextDependencyBuilder<W: Semiring> {
    /// Number of phones in the inventory.
    num_phones: usize,

    /// Left context size (number of preceding phones to consider).
    left_context_size: usize,

    /// Right context size (number of following phones to consider).
    right_context_size: usize,

    /// Configuration options.
    config: ContextDependencyConfig,

    /// Phantom for weight type.
    _weight: std::marker::PhantomData<W>,
}

impl<W: Semiring> ContextDependencyBuilder<W> {
    const MAX_INITIAL_RESERVE: usize = 1 << 20;

    /// Create a new context-dependency builder.
    ///
    /// # Arguments
    ///
    /// * `num_phones` - Number of phones in the inventory
    /// * `left_context_size` - Number of preceding phones to consider
    /// * `right_context_size` - Number of following phones to consider
    pub fn new(num_phones: usize, left_context_size: usize, right_context_size: usize) -> Self {
        Self {
            num_phones,
            left_context_size,
            right_context_size,
            config: ContextDependencyConfig::default(),
            _weight: std::marker::PhantomData,
        }
    }

    /// Set configuration options.
    pub fn config(mut self, config: ContextDependencyConfig) -> Self {
        self.config = config;
        self
    }

    /// Configured left context size (preceding phones).
    pub fn left_context_size(&self) -> usize {
        self.left_context_size
    }

    /// Configured right context size (following phones).
    pub fn right_context_size(&self) -> usize {
        self.right_context_size
    }

    /// Enable deterministic construction.
    pub fn deterministic(mut self, boundary_symbol: PhoneId) -> Self {
        self.config.deterministic = true;
        self.config.boundary_symbol = Some(boundary_symbol);
        self
    }

    /// Enable auxiliary symbol self-loops.
    pub fn with_auxiliary_symbols(mut self, range: std::ops::Range<PhoneId>) -> Self {
        self.config.auxiliary_self_loops = true;
        self.config.auxiliary_symbols = Some(range);
        self
    }

    /// Build the context-dependency transducer.
    ///
    /// This is the legacy convenience wrapper around [`try_build`](Self::try_build).
    /// It returns an empty WFST when the configuration cannot be represented by
    /// the current [`PhoneId`] / [`StateId`] storage types. Use
    /// [`try_build`](Self::try_build) when the caller needs the diagnostic.
    pub fn build(&self) -> VectorWfst<PhoneId, W> {
        self.try_build().unwrap_or_else(|_| VectorWfst::new())
    }

    /// Build the context-dependency transducer, validating capacity and label
    /// arithmetic before eager construction.
    ///
    /// For non-deterministic construction:
    /// - Input: center phone
    /// - Output: context-dependent phone (triphone/tetraphone label)
    ///
    /// For deterministic construction:
    /// - Input: right-context phone
    /// - Output: context-dependent phone
    pub fn try_build(&self) -> Result<VectorWfst<PhoneId, W>, ContextDependencyError> {
        self.validate_phone_inventory()?;
        self.validate_constructible_size()?;
        let label_base = self.label_base()?;
        self.validate_label_space(label_base)?;
        let context_state_capacity = self.checked_context_state_count().ok_or(
            ContextDependencyError::StateSpaceOverflow {
                num_phones: self.num_phones,
                context_size: self.left_context_size,
            },
        )?;
        let constructed_state_capacity = self
            .checked_constructed_state_count(context_state_capacity)
            .ok_or(ContextDependencyError::StateSpaceOverflow {
                num_phones: self.num_phones,
                context_size: self.left_context_size,
            })?;
        let outgoing_capacity =
            self.arcs_per_context_state()
                .ok_or(ContextDependencyError::ArcSpaceOverflow {
                    num_states: context_state_capacity,
                    arcs_per_state: usize::MAX,
                })?;

        let layer_offsets = self.checked_context_layer_offsets().ok_or(
            ContextDependencyError::StateSpaceOverflow {
                num_phones: self.num_phones,
                context_size: self.left_context_size,
            },
        )?;
        let radix_powers =
            self.checked_radix_powers()
                .ok_or(ContextDependencyError::StateSpaceOverflow {
                    num_phones: self.num_phones,
                    context_size: self.left_context_size,
                })?;

        let mut fst: VectorWfst<PhoneId, W> =
            VectorWfst::with_capacity(Self::initial_reserve_capacity(constructed_state_capacity));

        for _ in 0..context_state_capacity {
            fst.add_state();
        }
        fst.set_start(0);

        for state_index in 0..context_state_capacity {
            let current_id = self.state_id_for_context_index(state_index)?;
            let (context_len, context_rank) =
                self.context_len_and_rank(state_index, &layer_offsets, &radix_powers)?;
            let current_context =
                self.context_from_rank(context_len, context_rank, &radix_powers)?;

            fst.reserve_transitions(current_id, outgoing_capacity);

            // For each possible input phone
            for phone_index in 0..self.num_phones {
                let phone = PhoneId::try_from(phone_index).map_err(|_| {
                    ContextDependencyError::PhoneInventoryTooLarge {
                        num_phones: self.num_phones,
                        max_supported: Self::max_phone_inventory(),
                    }
                })?;
                // Skip epsilon/special phones if needed

                let next_id = self.extended_context_state_id(
                    context_len,
                    context_rank,
                    phone_index,
                    &layer_offsets,
                    &radix_powers,
                )?;

                // Compute context-dependent output label
                let output_label =
                    self.compute_cd_label_for_context_checked(&current_context, phone, label_base)?;

                // Add transition
                fst.add_arc(
                    current_id,
                    Some(phone),
                    Some(output_label),
                    next_id,
                    W::one(),
                );
            }

            // Add auxiliary symbol self-loops if configured
            if self.config.auxiliary_self_loops {
                if let Some(ref range) = self.config.auxiliary_symbols {
                    for aux in range.clone() {
                        fst.add_arc(current_id, Some(aux), Some(aux), current_id, W::one());
                    }
                }
            }

            // All states with full context are final
            if context_len >= self.left_context_size {
                fst.set_final(current_id, W::one());
            }
        }

        // For deterministic construction, add boundary handling
        if self.config.deterministic {
            if let Some(boundary) = self.config.boundary_symbol {
                self.add_boundary_handling(
                    &mut fst,
                    context_state_capacity,
                    &layer_offsets,
                    &radix_powers,
                    boundary,
                    label_base,
                )?;
            }
        }

        // Make all states final for proper word boundary handling
        for id in 0..fst.num_states() {
            let id =
                StateId::try_from(id).map_err(|_| ContextDependencyError::StateSpaceOverflow {
                    num_phones: self.num_phones,
                    context_size: self.left_context_size,
                })?;
            if !fst.is_final(id) {
                fst.set_final(id, W::one());
            }
        }

        Ok(fst)
    }

    fn max_phone_inventory() -> usize {
        PhoneId::MAX as usize
    }

    fn initial_reserve_capacity(expected: usize) -> usize {
        expected.min(Self::MAX_INITIAL_RESERVE)
    }

    fn validate_phone_inventory(&self) -> Result<(), ContextDependencyError> {
        let max_supported = Self::max_phone_inventory();
        if self.num_phones > max_supported {
            return Err(ContextDependencyError::PhoneInventoryTooLarge {
                num_phones: self.num_phones,
                max_supported,
            });
        }

        Ok(())
    }

    fn checked_context_state_count(&self) -> Option<usize> {
        let mut total = 1usize;
        let mut layer = 1usize;

        for _ in 0..self.left_context_size {
            layer = layer.checked_mul(self.num_phones)?;
            total = total.checked_add(layer)?;
        }

        Some(total)
    }

    fn checked_radix_powers(&self) -> Option<Vec<usize>> {
        let mut powers = Vec::with_capacity(self.left_context_size + 1);
        let mut power = 1usize;
        powers.push(power);

        for _ in 0..self.left_context_size {
            power = power.checked_mul(self.num_phones)?;
            powers.push(power);
        }

        Some(powers)
    }

    fn checked_context_layer_offsets(&self) -> Option<Vec<usize>> {
        let mut offsets = Vec::with_capacity(self.left_context_size + 1);
        let mut offset = 0usize;
        let mut layer_width = 1usize;
        offsets.push(offset);

        for _ in 0..self.left_context_size {
            offset = offset.checked_add(layer_width)?;
            offsets.push(offset);
            layer_width = layer_width.checked_mul(self.num_phones)?;
        }

        Some(offsets)
    }

    fn state_id_for_context_index(
        &self,
        state_index: usize,
    ) -> Result<StateId, ContextDependencyError> {
        StateId::try_from(state_index).map_err(|_| ContextDependencyError::StateSpaceOverflow {
            num_phones: self.num_phones,
            context_size: self.left_context_size,
        })
    }

    fn context_len_and_rank(
        &self,
        state_index: usize,
        layer_offsets: &[usize],
        radix_powers: &[usize],
    ) -> Result<(usize, usize), ContextDependencyError> {
        for len in 0..=self.left_context_size {
            let offset = layer_offsets[len];
            let width = radix_powers[len];
            let end =
                offset
                    .checked_add(width)
                    .ok_or(ContextDependencyError::StateSpaceOverflow {
                        num_phones: self.num_phones,
                        context_size: self.left_context_size,
                    })?;

            if (offset..end).contains(&state_index) {
                return Ok((len, state_index - offset));
            }
        }

        Err(ContextDependencyError::StateSpaceOverflow {
            num_phones: self.num_phones,
            context_size: self.left_context_size,
        })
    }

    fn context_from_rank(
        &self,
        context_len: usize,
        mut rank: usize,
        radix_powers: &[usize],
    ) -> Result<Vec<PhoneId>, ContextDependencyError> {
        let mut context = Vec::with_capacity(context_len);

        for depth in (0..context_len).rev() {
            let divisor = radix_powers[depth];
            let phone_index = rank / divisor;
            rank %= divisor;
            let phone = PhoneId::try_from(phone_index).map_err(|_| {
                ContextDependencyError::PhoneInventoryTooLarge {
                    num_phones: self.num_phones,
                    max_supported: Self::max_phone_inventory(),
                }
            })?;
            context.push(phone);
        }

        Ok(context)
    }

    fn extended_context_state_id(
        &self,
        context_len: usize,
        context_rank: usize,
        phone_index: usize,
        layer_offsets: &[usize],
        radix_powers: &[usize],
    ) -> Result<StateId, ContextDependencyError> {
        if self.left_context_size == 0 {
            return Ok(0);
        }

        let (next_len, next_rank) = if context_len < self.left_context_size {
            (
                context_len + 1,
                context_rank
                    .checked_mul(self.num_phones)
                    .and_then(|rank| rank.checked_add(phone_index))
                    .ok_or(ContextDependencyError::StateSpaceOverflow {
                        num_phones: self.num_phones,
                        context_size: self.left_context_size,
                    })?,
            )
        } else {
            let suffix_width = radix_powers[self.left_context_size - 1];
            (
                self.left_context_size,
                (context_rank % suffix_width)
                    .checked_mul(self.num_phones)
                    .and_then(|rank| rank.checked_add(phone_index))
                    .ok_or(ContextDependencyError::StateSpaceOverflow {
                        num_phones: self.num_phones,
                        context_size: self.left_context_size,
                    })?,
            )
        };

        let state_index = layer_offsets[next_len].checked_add(next_rank).ok_or(
            ContextDependencyError::StateSpaceOverflow {
                num_phones: self.num_phones,
                context_size: self.left_context_size,
            },
        )?;
        self.state_id_for_context_index(state_index)
    }

    fn checked_constructed_state_count(&self, context_states: usize) -> Option<usize> {
        if self.config.deterministic && self.config.boundary_symbol.is_some() {
            context_states.checked_add(context_states.saturating_sub(1))
        } else {
            Some(context_states)
        }
    }

    fn auxiliary_symbol_count(&self) -> usize {
        if !self.config.auxiliary_self_loops {
            return 0;
        }

        self.config
            .auxiliary_symbols
            .as_ref()
            .map(|range| range.end.saturating_sub(range.start) as usize)
            .unwrap_or(0)
    }

    fn arcs_per_context_state(&self) -> Option<usize> {
        let mut arcs = self.num_phones;
        arcs = arcs.checked_add(self.auxiliary_symbol_count())?;
        if self.config.deterministic && self.config.boundary_symbol.is_some() {
            arcs = arcs.checked_add(1)?;
        }
        Some(arcs)
    }

    fn checked_arc_count(&self) -> Option<usize> {
        self.checked_context_state_count()?
            .checked_mul(self.arcs_per_context_state()?)
    }

    fn validate_constructible_size(&self) -> Result<(), ContextDependencyError> {
        let context_states = self.checked_context_state_count().ok_or(
            ContextDependencyError::StateSpaceOverflow {
                num_phones: self.num_phones,
                context_size: self.left_context_size,
            },
        )?;

        let constructed_states = self.checked_constructed_state_count(context_states).ok_or(
            ContextDependencyError::StateSpaceOverflow {
                num_phones: self.num_phones,
                context_size: self.left_context_size,
            },
        )?;

        if constructed_states > StateId::MAX as usize {
            return Err(ContextDependencyError::StateSpaceOverflow {
                num_phones: self.num_phones,
                context_size: self.left_context_size,
            });
        }

        let arcs_per_state =
            self.arcs_per_context_state()
                .ok_or(ContextDependencyError::ArcSpaceOverflow {
                    num_states: context_states,
                    arcs_per_state: usize::MAX,
                })?;

        self.checked_arc_count()
            .ok_or(ContextDependencyError::ArcSpaceOverflow {
                num_states: context_states,
                arcs_per_state,
            })?;

        Ok(())
    }

    fn max_direct_label_symbol(&self) -> Option<PhoneId> {
        let inventory_max = self
            .num_phones
            .checked_sub(1)
            .and_then(|phone| PhoneId::try_from(phone).ok());

        let boundary = self
            .config
            .deterministic
            .then_some(self.config.boundary_symbol)
            .flatten();

        let auxiliary_max = (self.config.auxiliary_self_loops)
            .then(|| {
                self.config
                    .auxiliary_symbols
                    .as_ref()
                    .and_then(|range| range.end.checked_sub(1).filter(|_| range.start < range.end))
            })
            .flatten();

        [inventory_max, boundary, auxiliary_max]
            .into_iter()
            .flatten()
            .max()
    }

    fn label_base(&self) -> Result<PhoneId, ContextDependencyError> {
        self.validate_phone_inventory()?;

        if self.left_context_size == 0 {
            return Ok(1);
        }

        let Some(max_symbol) = self.max_direct_label_symbol() else {
            return Ok(1);
        };

        max_symbol
            .checked_add(2)
            .ok_or(ContextDependencyError::LabelAlphabetTooLarge { max_symbol })
    }

    fn label_space_error(&self, label_base: PhoneId) -> ContextDependencyError {
        ContextDependencyError::LabelSpaceOverflow {
            num_phones: self.num_phones,
            context_size: self.left_context_size,
            label_base,
        }
    }

    fn validate_label_space(&self, label_base: PhoneId) -> Result<(), ContextDependencyError> {
        if self.left_context_size == 0 || self.num_phones == 0 {
            return Ok(());
        }

        let base = u128::from(label_base);
        let context_digit = self.num_phones as u128;
        let mut max_label = self
            .max_direct_label_symbol()
            .map(u128::from)
            .unwrap_or_default();
        let mut multiplier = base;

        for depth in 0..self.left_context_size {
            let term = context_digit
                .checked_mul(multiplier)
                .ok_or_else(|| self.label_space_error(label_base))?;
            max_label = max_label
                .checked_add(term)
                .ok_or_else(|| self.label_space_error(label_base))?;

            if max_label > u128::from(PhoneId::MAX) {
                return Err(self.label_space_error(label_base));
            }

            if depth + 1 < self.left_context_size {
                multiplier = multiplier
                    .checked_mul(base)
                    .ok_or_else(|| self.label_space_error(label_base))?;
            }
        }

        Ok(())
    }

    /// Compute context-dependent phone label.
    ///
    /// Encodes (left_context, center_phone) as a single label using offset-by-1
    /// encoding to ensure injectivity even when phone 0 is in the context.
    ///
    /// # Encoding
    ///
    /// Uses a mixed-radix positional encoding where each context phone is offset
    /// by 1 to ensure phone 0 contributes a non-zero value:
    ///
    /// ```text
    /// label = center + sum((ctx[i] + 1) * base^(i+1))
    /// ```
    ///
    /// This ensures that contexts [0] and [] produce different labels, as
    /// phone 0 in context contributes (0 + 1) = 1, not 0.
    fn compute_cd_label_for_context_checked(
        &self,
        left_context: &[PhoneId],
        center_phone: PhoneId,
        label_base: PhoneId,
    ) -> Result<PhoneId, ContextDependencyError> {
        if left_context.is_empty() {
            return Ok(center_phone);
        }

        let mut label = u64::from(center_phone);
        let base = u64::from(label_base);
        let mut multiplier = base;

        // Process context phones from most recent to oldest
        for (depth, &ctx_phone) in left_context.iter().rev().enumerate() {
            if usize::try_from(ctx_phone)
                .map(|phone| phone >= self.num_phones)
                .unwrap_or(true)
            {
                return Err(ContextDependencyError::ContextPhoneOutOfRange {
                    phone: ctx_phone,
                    num_phones: self.num_phones,
                });
            }

            // Offset by 1 so phone 0 contributes (0 + 1) * multiplier, not 0
            let digit = u64::from(ctx_phone) + 1;
            let term = digit.checked_mul(multiplier).ok_or(
                ContextDependencyError::LabelSpaceOverflow {
                    num_phones: self.num_phones,
                    context_size: left_context.len(),
                    label_base,
                },
            )?;
            label = label
                .checked_add(term)
                .ok_or(ContextDependencyError::LabelSpaceOverflow {
                    num_phones: self.num_phones,
                    context_size: left_context.len(),
                    label_base,
                })?;

            if label > u64::from(PhoneId::MAX) {
                return Err(ContextDependencyError::LabelSpaceOverflow {
                    num_phones: self.num_phones,
                    context_size: left_context.len(),
                    label_base,
                });
            }

            if depth + 1 < left_context.len() {
                multiplier = multiplier.checked_mul(base).ok_or(
                    ContextDependencyError::LabelSpaceOverflow {
                        num_phones: self.num_phones,
                        context_size: left_context.len(),
                        label_base,
                    },
                )?;
            }
        }

        PhoneId::try_from(label).map_err(|_| ContextDependencyError::LabelSpaceOverflow {
            num_phones: self.num_phones,
            context_size: left_context.len(),
            label_base,
        })
    }

    #[cfg(test)]
    fn compute_cd_label_checked(
        &self,
        state: &ContextState,
        center_phone: PhoneId,
        label_base: PhoneId,
    ) -> Result<PhoneId, ContextDependencyError> {
        self.compute_cd_label_for_context_checked(&state.left_context, center_phone, label_base)
    }

    #[cfg(test)]
    fn compute_cd_label(&self, state: &ContextState, center_phone: PhoneId) -> PhoneId {
        let label_base = self.label_base();
        assert!(
            label_base.is_ok(),
            "test label base should fit: {label_base:?}"
        );
        let label_base = label_base.unwrap_or_default();
        let label = self.compute_cd_label_checked(state, center_phone, label_base);
        assert!(
            label.is_ok(),
            "test context-dependent label should fit: {label:?}"
        );
        label.unwrap_or_default()
    }

    /// Add boundary handling for deterministic construction.
    ///
    /// For deterministic context-dependency transducers, we need to handle word
    /// boundaries by emitting the remaining context when the boundary symbol is seen.
    ///
    /// This allows proper handling of word-final context without look-ahead.
    fn add_boundary_handling(
        &self,
        fst: &mut VectorWfst<PhoneId, W>,
        context_state_count: usize,
        layer_offsets: &[usize],
        radix_powers: &[usize],
        boundary: PhoneId,
        label_base: PhoneId,
    ) -> Result<(), ContextDependencyError> {
        // For each state with accumulated context, add a boundary transition
        // that outputs a context-dependent label including the boundary
        for state_index in 0..context_state_count {
            let state_id = self.state_id_for_context_index(state_index)?;
            let (context_len, context_rank) =
                self.context_len_and_rank(state_index, layer_offsets, radix_powers)?;

            // Only process states with context (not the initial empty state)
            if context_len == 0 {
                // For initial state, just add a boundary self-loop
                fst.add_arc(state_id, Some(boundary), Some(boundary), state_id, W::one());
                continue;
            }

            // For states with context, add a transition that outputs
            // the context-dependent boundary label
            let context = self.context_from_rank(context_len, context_rank, radix_powers)?;
            let boundary_label =
                self.compute_cd_label_for_context_checked(&context, boundary, label_base)?;

            // Create a boundary-exit state for this context
            let exit_state = fst.add_state();
            fst.set_final(exit_state, W::one());

            // Add transition: current_state --boundary:cd_boundary_label--> exit_state
            fst.add_arc(
                state_id,
                Some(boundary),
                Some(boundary_label),
                exit_state,
                W::one(),
            );
        }

        Ok(())
    }
}

/// Builder for triphone context-dependency transducers.
///
/// A triphone considers one phone of left and right context.
pub struct TriphoneBuilder<W: Semiring> {
    inner: ContextDependencyBuilder<W>,
}

impl<W: Semiring> TriphoneBuilder<W> {
    /// Create a new triphone builder.
    ///
    /// # Arguments
    ///
    /// * `num_phones` - Number of phones in the inventory
    pub fn new(num_phones: usize) -> Self {
        Self {
            inner: ContextDependencyBuilder::new(num_phones, 1, 1),
        }
    }

    /// Set configuration options.
    pub fn config(mut self, config: ContextDependencyConfig) -> Self {
        self.inner.config = config;
        self
    }

    /// Enable deterministic construction.
    pub fn deterministic(mut self, boundary_symbol: PhoneId) -> Self {
        self.inner = self.inner.deterministic(boundary_symbol);
        self
    }

    /// Enable auxiliary symbol self-loops.
    pub fn with_auxiliary_symbols(mut self, range: std::ops::Range<PhoneId>) -> Self {
        self.inner = self.inner.with_auxiliary_symbols(range);
        self
    }

    /// Build the triphone transducer.
    ///
    /// # Complexity
    ///
    /// - States: O(n) for n phones
    /// - Arcs: O(n²) for n phones
    pub fn build(&self) -> VectorWfst<PhoneId, W> {
        self.inner.build()
    }

    /// Build the triphone transducer with validation diagnostics.
    pub fn try_build(&self) -> Result<VectorWfst<PhoneId, W>, ContextDependencyError> {
        self.inner.try_build()
    }

    /// Get checked expected number of context states.
    pub fn checked_expected_states(&self) -> Option<usize> {
        self.inner.checked_context_state_count()
    }

    /// Get expected number of states.
    pub fn expected_states(&self) -> usize {
        self.checked_expected_states().unwrap_or(usize::MAX)
    }

    /// Get checked expected number of configured outgoing arcs.
    pub fn checked_expected_arcs(&self) -> Option<usize> {
        self.inner.checked_arc_count()
    }

    /// Get expected number of arcs.
    pub fn expected_arcs(&self) -> usize {
        self.checked_expected_arcs().unwrap_or(usize::MAX)
    }
}

/// Builder for tetraphone context-dependency transducers.
///
/// A tetraphone stores two phones of left-context history.
pub struct TetraploneBuilder<W: Semiring> {
    inner: ContextDependencyBuilder<W>,
}

impl<W: Semiring> TetraploneBuilder<W> {
    /// Create a new tetraphone builder.
    ///
    /// # Arguments
    ///
    /// * `num_phones` - Number of phones in the inventory
    pub fn new(num_phones: usize) -> Self {
        Self {
            inner: ContextDependencyBuilder::new(num_phones, 2, 2),
        }
    }

    /// Set configuration options.
    pub fn config(mut self, config: ContextDependencyConfig) -> Self {
        self.inner.config = config;
        self
    }

    /// Enable deterministic construction.
    pub fn deterministic(mut self, boundary_symbol: PhoneId) -> Self {
        self.inner = self.inner.deterministic(boundary_symbol);
        self
    }

    /// Enable auxiliary symbol self-loops.
    pub fn with_auxiliary_symbols(mut self, range: std::ops::Range<PhoneId>) -> Self {
        self.inner = self.inner.with_auxiliary_symbols(range);
        self
    }

    /// Build the tetraphone transducer.
    ///
    /// # Complexity
    ///
    /// - States: O(n²) for n phones
    /// - Arcs: O(n³) for n phones
    pub fn build(&self) -> VectorWfst<PhoneId, W> {
        self.inner.build()
    }

    /// Build the tetraphone transducer with validation diagnostics.
    pub fn try_build(&self) -> Result<VectorWfst<PhoneId, W>, ContextDependencyError> {
        self.inner.try_build()
    }

    /// Get checked expected number of context states.
    pub fn checked_expected_states(&self) -> Option<usize> {
        self.inner.checked_context_state_count()
    }

    /// Get expected number of states.
    pub fn expected_states(&self) -> usize {
        self.checked_expected_states().unwrap_or(usize::MAX)
    }

    /// Get checked expected number of configured outgoing arcs.
    pub fn checked_expected_arcs(&self) -> Option<usize> {
        self.inner.checked_arc_count()
    }

    /// Get expected number of arcs.
    pub fn expected_arcs(&self) -> usize {
        self.checked_expected_arcs().unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semiring::LogWeight;
    use crate::wfst::{Wfst, NO_STATE};

    #[test]
    fn test_context_state_initial() {
        let state = ContextState::initial();
        assert!(state.left_context.is_empty());
    }

    #[test]
    fn test_context_state_extend() {
        let state = ContextState::initial();

        let state1 = state.extend(1, 2);
        assert_eq!(state1.left_context, vec![1]);

        let state2 = state1.extend(2, 2);
        assert_eq!(state2.left_context, vec![1, 2]);

        // Should trim to max context
        let state3 = state2.extend(3, 2);
        assert_eq!(state3.left_context, vec![2, 3]);
    }

    #[test]
    fn test_context_state_extend_zero_context() {
        let state = ContextState::with_context(vec![1, 2]);
        let extended = state.extend(3, 0);

        assert!(extended.left_context.is_empty());
    }

    #[test]
    fn test_context_state_extend_trims_oversized_context() {
        let state = ContextState::with_context(vec![1, 2, 3, 4]);

        assert_eq!(state.extend(5, 2).left_context, vec![4, 5]);
        assert_eq!(state.extend(5, 3).left_context, vec![3, 4, 5]);
    }

    #[test]
    fn test_triphone_builder() {
        let builder = TriphoneBuilder::<LogWeight>::new(5);
        let fst = builder.build();

        // Should have states for empty and 1-phone context
        assert!(fst.num_states() >= 1);
        assert!(fst.start() != NO_STATE);
    }

    #[test]
    fn test_triphone_state_count() {
        let builder = TriphoneBuilder::<LogWeight>::new(3);
        let fst = builder.build();

        // 1 (empty) + 3 (1-phone context) = 4 states
        assert_eq!(fst.num_states(), 4);
    }

    #[test]
    fn test_triphone_arc_count() {
        let builder = TriphoneBuilder::<LogWeight>::new(3);
        let fst = builder.build();

        // Count total arcs
        let total_arcs: usize = (0..fst.num_states() as StateId)
            .map(|s| fst.transitions(s).len())
            .sum();

        // Expected: 4 states * 3 phones = 12 arcs
        assert_eq!(total_arcs, 12);
    }

    #[test]
    fn test_tetraphone_state_count() {
        let builder = TetraploneBuilder::<LogWeight>::new(3);
        let fst = builder.build();

        // 1 (empty) + 3 (1-phone) + 9 (2-phone) = 13 states
        assert_eq!(fst.num_states(), 13);
    }

    #[test]
    fn test_indexed_context_state_order() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(3, 2, 1);
        let layer_offsets = builder.checked_context_layer_offsets().unwrap();
        let radix_powers = builder.checked_radix_powers().unwrap();
        let state_count = builder.checked_context_state_count().unwrap();

        let contexts: Vec<Vec<PhoneId>> = (0..state_count)
            .map(|state_index| {
                let (context_len, context_rank) = builder
                    .context_len_and_rank(state_index, &layer_offsets, &radix_powers)
                    .unwrap();
                builder
                    .context_from_rank(context_len, context_rank, &radix_powers)
                    .unwrap()
            })
            .collect();

        assert_eq!(
            contexts,
            vec![
                vec![],
                vec![0],
                vec![1],
                vec![2],
                vec![0, 0],
                vec![0, 1],
                vec![0, 2],
                vec![1, 0],
                vec![1, 1],
                vec![1, 2],
                vec![2, 0],
                vec![2, 1],
                vec![2, 2],
            ]
        );
    }

    #[test]
    fn test_indexed_context_extension_targets() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(3, 2, 1);
        let layer_offsets = builder.checked_context_layer_offsets().unwrap();
        let radix_powers = builder.checked_radix_powers().unwrap();

        assert_eq!(
            builder
                .extended_context_state_id(0, 0, 2, &layer_offsets, &radix_powers)
                .unwrap(),
            3
        );
        assert_eq!(
            builder
                .extended_context_state_id(1, 1, 0, &layer_offsets, &radix_powers)
                .unwrap(),
            7
        );
        assert_eq!(
            builder
                .extended_context_state_id(2, 5, 0, &layer_offsets, &radix_powers)
                .unwrap(),
            10
        );
    }

    #[test]
    fn test_cd_label_encoding() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(10, 1, 1);

        let empty = ContextState::initial();
        let with_ctx = ContextState::with_context(vec![5]);

        // Label should encode context
        let label1 = builder.compute_cd_label(&empty, 3);
        let label2 = builder.compute_cd_label(&with_ctx, 3);

        // Same center phone but different context should give different labels
        assert_ne!(label1, label2);
    }

    #[test]
    fn test_all_states_final() {
        let builder = TriphoneBuilder::<LogWeight>::new(3);
        let fst = builder.build();

        // All states should be final for word boundary handling
        for id in 0..fst.num_states() as StateId {
            assert!(fst.is_final(id));
        }
    }

    /// Test that phone 0 in context produces a different label than empty context.
    ///
    /// This verifies the offset-by-1 encoding fix.
    #[test]
    fn test_phone_0_contributes_to_label() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(10, 2, 1);

        let empty = ContextState::initial();
        let with_zero = ContextState::with_context(vec![0]);

        // With the fix, these should produce different labels
        let label_empty = builder.compute_cd_label(&empty, 5);
        let label_with_zero = builder.compute_cd_label(&with_zero, 5);

        assert_ne!(
            label_empty, label_with_zero,
            "Phone 0 in context must produce different label than empty context. \
             Empty: {}, With [0]: {}",
            label_empty, label_with_zero
        );
    }

    /// Test that different positions of phone 0 produce different labels.
    #[test]
    fn test_different_phone_0_positions() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(10, 2, 1);

        // [0, 1] vs [1, 0] should produce different labels
        let ctx_01 = ContextState::with_context(vec![0, 1]);
        let ctx_10 = ContextState::with_context(vec![1, 0]);

        let label_01 = builder.compute_cd_label(&ctx_01, 5);
        let label_10 = builder.compute_cd_label(&ctx_10, 5);

        assert_ne!(
            label_01, label_10,
            "Different phone 0 positions must produce different labels. \
             [0,1]: {}, [1,0]: {}",
            label_01, label_10
        );
    }

    /// Test that the encoding is injective: different (context, center) pairs
    /// always produce different labels.
    #[test]
    fn test_cd_label_injectivity_with_phone_0() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(5, 2, 1);

        // Collect all (context, center) -> label mappings for small cases
        let mut seen_labels: std::collections::HashMap<PhoneId, (Vec<PhoneId>, PhoneId)> =
            std::collections::HashMap::new();

        // Test all combinations of context length 0, 1, 2 with phones 0..3
        let phones: Vec<PhoneId> = vec![0, 1, 2];

        for center in phones.iter().copied() {
            // Empty context
            let empty = ContextState::initial();
            let label = builder.compute_cd_label(&empty, center);
            let previous = seen_labels.insert(label, (vec![], center));
            assert_eq!(
                previous, None,
                "label {label} also maps to empty context and center {center}"
            );

            // Single-phone context
            for &ctx0 in &phones {
                let ctx = ContextState::with_context(vec![ctx0]);
                let label = builder.compute_cd_label(&ctx, center);
                let previous = seen_labels.insert(label, (vec![ctx0], center));
                assert_eq!(
                    previous, None,
                    "label {label} also maps to context [{ctx0}] and center {center}"
                );
            }

            // Two-phone context
            for &ctx0 in &phones {
                for &ctx1 in &phones {
                    let ctx = ContextState::with_context(vec![ctx0, ctx1]);
                    let label = builder.compute_cd_label(&ctx, center);
                    let previous = seen_labels.insert(label, (vec![ctx0, ctx1], center));
                    assert_eq!(
                        previous, None,
                        "label {label} also maps to context [{ctx0}, {ctx1}] and center {center}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_label_space_overflow_is_rejected_before_building() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(65_536, 1, 1);
        let label_base = builder.label_base().unwrap();

        assert_eq!(label_base, 65_537);
        assert_eq!(
            builder.validate_label_space(label_base),
            Err(ContextDependencyError::LabelSpaceOverflow {
                num_phones: 65_536,
                context_size: 1,
                label_base,
            })
        );
        assert!(matches!(
            builder.try_build(),
            Err(ContextDependencyError::LabelSpaceOverflow { .. })
        ));
    }

    #[test]
    fn test_build_returns_empty_for_invalid_label_space() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(65_536, 1, 1);
        let fst = builder.build();

        assert!(fst.is_empty());
        assert_eq!(fst.start(), NO_STATE);
    }

    #[test]
    fn test_checked_label_rejects_out_of_range_context_phone() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(3, 1, 1);
        let state = ContextState::with_context(vec![3]);
        let label_base = builder.label_base().unwrap();

        assert_eq!(
            builder.compute_cd_label_checked(&state, 1, label_base),
            Err(ContextDependencyError::ContextPhoneOutOfRange {
                phone: 3,
                num_phones: 3,
            })
        );
    }

    #[test]
    fn test_label_base_accounts_for_deterministic_boundary() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(3, 1, 1).deterministic(100);
        let state = ContextState::with_context(vec![2]);
        let label_base = builder.label_base().unwrap();
        let boundary_label = builder
            .compute_cd_label_checked(&state, 100, label_base)
            .unwrap();

        assert_eq!(label_base, 102);
        assert!(boundary_label > 100);
    }

    #[test]
    fn test_expected_counts_saturate_when_checked_counts_overflow() {
        let builder = TetraploneBuilder::<LogWeight>::new(usize::MAX);

        assert_eq!(builder.checked_expected_states(), None);
        assert_eq!(builder.checked_expected_arcs(), None);
        assert_eq!(builder.expected_states(), usize::MAX);
        assert_eq!(builder.expected_arcs(), usize::MAX);
    }

    #[test]
    fn test_triphone_auxiliary_symbol_forwarder() {
        let builder = TriphoneBuilder::<LogWeight>::new(2).with_auxiliary_symbols(100..102);
        let fst = builder.try_build().unwrap();
        let total_arcs: usize = (0..fst.num_states() as StateId)
            .map(|state| fst.transitions(state).len())
            .sum();

        assert_eq!(fst.num_states(), 3);
        assert_eq!(builder.checked_expected_arcs(), Some(12));
        assert_eq!(builder.expected_arcs(), 12);
        assert_eq!(total_arcs, 12);
    }

    #[test]
    fn test_deterministic_try_build_adds_boundary_exit_shape() {
        let builder = ContextDependencyBuilder::<LogWeight>::new(2, 1, 1).deterministic(99);
        let fst = builder.try_build().unwrap();
        let total_arcs: usize = (0..fst.num_states() as StateId)
            .map(|state| fst.transitions(state).len())
            .sum();

        assert_eq!(fst.num_states(), 5);
        assert_eq!(total_arcs, 9);
        for state in 0..fst.num_states() as StateId {
            assert!(fst.is_final(state));
        }
    }

    #[test]
    fn test_expected_arcs_include_deterministic_and_auxiliary_arcs() {
        let triphone = TriphoneBuilder::<LogWeight>::new(2).deterministic(99);
        let triphone_fst = triphone.try_build().unwrap();
        let triphone_arcs: usize = (0..triphone_fst.num_states() as StateId)
            .map(|state| triphone_fst.transitions(state).len())
            .sum();
        assert_eq!(triphone.checked_expected_arcs(), Some(9));
        assert_eq!(triphone.expected_arcs(), 9);
        assert_eq!(triphone_arcs, triphone.expected_arcs());

        let tetraphone = TetraploneBuilder::<LogWeight>::new(2)
            .deterministic(99)
            .with_auxiliary_symbols(100..101);
        let tetraphone_fst = tetraphone.try_build().unwrap();
        let tetraphone_arcs: usize = (0..tetraphone_fst.num_states() as StateId)
            .map(|state| tetraphone_fst.transitions(state).len())
            .sum();
        assert_eq!(tetraphone.checked_expected_arcs(), Some(28));
        assert_eq!(tetraphone.expected_arcs(), 28);
        assert_eq!(tetraphone_arcs, tetraphone.expected_arcs());
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::semiring::LogWeight;
    use crate::wfst::{Wfst, NO_STATE};
    use proptest::prelude::*;

    // -------------------------------------------------------------------------
    // ContextState Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Initial state has empty context.
        #[test]
        fn initial_state_empty(_seed in any::<u64>()) {
            let state = ContextState::initial();
            prop_assert!(state.left_context.is_empty());
        }

        /// Extending with a phone adds it to context.
        #[test]
        fn extend_adds_phone(phone in 0u32..100, max_ctx in 1usize..5) {
            let state = ContextState::initial();
            let extended = state.extend(phone, max_ctx);
            prop_assert!(extended.left_context.contains(&phone));
        }

        /// Context length never exceeds max_context.
        #[test]
        fn extend_respects_max_context(
            phones in prop::collection::vec(0u32..100, 1..20),
            max_ctx in 1usize..5
        ) {
            let mut state = ContextState::initial();
            for &phone in &phones {
                state = state.extend(phone, max_ctx);
                prop_assert!(state.left_context.len() <= max_ctx);
            }
        }

        /// When context exceeds max, oldest phone is removed.
        #[test]
        fn extend_removes_oldest_when_full(max_ctx in 1usize..5) {
            let mut state = ContextState::initial();

            // Fill context
            for i in 0..max_ctx as u32 {
                state = state.extend(i, max_ctx);
            }
            prop_assert_eq!(state.left_context.len(), max_ctx);

            // Add one more - oldest (0) should be removed
            let new_phone = max_ctx as u32 + 100;
            state = state.extend(new_phone, max_ctx);

            prop_assert_eq!(state.left_context.len(), max_ctx);
            prop_assert!(!state.left_context.contains(&0));
            prop_assert!(state.left_context.contains(&new_phone));
        }

        /// with_context preserves the given context.
        #[test]
        fn with_context_preserves(context in prop::collection::vec(0u32..100, 0..5)) {
            let state = ContextState::with_context(context.clone());
            prop_assert_eq!(state.left_context, context);
        }

        /// ContextState equality is based on context content.
        #[test]
        fn context_state_equality(context in prop::collection::vec(0u32..50, 0..4)) {
            let state1 = ContextState::with_context(context.clone());
            let state2 = ContextState::with_context(context);
            prop_assert_eq!(state1, state2);
        }

        /// Different contexts produce different states.
        #[test]
        fn different_contexts_different_states(
            ctx1 in prop::collection::vec(0u32..50, 1..3),
            ctx2 in prop::collection::vec(50u32..100, 1..3)
        ) {
            let state1 = ContextState::with_context(ctx1);
            let state2 = ContextState::with_context(ctx2);
            prop_assert_ne!(state1, state2);
        }
    }

    // -------------------------------------------------------------------------
    // ContextDependencyConfig Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        /// Default config is non-deterministic.
        #[test]
        fn default_config_non_deterministic(_seed in any::<u64>()) {
            let config = ContextDependencyConfig::default();
            prop_assert!(!config.deterministic);
            prop_assert!(config.boundary_symbol.is_none());
        }

        /// Default config has no auxiliary symbols.
        #[test]
        fn default_config_no_aux(_seed in any::<u64>()) {
            let config = ContextDependencyConfig::default();
            prop_assert!(!config.auxiliary_self_loops);
            prop_assert!(config.auxiliary_symbols.is_none());
        }
    }

    // -------------------------------------------------------------------------
    // ContextDependencyBuilder Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        /// CD label encoding is deterministic.
        #[test]
        fn cd_label_deterministic(
            num_phones in 2usize..10,
            context in prop::collection::vec(0u32..10, 0..2),
            center in 0u32..10
        ) {
            let context: Vec<u32> = context
                .into_iter()
                .map(|phone| phone % num_phones as u32)
                .collect();
            let center = center % num_phones as u32;
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 2, 1);
            let state = ContextState::with_context(context);

            let label1 = builder.compute_cd_label(&state, center);
            let label2 = builder.compute_cd_label(&state, center);

            prop_assert_eq!(label1, label2);
        }

        /// Different contexts produce different CD labels.
        #[test]
        fn cd_label_context_sensitivity(
            num_phones in 5usize..15,
            center in 0u32..5
        ) {
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 1, 1);

            let empty = ContextState::initial();
            let with_ctx = ContextState::with_context(vec![1]);

            let label1 = builder.compute_cd_label(&empty, center);
            let label2 = builder.compute_cd_label(&with_ctx, center);

            // Labels should differ when context differs
            prop_assert_ne!(label1, label2);
        }

        /// Different center phones produce different CD labels.
        #[test]
        fn cd_label_center_sensitivity(
            num_phones in 5usize..15,
            center1 in 0u32..5,
            center2 in 5u32..10
        ) {
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 1, 1);
            let state = ContextState::initial();

            let label1 = builder.compute_cd_label(&state, center1);
            let label2 = builder.compute_cd_label(&state, center2);

            prop_assert_ne!(label1, label2);
        }

        /// Builder config method updates config.
        #[test]
        fn builder_config_updates(
            num_phones in 2usize..10,
            deterministic in any::<bool>()
        ) {
            let config = ContextDependencyConfig {
                deterministic,
                ..Default::default()
            };

            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 1, 1)
                .config(config);

            prop_assert_eq!(builder.config.deterministic, deterministic);
        }

        /// Deterministic method sets appropriate fields.
        #[test]
        fn builder_deterministic_sets_fields(
            num_phones in 2usize..10,
            boundary in 0u32..100
        ) {
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 1, 1)
                .deterministic(boundary);

            prop_assert!(builder.config.deterministic);
            prop_assert_eq!(builder.config.boundary_symbol, Some(boundary));
        }

        /// Auxiliary symbols method sets appropriate fields.
        #[test]
        fn builder_aux_symbols_sets_fields(
            num_phones in 2usize..10,
            start in 100u32..200,
            end in 200u32..300
        ) {
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 1, 1)
                .with_auxiliary_symbols(start..end);

            prop_assert!(builder.config.auxiliary_self_loops);
            prop_assert_eq!(builder.config.auxiliary_symbols, Some(start..end));
        }
    }

    // -------------------------------------------------------------------------
    // TriphoneBuilder Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// Triphone FST has expected state count.
        #[test]
        fn triphone_state_count(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            // States: 1 (empty) + num_phones (1-phone contexts)
            prop_assert_eq!(fst.num_states(), 1 + num_phones);
        }

        /// Triphone FST has expected arc count.
        #[test]
        fn triphone_arc_count(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            let total_arcs: usize = (0..fst.num_states() as StateId)
                .map(|s| fst.transitions(s).len())
                .sum();

            // Arcs: (1 + num_phones) states * num_phones arcs each
            prop_assert_eq!(total_arcs, (1 + num_phones) * num_phones);
        }

        /// Triphone expected_states matches actual.
        #[test]
        fn triphone_expected_states_accurate(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            prop_assert_eq!(fst.num_states(), builder.expected_states());
        }

        /// Triphone expected_arcs matches actual.
        #[test]
        fn triphone_expected_arcs_accurate(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            let total_arcs: usize = (0..fst.num_states() as StateId)
                .map(|s| fst.transitions(s).len())
                .sum();

            prop_assert_eq!(total_arcs, builder.expected_arcs());
        }

        /// All triphone states are final.
        #[test]
        fn triphone_all_states_final(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            for id in 0..fst.num_states() as StateId {
                prop_assert!(fst.is_final(id));
            }
        }

        /// Triphone FST has a valid start state.
        #[test]
        fn triphone_has_start(num_phones in 2usize..8) {
            let builder = TriphoneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            prop_assert!(fst.start() != NO_STATE);
        }
    }

    // -------------------------------------------------------------------------
    // TetraploneBuilder Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(15))]

        /// Tetraphone FST has expected state count.
        #[test]
        fn tetraphone_state_count(num_phones in 2usize..5) {
            let builder = TetraploneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            // States: 1 + n + n²
            let expected = 1 + num_phones + num_phones * num_phones;
            prop_assert_eq!(fst.num_states(), expected);
        }

        /// Tetraphone expected_states matches actual.
        #[test]
        fn tetraphone_expected_states_accurate(num_phones in 2usize..5) {
            let builder = TetraploneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            prop_assert_eq!(fst.num_states(), builder.expected_states());
        }

        /// Tetraphone expected_arcs matches actual.
        #[test]
        fn tetraphone_expected_arcs_accurate(num_phones in 2usize..5) {
            let builder = TetraploneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            let total_arcs: usize = (0..fst.num_states() as StateId)
                .map(|s| fst.transitions(s).len())
                .sum();

            prop_assert_eq!(total_arcs, builder.expected_arcs());
        }

        /// All tetraphone states are final.
        #[test]
        fn tetraphone_all_states_final(num_phones in 2usize..5) {
            let builder = TetraploneBuilder::<LogWeight>::new(num_phones);
            let fst = builder.build();

            for id in 0..fst.num_states() as StateId {
                prop_assert!(fst.is_final(id));
            }
        }

        /// Tetraphone has more states than triphone.
        #[test]
        fn tetraphone_more_states_than_triphone(num_phones in 3usize..6) {
            let tri = TriphoneBuilder::<LogWeight>::new(num_phones);
            let tetra = TetraploneBuilder::<LogWeight>::new(num_phones);

            prop_assert!(tetra.expected_states() > tri.expected_states());
        }

        /// Tetraphone has more arcs than triphone.
        #[test]
        fn tetraphone_more_arcs_than_triphone(num_phones in 3usize..6) {
            let tri = TriphoneBuilder::<LogWeight>::new(num_phones);
            let tetra = TetraploneBuilder::<LogWeight>::new(num_phones);

            prop_assert!(tetra.expected_arcs() > tri.expected_arcs());
        }
    }

    // -------------------------------------------------------------------------
    // CD Label Injectivity Properties
    // -------------------------------------------------------------------------

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// CD label encoding is injective for all contexts including phone 0.
        ///
        /// The offset-by-1 encoding ensures that phone 0 in context contributes
        /// a non-zero value to the label, making the encoding fully injective.
        #[test]
        fn cd_label_injective(
            num_phones in 3usize..8,
            ctx1 in prop::collection::vec(0u32..3, 0..2),
            ctx2 in prop::collection::vec(0u32..3, 0..2),
            center1 in 0u32..3,
            center2 in 0u32..3
        ) {
            // Ensure phones are within range
            let ctx1: Vec<u32> = ctx1.into_iter().map(|p| p % num_phones as u32).collect();
            let ctx2: Vec<u32> = ctx2.into_iter().map(|p| p % num_phones as u32).collect();
            let center1 = center1 % num_phones as u32;
            let center2 = center2 % num_phones as u32;

            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 2, 1);

            let state1 = ContextState::with_context(ctx1.clone());
            let state2 = ContextState::with_context(ctx2.clone());

            let label1 = builder.compute_cd_label(&state1, center1);
            let label2 = builder.compute_cd_label(&state2, center2);

            // If labels are equal, contexts and centers must be equal
            if label1 == label2 {
                prop_assert_eq!(ctx1, ctx2);
                prop_assert_eq!(center1, center2);
            }
        }

        /// Phone 0 in context produces different label than empty context.
        #[test]
        fn phone_0_context_differs_from_empty(
            num_phones in 3usize..10,
            center in 0u32..5
        ) {
            let center = center % num_phones as u32;
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 2, 1);

            let empty = ContextState::initial();
            let with_zero = ContextState::with_context(vec![0]);

            let label_empty = builder.compute_cd_label(&empty, center);
            let label_with_zero = builder.compute_cd_label(&with_zero, center);

            prop_assert_ne!(
                label_empty,
                label_with_zero,
                "Phone 0 in context must produce different label than empty context"
            );
        }

        /// Different positions of phone 0 produce different labels.
        #[test]
        fn phone_0_position_matters(
            num_phones in 3usize..10,
            center in 0u32..5
        ) {
            let center = center % num_phones as u32;
            let builder = ContextDependencyBuilder::<LogWeight>::new(num_phones, 2, 1);

            // [0, 1] vs [1, 0] should produce different labels
            let ctx_01 = ContextState::with_context(vec![0, 1]);
            let ctx_10 = ContextState::with_context(vec![1, 0]);

            let label_01 = builder.compute_cd_label(&ctx_01, center);
            let label_10 = builder.compute_cd_label(&ctx_10, center);

            prop_assert_ne!(
                label_01,
                label_10,
                "Different phone 0 positions must produce different labels"
            );
        }
    }
}
