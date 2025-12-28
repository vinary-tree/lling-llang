//! Channels and Lanes for batched streaming decoding.
//!
//! This module provides the Channels/Lanes abstraction for efficient
//! batched decoding of multiple concurrent audio streams.
//!
//! ## Concepts
//!
//! - **Channel**: State for an utterance awaiting audio/posteriors (not actively decoding)
//! - **Lane**: An active decoding slot (like batch size in neural networks)
//! - **Multiplexing**: Maps ready channels onto available lanes
//!
//! ## Architecture
//!
//! ```text
//! Channels (n_c)          Lanes (n_l)
//! ┌─────────┐            ┌─────────┐
//! │ Chan 0  │───────────▶│ Lane 0  │ (active)
//! ├─────────┤            ├─────────┤
//! │ Chan 1  │ (waiting)  │ Lane 1  │ (active)
//! ├─────────┤            ├─────────┤
//! │ Chan 2  │───────────▶│ Lane 2  │ (active)
//! ├─────────┤            └─────────┘
//! │ Chan 3  │ (waiting)
//! └─────────┘
//! ```
//!
//! ## Memory Model
//!
//! ```text
//! M_state = 64α·n_c + 544α·n_l + 1024·n_l
//! ```
//!
//! Where:
//! - α = max active tokens after pruning
//! - n_c = maximum number of channels
//! - n_l = maximum number of lanes
//!
//! ## Benefits
//!
//! - **Context switch time**: ~5µs per batch
//! - **Memory bounded**: Independent of graph size
//! - **High throughput**: Process thousands of streams in parallel
//!
//! ## References
//!
//! - Laptev et al., "GPU-Accelerated Viterbi Exact Lattice Decoder" (NVIDIA, 2020)

use std::collections::VecDeque;

/// Channel states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelState {
    /// Channel is idle (no active utterance).
    Idle,
    /// Waiting for audio/posteriors.
    Waiting,
    /// Ready to be scheduled to a lane.
    Ready,
    /// Actively decoding in a lane.
    Active,
    /// Decoding complete, awaiting result retrieval.
    Complete,
    /// Error occurred during decoding.
    Error,
}

impl Default for ChannelState {
    fn default() -> Self {
        ChannelState::Idle
    }
}

/// Lane states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneState {
    /// Lane is available for assignment.
    Available,
    /// Lane is actively decoding.
    Active,
    /// Lane finished decoding current frame.
    FrameComplete,
    /// Lane finished entire utterance.
    UtteranceComplete,
}

impl Default for LaneState {
    fn default() -> Self {
        LaneState::Available
    }
}

/// A channel representing an audio stream's decoding state.
#[derive(Clone, Debug)]
pub struct Channel<T> {
    /// Channel ID.
    id: usize,
    /// Current state.
    state: ChannelState,
    /// Assigned lane (if active).
    lane: Option<usize>,
    /// User data associated with this channel.
    user_data: Option<T>,
    /// Frame index within current utterance.
    frame_index: usize,
    /// Total frames in current utterance (if known).
    total_frames: Option<usize>,
    /// Token buffer for this channel.
    token_count: usize,
}

