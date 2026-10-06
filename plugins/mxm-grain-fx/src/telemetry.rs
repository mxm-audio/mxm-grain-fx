//! Lock-free, lossy audio-to-editor telemetry. An effect has no developer MIDI channel.
//!
//! Two numbers - the output peak and how many grains were sounding - and a ring of recent grain
//! onsets, which is what lets the Playback card draw marks that actually fired instead of a comb
//! computed from the controls.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use mxm_grain_fx_dsp::Onset;

/// How many onsets the editor can see at once.
///
/// Chosen. The display shows a couple of seconds; at the rate ceiling that is more marks than are
/// distinguishable anyway, and losing the oldest is the right loss for a picture of what the cloud
/// is doing now.
pub const ONSET_SLOTS: usize = 256;

pub struct Telemetry {
    peak: AtomicU32,
    grains: AtomicU32,
    clipped: AtomicBool,
    /// The engine's sample clock at the end of the last published block.
    clock: AtomicU64,
    /// Recent onsets, each packed as `age_fraction` bits in the high word and a truncated sample
    /// clock in the low one. Packed so a slot is written in one store and can never be read half
    /// updated; truncating the clock to 32 bits wraps after a day at 48 kHz, and the reader only
    /// ever takes a difference across a couple of seconds.
    onsets: [AtomicU64; ONSET_SLOTS],
    /// How many onsets have ever been written. The reader takes the last `ONSET_SLOTS` of them.
    onsets_written: AtomicU64,
    /// The host tempo in force, so a synced grain rate or read delay reads its division.
    pub tempo: mxm_tempo::TempoCell,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Telemetry {
    pub fn new() -> Self {
        Self {
            peak: AtomicU32::new(0),
            grains: AtomicU32::new(0),
            clipped: AtomicBool::new(false),
            clock: AtomicU64::new(0),
            onsets: std::array::from_fn(|_| AtomicU64::new(0)),
            onsets_written: AtomicU64::new(0),
            tempo: mxm_tempo::TempoCell::new(),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    fn publish_max(slot: &AtomicU32, value: f32) {
        let value = value.abs();
        let bits = value.to_bits();
        let mut current = slot.load(Ordering::Relaxed);
        while f32::from_bits(current) < value {
            match slot.compare_exchange_weak(current, bits, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(seen) => current = seen,
            }
        }
    }

    pub fn publish(&self, peak: f32, grains: usize) {
        Self::publish_max(&self.peak, peak);
        // The sounding grain count, so the meter can say how busy the cloud is. Max-combined and
        // reset on read like the peak, so a frame the editor missed cannot hide a burst.
        Self::publish_max(&self.grains, grains as f32);
        if peak >= 1.0 {
            self.clipped.store(true, Ordering::Relaxed);
        }
    }

    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    pub fn take_grains(&self) -> f32 {
        f32::from_bits(self.grains.swap(0, Ordering::Relaxed))
    }

    /// Publish the engine's clock and whatever onsets it produced since the last block.
    ///
    /// Realtime-safe: a handful of relaxed stores, no allocation and no lock. Lossy by design - a
    /// reader that misses a frame loses the oldest marks, which is what design system 13 licenses a
    /// display to do.
    pub fn publish_onsets(&self, clock: u64, onsets: &[Onset]) {
        self.clock.store(clock, Ordering::Relaxed);
        if onsets.is_empty() {
            return;
        }
        let mut written = self.onsets_written.load(Ordering::Relaxed);
        for onset in onsets {
            let packed =
                (u64::from(onset.age_fraction.to_bits()) << 32) | u64::from(onset.at as u32);
            self.onsets[(written as usize) % ONSET_SLOTS].store(packed, Ordering::Relaxed);
            written = written.wrapping_add(1);
        }
        // Published last, so a reader that sees the count has already seen the slots.
        self.onsets_written.store(written, Ordering::Release);
    }

    /// The engine's sample clock as of the last published block.
    pub fn clock(&self) -> u64 {
        self.clock.load(Ordering::Relaxed)
    }

    /// Copy the visible onsets as `(truncated clock, age fraction)`, oldest first.
    ///
    /// Reading does not consume: the ring is a window on the recent past, so a display that repaints
    /// twice between blocks draws the same marks twice rather than a hole.
    pub fn onsets(&self, out: &mut [(u32, f32)]) -> usize {
        let written = self.onsets_written.load(Ordering::Acquire);
        let take = (written as usize).min(ONSET_SLOTS).min(out.len());
        let first = written - take as u64;
        for (i, slot) in out[..take].iter_mut().enumerate() {
            let packed =
                self.onsets[((first + i as u64) as usize) % ONSET_SLOTS].load(Ordering::Relaxed);
            *slot = (packed as u32, f32::from_bits((packed >> 32) as u32));
        }
        take
    }

    pub fn clipped(&self) -> bool {
        self.clipped.load(Ordering::Relaxed)
    }

    pub fn clear_clip(&self) {
        self.clipped.store(false, Ordering::Relaxed);
    }
}