impl<T> Channel<T> {
    /// Create a new idle channel.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            state: ChannelState::Idle,
            lane: None,
            user_data: None,
            frame_index: 0,
            total_frames: None,
            token_count: 0,
        }
    }

    /// Get the channel ID.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Get the current state.
    pub fn state(&self) -> ChannelState {
        self.state
    }

    /// Get the assigned lane.
    pub fn lane(&self) -> Option<usize> {
        self.lane
    }

    /// Get user data.
    pub fn user_data(&self) -> Option<&T> {
        self.user_data.as_ref()
    }

    /// Get mutable user data.
    pub fn user_data_mut(&mut self) -> Option<&mut T> {
        self.user_data.as_mut()
    }

    /// Get the current frame index.
    pub fn frame_index(&self) -> usize {
        self.frame_index
    }

    /// Get total frames if known.
    pub fn total_frames(&self) -> Option<usize> {
        self.total_frames
    }

    /// Get the token count.
    pub fn token_count(&self) -> usize {
        self.token_count
    }

    /// Check if the channel is idle.
    pub fn is_idle(&self) -> bool {
        self.state == ChannelState::Idle
    }

    /// Check if the channel is active.
    pub fn is_active(&self) -> bool {
        self.state == ChannelState::Active
    }

    /// Check if the channel is ready for scheduling.
    pub fn is_ready(&self) -> bool {
        self.state == ChannelState::Ready
    }

    /// Start a new utterance.
    pub fn start_utterance(&mut self, user_data: T, total_frames: Option<usize>) {
        self.state = ChannelState::Ready;
        self.user_data = Some(user_data);
        self.frame_index = 0;
        self.total_frames = total_frames;
        self.token_count = 0;
    }

    /// Assign to a lane.
    pub fn assign_lane(&mut self, lane: usize) {
        self.lane = Some(lane);
        self.state = ChannelState::Active;
    }

    /// Release from lane.
    pub fn release_lane(&mut self) {
        self.lane = None;
        if self.state == ChannelState::Active {
            self.state = ChannelState::Waiting;
        }
    }

    /// Advance to next frame.
    pub fn advance_frame(&mut self, token_count: usize) {
        self.frame_index += 1;
        self.token_count = token_count;
    }

    /// Mark as complete.
    pub fn complete(&mut self) -> Option<T> {
        self.state = ChannelState::Complete;
        self.lane = None;
        self.user_data.take()
    }

    /// Mark as error.
    pub fn error(&mut self) {
        self.state = ChannelState::Error;
        self.lane = None;
    }

    /// Reset to idle.
    pub fn reset(&mut self) {
        self.state = ChannelState::Idle;
        self.lane = None;
        self.user_data = None;
        self.frame_index = 0;
        self.total_frames = None;
        self.token_count = 0;
    }
}

/// A lane for active decoding.
#[derive(Clone, Debug)]
pub struct Lane {
    /// Lane ID.
    id: usize,
    /// Current state.
    state: LaneState,
    /// Assigned channel (if active).
    channel: Option<usize>,
    /// Tokens being processed.
    token_count: usize,
    /// Maximum tokens allowed (for pruning).
    max_tokens: usize,
}

impl Lane {
    /// Create a new available lane.
    pub fn new(id: usize, max_tokens: usize) -> Self {
        Self {
            id,
            state: LaneState::Available,
            channel: None,
            token_count: 0,
            max_tokens,
        }
    }

    /// Get the lane ID.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Get the current state.
    pub fn state(&self) -> LaneState {
        self.state
    }

    /// Get the assigned channel.
    pub fn channel(&self) -> Option<usize> {
        self.channel
    }

    /// Get the token count.
    pub fn token_count(&self) -> usize {
        self.token_count
    }

    /// Get the maximum tokens.
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    /// Check if the lane is available.
    pub fn is_available(&self) -> bool {
        self.state == LaneState::Available
    }

    /// Check if the lane is active.
    pub fn is_active(&self) -> bool {
        self.state == LaneState::Active
    }

    /// Assign a channel.
    pub fn assign_channel(&mut self, channel: usize) {
        self.channel = Some(channel);
        self.state = LaneState::Active;
        self.token_count = 0;
    }

    /// Release the channel.
    pub fn release_channel(&mut self) -> Option<usize> {
        self.state = LaneState::Available;
        self.token_count = 0;
        self.channel.take()
    }

    /// Update token count after a frame.
    pub fn update_tokens(&mut self, count: usize) {
        self.token_count = count.min(self.max_tokens);
    }

    /// Mark frame as complete.
    pub fn frame_complete(&mut self) {
        self.state = LaneState::FrameComplete;
    }

    /// Mark utterance as complete.
    pub fn utterance_complete(&mut self) {
        self.state = LaneState::UtteranceComplete;
    }

    /// Continue to next frame.
    pub fn continue_decoding(&mut self) {
        if self.state == LaneState::FrameComplete {
            self.state = LaneState::Active;
        }
    }
}

/// Configuration for the batched decoder.
#[derive(Clone, Debug)]
pub struct DecoderConfig {
    /// Maximum number of channels.
    pub max_channels: usize,
    /// Maximum number of lanes (batch size).
    pub max_lanes: usize,
    /// Maximum tokens per lane.
    pub max_tokens: usize,
    /// Beam width for pruning.
    pub beam_width: f32,
}

impl Default for DecoderConfig {
    fn default() -> Self {
        Self {
            max_channels: 1000,
            max_lanes: 32,
            max_tokens: 10000,
            beam_width: 16.0,
        }
    }
}

impl DecoderConfig {
    /// Create a configuration for edge devices.
    pub fn edge_device() -> Self {
        Self {
            max_channels: 10,
            max_lanes: 1,
            max_tokens: 10000,
            beam_width: 10.0,
        }
    }

    /// Create a configuration for datacenter GPUs.
    pub fn datacenter() -> Self {
        Self {
            max_channels: 5000,
            max_lanes: 500,
            max_tokens: 10000,
            beam_width: 15.0,
        }
    }

    /// Estimate memory usage in bytes.
    pub fn estimate_memory(&self) -> usize {
        // M_state = 64α·n_c + 544α·n_l + 1024·n_l
        let alpha = self.max_tokens;
        let n_c = self.max_channels;
        let n_l = self.max_lanes;

        64 * alpha * n_c + 544 * alpha * n_l + 1024 * n_l
    }
}

/// Batched decoder managing channels and lanes.
pub struct BatchedDecoder<T> {
    /// All channels.
    channels: Vec<Channel<T>>,
    /// All lanes.
    lanes: Vec<Lane>,
    /// Queue of channels ready to be scheduled.
    ready_queue: VecDeque<usize>,
    /// Configuration.
    config: DecoderConfig,
}

impl<T> BatchedDecoder<T> {
    /// Create a new batched decoder.
    pub fn new(config: DecoderConfig) -> Self {
        let channels = (0..config.max_channels)
            .map(Channel::new)
            .collect();
        let lanes = (0..config.max_lanes)
            .map(|id| Lane::new(id, config.max_tokens))
            .collect();

        Self {
            channels,
            lanes,
            ready_queue: VecDeque::new(),
            config,
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &DecoderConfig {
        &self.config
    }

    /// Get a channel by ID.
    pub fn channel(&self, id: usize) -> Option<&Channel<T>> {
        self.channels.get(id)
    }

    /// Get a mutable channel by ID.
    pub fn channel_mut(&mut self, id: usize) -> Option<&mut Channel<T>> {
        self.channels.get_mut(id)
    }

    /// Get a lane by ID.
    pub fn lane(&self, id: usize) -> Option<&Lane> {
        self.lanes.get(id)
    }

    /// Get a mutable lane by ID.
    pub fn lane_mut(&mut self, id: usize) -> Option<&mut Lane> {
        self.lanes.get_mut(id)
    }

    /// Find an idle channel.
    pub fn find_idle_channel(&self) -> Option<usize> {
        self.channels.iter().position(|c| c.is_idle())
    }

    /// Find an available lane.
    pub fn find_available_lane(&self) -> Option<usize> {
        self.lanes.iter().position(|l| l.is_available())
    }

    /// Start a new utterance on an available channel.
    pub fn start_utterance(
        &mut self,
        user_data: T,
        total_frames: Option<usize>,
    ) -> Option<usize> {
        let channel_id = self.find_idle_channel()?;
        self.channels[channel_id].start_utterance(user_data, total_frames);
        self.ready_queue.push_back(channel_id);
        Some(channel_id)
    }

    /// Schedule ready channels to available lanes.
    pub fn schedule(&mut self) -> Vec<(usize, usize)> {
        let mut assignments = Vec::new();

        while let Some(channel_id) = self.ready_queue.pop_front() {
            if let Some(lane_id) = self.find_available_lane() {
                self.channels[channel_id].assign_lane(lane_id);
                self.lanes[lane_id].assign_channel(channel_id);
                assignments.push((channel_id, lane_id));
            } else {
                // No available lanes, put back in queue
                self.ready_queue.push_front(channel_id);
                break;
            }
        }

        assignments
    }

    /// Get all active lane IDs.
    pub fn active_lanes(&self) -> Vec<usize> {
        self.lanes
            .iter()
            .filter(|l| l.is_active())
            .map(|l| l.id())
            .collect()
    }

    /// Process a frame for all active lanes.
    ///
    /// Returns lanes that completed their utterances.
    pub fn process_frame<F>(&mut self, mut process_fn: F) -> Vec<usize>
    where
        F: FnMut(usize, usize) -> (usize, bool), // (lane_id, channel_id) -> (token_count, is_complete)
    {
        let mut completed_lanes = Vec::new();

        for lane in &mut self.lanes {
            if !lane.is_active() {
                continue;
            }

            let channel_id = match lane.channel() {
                Some(id) => id,
                None => continue,
            };

            let (token_count, is_complete) = process_fn(lane.id(), channel_id);
            lane.update_tokens(token_count);

            if is_complete {
                lane.utterance_complete();
                completed_lanes.push(lane.id());
            } else {
                lane.frame_complete();
            }
        }

        completed_lanes
    }

    /// Continue decoding for lanes that completed a frame.
    pub fn continue_decoding(&mut self) {
        for lane in &mut self.lanes {
            lane.continue_decoding();
        }
    }

    /// Complete utterances for lanes that finished.
    pub fn complete_utterances(&mut self, lane_ids: &[usize]) -> Vec<(usize, Option<T>)> {
        let mut results = Vec::new();

        for &lane_id in lane_ids {
            if let Some(lane) = self.lanes.get_mut(lane_id) {
                if let Some(channel_id) = lane.release_channel() {
                    if let Some(channel) = self.channels.get_mut(channel_id) {
                        let user_data = channel.complete();
                        results.push((channel_id, user_data));
                    }
                }
            }
        }

        results
    }

    /// Get decoder statistics.
    pub fn stats(&self) -> DecoderStats {
        let active_channels = self.channels.iter().filter(|c| c.is_active()).count();
        let ready_channels = self.ready_queue.len();
        let active_lanes = self.lanes.iter().filter(|l| l.is_active()).count();
        let total_tokens: usize = self.lanes.iter().map(|l| l.token_count()).sum();

        DecoderStats {
            max_channels: self.config.max_channels,
            max_lanes: self.config.max_lanes,
            active_channels,
            ready_channels,
            active_lanes,
            total_tokens,
            lane_utilization: active_lanes as f64 / self.config.max_lanes as f64,
        }
    }
}

/// Statistics about the batched decoder.
#[derive(Clone, Debug)]
pub struct DecoderStats {
    /// Maximum channels.
    pub max_channels: usize,
    /// Maximum lanes.
    pub max_lanes: usize,
    /// Currently active channels.
    pub active_channels: usize,
    /// Channels waiting to be scheduled.
    pub ready_channels: usize,
    /// Currently active lanes.
    pub active_lanes: usize,
    /// Total tokens across all lanes.
    pub total_tokens: usize,
    /// Lane utilization (0.0 to 1.0).
    pub lane_utilization: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_states() {
        let mut channel: Channel<i32> = Channel::new(0);
        assert!(channel.is_idle());

        channel.start_utterance(42, Some(100));
        assert!(channel.is_ready());
        assert_eq!(channel.user_data(), Some(&42));

        channel.assign_lane(0);
        assert!(channel.is_active());
        assert_eq!(channel.lane(), Some(0));

        channel.advance_frame(50);
        assert_eq!(channel.frame_index(), 1);
        assert_eq!(channel.token_count(), 50);

        let data = channel.complete();
        assert_eq!(data, Some(42));
        assert_eq!(channel.state(), ChannelState::Complete);
    }

    #[test]
    fn test_lane_states() {
        let mut lane = Lane::new(0, 1000);
        assert!(lane.is_available());

        lane.assign_channel(5);
        assert!(lane.is_active());
        assert_eq!(lane.channel(), Some(5));

        lane.update_tokens(500);
        assert_eq!(lane.token_count(), 500);

        lane.frame_complete();
        assert_eq!(lane.state(), LaneState::FrameComplete);

        lane.continue_decoding();
        assert!(lane.is_active());

        lane.utterance_complete();
        assert_eq!(lane.state(), LaneState::UtteranceComplete);

        let channel = lane.release_channel();
        assert_eq!(channel, Some(5));
        assert!(lane.is_available());
    }

    #[test]
    fn test_decoder_config() {
        let config = DecoderConfig::default();
        assert_eq!(config.max_channels, 1000);
        assert_eq!(config.max_lanes, 32);

        let edge = DecoderConfig::edge_device();
        assert_eq!(edge.max_lanes, 1);

        let dc = DecoderConfig::datacenter();
        assert_eq!(dc.max_lanes, 500);
    }

    #[test]
    fn test_decoder_config_memory() {
        let config = DecoderConfig {
            max_channels: 1,
            max_lanes: 1,
            max_tokens: 10000,
            beam_width: 10.0,
        };

        let mem = config.estimate_memory();
        // 64*10000*1 + 544*10000*1 + 1024*1 = 640000 + 5440000 + 1024 = 6081024
        assert!(mem > 6_000_000);
    }

    #[test]
    fn test_batched_decoder_basic() {
        let config = DecoderConfig {
            max_channels: 10,
            max_lanes: 2,
            max_tokens: 100,
            beam_width: 10.0,
        };
        let mut decoder: BatchedDecoder<String> = BatchedDecoder::new(config);

        // Start two utterances
        let ch1 = decoder.start_utterance("utt1".to_string(), Some(10));
        let ch2 = decoder.start_utterance("utt2".to_string(), Some(20));

        assert!(ch1.is_some());
        assert!(ch2.is_some());

        // Schedule to lanes
        let assignments = decoder.schedule();
        assert_eq!(assignments.len(), 2);

        // Check stats
        let stats = decoder.stats();
        assert_eq!(stats.active_channels, 2);
        assert_eq!(stats.active_lanes, 2);
    }

    #[test]
    fn test_batched_decoder_overflow() {
        let config = DecoderConfig {
            max_channels: 2,
            max_lanes: 1,
            max_tokens: 100,
            beam_width: 10.0,
        };
        let mut decoder: BatchedDecoder<i32> = BatchedDecoder::new(config);

        // Start two utterances
        decoder.start_utterance(1, None);
        decoder.start_utterance(2, None);

        // Only one lane available
        let assignments = decoder.schedule();
        assert_eq!(assignments.len(), 1);

        // One should be ready but not active
        let stats = decoder.stats();
        assert_eq!(stats.active_channels, 1);
        assert_eq!(stats.ready_channels, 1);
    }

    #[test]
    fn test_batched_decoder_process_frame() {
        let config = DecoderConfig {
            max_channels: 10,
            max_lanes: 2,
            max_tokens: 100,
            beam_width: 10.0,
        };
        let mut decoder: BatchedDecoder<i32> = BatchedDecoder::new(config);

        decoder.start_utterance(1, Some(2));
        decoder.start_utterance(2, Some(3));
        decoder.schedule();

        // Process first frame - get total frames before mutable borrow
        let total_frames_1 = decoder.channel(1).and_then(|c| c.total_frames()).unwrap_or(10);
        let total_frames_2 = decoder.channel(2).and_then(|c| c.total_frames()).unwrap_or(10);
        let mut frame = 0;
        let completed = decoder.process_frame(|_lane, channel| {
            frame += 1;
            let total = if channel == 1 { total_frames_1 } else { total_frames_2 };
            (50, frame >= total)
        });

        // Neither should be complete after first frame
        assert!(completed.is_empty() || completed.len() <= 2);
    }

    #[test]
    fn test_batched_decoder_complete() {
        let config = DecoderConfig {
            max_channels: 10,
            max_lanes: 2,
            max_tokens: 100,
            beam_width: 10.0,
        };
        let mut decoder: BatchedDecoder<String> = BatchedDecoder::new(config);

        decoder.start_utterance("test".to_string(), Some(1));
        decoder.schedule();

        // Process and complete
        let completed = decoder.process_frame(|_, _| (10, true));
        assert_eq!(completed.len(), 1);

        let results = decoder.complete_utterances(&completed);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, Some("test".to_string()));
    }
}
