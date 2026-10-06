//! Engine-level contracts: the ones `plans/plan-mxm-grain-fx.md` §8 says must not break.
//!
//! The placement guard's own sweep, the window's morph and the scheduler's statistics are unit
//! tested beside the code they prove. What is here is everything that only exists once the parts are
//! assembled: determinism, partition invariance, boundedness, the engagement edges, freeze, and the
//! three activity states.

use mxm_grain_fx_dsp::{
    Activity, Controls, GrainEngine, MAX_LENGTH_S, ONSET_LOG, Onset, PoolPolicy, Randomisation,
};

/// A setting that puts the engine under real load: dense, scattered, transposed and recycling.
fn busy() -> Controls {
    Controls {
        mix: 0.7,
        length_s: 0.040,
        density_hz: 300.0, // 12 overlapping at 40 ms
        scatter: 0.8,
        position: 0.4,
        pitch_semitones: 7.0,
        reverse: 0.3,
        window_shape: 0.7,
        feedback: 0.5,
        freeze: false,
        randomisation: Randomisation {
            position: 0.5,
            rate: 0.4,
            pan: 0.6,
            length: 0.3,
            level: 0.2,
        },
    }
}

/// A repeatable, broadband stimulus. Deterministic so two runs compare exactly.
fn stimulus(n: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    let mut state = 12_345u32;
    for i in 0..n {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (state >> 9) as f32 / 8_388_608.0 - 1.0;
        let tone = (i as f32 * 0.03).sin();
        l.push(0.5 * tone + 0.2 * noise);
        r.push(0.5 * (i as f32 * 0.031).sin() + 0.2 * noise);
    }
    (l, r)
}

fn render(
    engine: &mut GrainEngine,
    input: &(Vec<f32>, Vec<f32>),
    block: usize,
) -> (Vec<f32>, Vec<f32>) {
    let mut l = input.0.clone();
    let mut r = input.1.clone();
    let n = l.len();
    let mut at = 0;
    while at < n {
        let take = block.min(n - at);
        let _ = engine.process(&mut l[at..at + take], &mut r[at..at + take]);
        at += take;
    }
    (l, r)
}

#[test]
fn silence_in_gives_silence_out() {
    let mut engine = GrainEngine::new(48_000.0);
    engine.set_controls(&busy());
    let silence = (vec![0.0f32; 48_000], vec![0.0f32; 48_000]);
    let (l, r) = render(&mut engine, &silence, 64);
    for (i, (a, b)) in l.iter().zip(r.iter()).enumerate() {
        assert_eq!(*a, 0.0, "left woke up at {i}");
        assert_eq!(*b, 0.0, "right woke up at {i}");
    }
}

#[test]
fn the_render_is_invariant_under_block_partitioning() {
    // §4's third condition, and the reason the generator is advanced per sample rather than per
    // block: with a stochastic scheduler engaged and dense, a per-block draw would make the onset
    // sequence depend on where the host cut the buffer.
    let input = stimulus(24_000);
    let mut controls = busy();
    controls.scatter = 1.0; // fully handed over to the per-sample draw
    controls.density_hz = 500.0; // 20 overlapping at 40 ms

    let mut whole = GrainEngine::new(48_000.0);
    whole.set_controls(&controls);
    let reference = render(&mut whole, &input, 24_000);

    for block in [1usize, 7, 32, 63, 128, 1024] {
        let mut engine = GrainEngine::new(48_000.0);
        engine.set_controls(&controls);
        let got = render(&mut engine, &input, block);
        assert_eq!(got.0, reference.0, "left differs at block size {block}");
        assert_eq!(got.1, reference.1, "right differs at block size {block}");
    }

    // And a partition that changes between calls, which is what a host actually does.
    let mut engine = GrainEngine::new(48_000.0);
    engine.set_controls(&controls);
    let mut l = input.0.clone();
    let mut r = input.1.clone();
    let sizes = [13usize, 1, 200, 64, 7, 511, 3];
    let mut at = 0;
    let mut which = 0;
    while at < l.len() {
        let take = sizes[which % sizes.len()].min(l.len() - at);
        let _ = engine.process(&mut l[at..at + take], &mut r[at..at + take]);
        at += take;
        which += 1;
    }
    assert_eq!(l, reference.0, "left differs under a ragged partition");
    assert_eq!(r, reference.1, "right differs under a ragged partition");
}

#[test]
fn reset_repeats_the_render_exactly() {
    // The generator returns to its seed, so the same input and controls produce the same samples.
    let input = stimulus(12_000);
    let mut engine = GrainEngine::new(48_000.0);
    engine.set_controls(&busy());
    let first = render(&mut engine, &input, 128);
    engine.reset();
    engine.set_controls(&busy());
    let second = render(&mut engine, &input, 128);
    assert_eq!(first.0, second.0);
    assert_eq!(first.1, second.1);
}

#[test]
fn no_old_tail_returns_after_a_reset() {
    let input = stimulus(8_000);
    let mut engine = GrainEngine::new(48_000.0);
    let mut controls = busy();
    controls.feedback = 0.9;
    engine.set_controls(&controls);
    let _ = render(&mut engine, &input, 128);
    engine.reset();
    engine.set_controls(&controls);
    let silence = (vec![0.0f32; 8_000], vec![0.0f32; 8_000]);
    let (l, r) = render(&mut engine, &silence, 128);
    assert!(l.iter().all(|v| *v == 0.0), "a tail survived the reset");
    assert!(r.iter().all(|v| *v == 0.0));
}

#[test]
fn every_rate_the_validator_sweeps_stays_finite() {
    // Times are converted from the active rate and no fixed sample count stands in for one, so the
    // engine has to behave from a few kilohertz to a few hundred.
    for rate in [
        1_000.0f32, 8_000.0, 22_050.0, 44_100.0, 48_000.0, 96_000.0, 192_000.0, 384_000.0,
        768_000.0,
    ] {
        let mut engine = GrainEngine::new(rate);
        // Every control at a corner at once, with every randomisation depth wide open.
        for &(length, density, scatter, pitch, shape) in &[
            (0.002f32, 500.0f32, 1.0f32, 24.0f32, 0.0f32),
            (0.002, 500.0, 0.0, -24.0, 1.0),
            (MAX_LENGTH_S, 0.5, 0.5, 24.0, 0.5),
            (MAX_LENGTH_S, 500.0, 1.0, 0.0, 1.0),
        ] {
            engine.reset();
            engine.set_controls(&Controls {
                mix: 1.0,
                length_s: length,
                density_hz: density,
                scatter,
                position: 0.9,
                pitch_semitones: pitch,
                reverse: 0.5,
                window_shape: shape,
                feedback: 1.0,
                freeze: false,
                randomisation: Randomisation::widest(),
            });
            let n = (rate as usize / 4).clamp(512, 24_000);
            let input = stimulus(n);
            let (l, r) = render(&mut engine, &input, 97);
            for (i, (a, b)) in l.iter().zip(r.iter()).enumerate() {
                assert!(
                    a.is_finite(),
                    "left not finite at {rate} Hz sample {i}: {a}"
                );
                assert!(
                    b.is_finite(),
                    "right not finite at {rate} Hz sample {i}: {b}"
                );
            }
        }
    }
}

#[test]
fn the_gain_budget_holds_at_the_worst_simultaneous_draw() {
    // §6. The budget is evaluated at the density extreme *and* at the worst simultaneous in-domain
    // draw of every randomisation family — not with the depths at zero, because the fold-pan law
    // means a grain panned hard returns both channels' energy on one side and beats the centred
    // case. Drive it with full-scale input and full feedback and require the output to stay bounded.
    //
    // **What this test does and does not prove, since the pan merge.** It used to set a `spread` of
    // 1.0 *and* a pan depth of 1.0, two draws that summed and clamped, so an eighth of all grains
    // hard-panned and the fold was exercised hard. One uniform draw reaches a hard pan at a
    // vanishing rate instead, so this setting can only produce a *lower* peak than it used to and
    // the assertion below is looser than it reads. The worst case is still proved, but it is proved
    // by `grain::tests::the_fold_law_is_the_identity_at_centre_and_sums_at_the_edge`, which calls
    // `Grain::start` at pan 0.0 directly rather than waiting for the generator to draw it. This
    // test's job is the *budget under load*; that one's is the fold.
    let mut engine = GrainEngine::new(48_000.0);
    engine.set_controls(&Controls {
        mix: 1.0,
        length_s: 0.030,
        density_hz: 500.0, // the ceiling; the pool caps the overlap it asks for
        scatter: 0.5,
        position: 0.2,
        pitch_semitones: 0.0,
        reverse: 0.5,
        window_shape: 0.0, // a boxcar returns the most energy per grain
        feedback: 1.0,
        freeze: false,
        randomisation: Randomisation::widest(),
    });
    let n = 48_000 * 4;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    for i in 0..n {
        // Full-scale, and correlated between channels so the fold has the most to sum.
        let v = if (i / 64) % 2 == 0 { 1.0 } else { -1.0 };
        l[i] = v;
        r[i] = v;
    }
    let mut at = 0;
    let mut peak = 0.0f32;
    while at < n {
        let take = 128.min(n - at);
        let _ = engine.process(&mut l[at..at + take], &mut r[at..at + take]);
        for i in at..at + take {
            assert!(l[i].is_finite() && r[i].is_finite(), "not finite at {i}");
            peak = peak.max(l[i].abs()).max(r[i].abs());
        }
        at += take;
    }
    // The saturator is what bounds this, and it is asymptotic to +/-2 per stage. A budget that had
    // run away would be orders above, not a few decibels.
    assert!(peak < 8.0, "the loop was not bounded: peak {peak}");
}

#[test]
fn mix_at_zero_is_dry_to_the_bit_and_then_parks() {
    let input = stimulus(48_000);
    let mut engine = GrainEngine::new(48_000.0);
    let mut controls = busy();
    controls.mix = 0.0;
    engine.set_controls(&controls);
    let (l, r) = render(&mut engine, &input, 128);
    assert_eq!(l, input.0, "left was not dry to the bit");
    assert_eq!(r, input.1, "right was not dry to the bit");
    assert!(engine.is_parked(), "Off must empty and park");
    assert_eq!(engine.active_grains(), 0);
}

#[test]
fn the_engagement_edges_are_click_free_and_start_from_empty() {
    // §7's Off row: reaching zero fades the wet out, empties and parks; re-engaging starts from
    // empty and fades in; a fade interrupted by the other edge restarts rather than resuming. What
    // must not happen is a step in the output, or old material reappearing on re-engagement.
    let mut engine = GrainEngine::new(48_000.0);
    let mut controls = busy();
    controls.feedback = 0.85;
    engine.set_controls(&controls);

    let mut previous = 0.0f32;
    let mut worst_step = 0.0f32;
    let input = stimulus(4_000);

    // Run engaged, then cross zero repeatedly under a live feedback tail.
    for round in 0..12 {
        controls.mix = if round % 2 == 0 { 0.8 } else { 0.0 };
        // Interrupt some fades part way, which is the case that must restart from empty.
        engine.set_controls(&controls);
        let take = if round % 3 == 0 { 200 } else { 4_000 };
        let mut l = input.0[..take].to_vec();
        let mut r = input.1[..take].to_vec();
        let _ = engine.process(&mut l, &mut r);
        for v in l.iter().chain(r.iter()) {
            assert!(v.is_finite());
            worst_step = worst_step.max((v - previous).abs());
            previous = *v;
        }
    }
    // The stimulus itself moves by up to about 0.7 between samples; a click at an engagement edge
    // would be a step far outside that.
    assert!(worst_step < 2.0, "an engagement edge stepped: {worst_step}");
}

#[test]
fn freeze_holds_the_field_while_the_dry_still_passes() {
    // §6. Freeze holds the *wet field*: the record head stops, grains keep reading it. The dry path
    // is untouched — the shimmer ruling the plan pre-empts was against a freeze that disconnected
    // the input, and this one does not.
    let mut engine = GrainEngine::new(48_000.0);
    let mut controls = busy();
    controls.feedback = 0.0;
    controls.mix = 1.0;
    engine.set_controls(&controls);
    let input = stimulus(48_000);
    let _ = render(&mut engine, &input, 128);

    controls.freeze = true;
    engine.set_controls(&controls);
    // Feed a signal that is nothing like what was captured, and check it reaches the output through
    // the dry path even though the buffer is held.
    let n = 8_000;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    let mut frozen_out = Vec::new();
    let mut at = 0;
    while at < n {
        let take = 128.min(n - at);
        let _ = engine.process(&mut l[at..at + take], &mut r[at..at + take]);
        frozen_out.extend_from_slice(&l[at..at + take]);
        at += take;
    }
    // Silence in, but the held field keeps sounding: that is what freeze is for.
    let energy: f32 = frozen_out.iter().map(|v| v * v).sum();
    assert!(energy > 1.0e-3, "the held field went silent: {energy}");

    // With mix at 1 the output is the wet, so a dry-path check needs the crossfade open.
    controls.mix = 0.5;
    engine.set_controls(&controls);
    let mut dl = vec![0.9f32; 2_000];
    let mut dr = vec![0.9f32; 2_000];
    let _ = engine.process(&mut dl, &mut dr);
    let mean: f32 = dl.iter().sum::<f32>() / dl.len() as f32;
    assert!(
        mean > 0.1,
        "the dry path was disconnected by freeze: mean {mean}"
    );
}

#[test]
fn freeze_mutes_the_loop_so_a_held_buffer_cannot_oscillate() {
    // A frozen buffer inside a feedback loop is an unconditional oscillator, so the loop is muted
    // while held. Full feedback, frozen, and left alone for ten seconds.
    let mut engine = GrainEngine::new(48_000.0);
    let mut controls = busy();
    controls.feedback = 1.0;
    controls.mix = 1.0;
    engine.set_controls(&controls);
    let input = stimulus(24_000);
    let _ = render(&mut engine, &input, 128);

    controls.freeze = true;
    engine.set_controls(&controls);
    let mut peak_early = 0.0f32;
    let mut peak_late = 0.0f32;
    for second in 0..10 {
        let mut l = vec![0.0f32; 48_000];
        let mut r = vec![0.0f32; 48_000];
        let _ = engine.process(&mut l, &mut r);
        let peak = l.iter().chain(r.iter()).fold(0.0f32, |a, v| a.max(v.abs()));
        assert!(peak.is_finite(), "not finite in second {second}");
        if second < 2 {
            peak_early = peak_early.max(peak);
        } else {
            peak_late = peak_late.max(peak);
        }
    }
    assert!(
        peak_late <= peak_early * 1.5 + 0.05,
        "a frozen buffer grew: {peak_early} -> {peak_late}"
    );
}

#[test]
fn activity_reports_parked_decaying_and_sustaining() {
    let mut engine = GrainEngine::new(48_000.0);

    // Parked: Off, emptied, costing a dry copy.
    let mut controls = busy();
    controls.mix = 0.0;
    engine.set_controls(&controls);
    let mut l = vec![0.0f32; 4_800];
    let mut r = vec![0.0f32; 4_800];
    let mut state = engine.process(&mut l, &mut r);
    for _ in 0..10 {
        state = engine.process(&mut l, &mut r);
    }
    assert_eq!(state, Activity::Parked, "Off should park");

    // Decaying: a bounded tail, with a conservative declaration.
    controls.mix = 0.7;
    controls.feedback = 0.3;
    engine.set_controls(&controls);
    let input = stimulus(4_800);
    let mut l = input.0.clone();
    let mut r = input.1.clone();
    let state = engine.process(&mut l, &mut r);
    match state {
        Activity::Decaying { tail_seconds } => {
            assert!(
                tail_seconds > MAX_LENGTH_S,
                "the tail must cover a whole grain: {tail_seconds}"
            );
            assert!(tail_seconds.is_finite());
        }
        other => panic!("expected a decaying tail, got {other:?}"),
    }

    // Sustaining: measured on the signal, not read off the controls. The setting is taken from
    // `examples/loop_probe`, which measures which ones actually hold rather than assuming: at full
    // feedback, density 8 and scatter 0.5 never reaches -60 dB and still sits at a tenth of its peak
    // after thirty seconds, while density 0.5 at scatter 1.0 is gone in half a second. The
    // randomisation depths are at zero because they change the normalisation law and therefore the
    // loop gain — which is exactly why the probe exists.
    //
    // Written out rather than derived from `busy()`: transposition and reverse both change what the
    // loop does with its own output, so a sustaining claim has to be measured at the setting it is
    // made for.
    let sustaining_controls = Controls {
        mix: 1.0,
        length_s: 0.040,
        density_hz: 200.0, // 8 overlapping at 40 ms
        scatter: 0.5,
        position: 0.3,
        pitch_semitones: 0.0,
        reverse: 0.0,
        window_shape: 0.7,
        feedback: 1.0,
        freeze: false,
        randomisation: Randomisation::default(),
    };
    let mut engine = GrainEngine::new(48_000.0);
    engine.set_controls(&sustaining_controls);
    let input = stimulus(96_000);
    let _ = render(&mut engine, &input, 512);
    let mut sustaining = false;
    for _ in 0..20 {
        let mut l = vec![0.0f32; 24_000];
        let mut r = vec![0.0f32; 24_000];
        if engine.process(&mut l, &mut r) == Activity::Sustaining {
            sustaining = true;
        }
    }
    assert!(
        sustaining,
        "a non-decaying loop was never reported as sustaining"
    );

    // And the converse, which is what keeps the state honest: a loop that *is* decaying, however
    // slowly, is declared as a tail rather than as a held texture.
    let mut slow = GrainEngine::new(48_000.0);
    let decaying = Controls {
        density_hz: 12.5, // 0.5 overlapping at 40 ms: gaps between grains
        scatter: 1.0,
        ..sustaining_controls
    };
    slow.set_controls(&decaying);
    let input = stimulus(48_000);
    let _ = render(&mut slow, &input, 512);
    let mut seen_sustaining = false;
    for _ in 0..20 {
        let mut l = vec![0.0f32; 24_000];
        let mut r = vec![0.0f32; 24_000];
        if slow.process(&mut l, &mut r) == Activity::Sustaining {
            seen_sustaining = true;
        }
    }
    assert!(
        !seen_sustaining,
        "a decaying loop was advertised as sustaining"
    );
}

/// A stimulus whose content is the same *sound* at any sample rate, so a rate sweep compares like
/// with like rather than comparing three different signals.
fn stimulus_at(rate: f32, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (rate * seconds) as usize;
    let mut l = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    let mut state = 12_345u32;
    for i in 0..n {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (state >> 9) as f32 / 8_388_608.0 - 1.0;
        let t = i as f32 / rate;
        l.push(0.5 * (t * 220.0 * core::f32::consts::TAU).sin() + 0.2 * noise);
        r.push(0.5 * (t * 233.0 * core::f32::consts::TAU).sin() + 0.2 * noise);
    }
    (l, r)
}

/// Drive `controls` at `rate` with two seconds of stimulus, then listen to ten seconds of silence
/// and report whether the engine ever called itself sustaining.
fn reaches_sustaining(rate: f32, controls: &Controls) -> bool {
    let mut engine = GrainEngine::new(rate);
    engine.set_controls(controls);
    let input = stimulus_at(rate, 2.0);
    let _ = render(&mut engine, &input, 512);
    let block = (rate * 0.5) as usize;
    let mut seen = false;
    for _ in 0..20 {
        let mut l = vec![0.0f32; block];
        let mut r = vec![0.0f32; block];
        if engine.process(&mut l, &mut r) == Activity::Sustaining {
            seen = true;
        }
    }
    seen
}

#[test]
fn the_sustain_measurement_does_not_depend_on_the_sample_rate() {
    // Every time in this engine is converted from the active rate, and the wet's level follower was
    // the one exception: a fixed 0.0005 per sample, which is 42 ms at 48 kHz, 250 ms at 8 kHz and
    // 10 ms at 192 kHz. At the slow end the follower's own lag became a large fraction of
    // `SUSTAIN_WINDOW_S`, so the comparison read the lag rather than the field.
    //
    // **The contract is the decision, not the coefficient.** The same loop has to be called the
    // same thing at every rate the validator sweeps, so both directions are asserted at each: a
    // loop that holds is Sustaining, and one that decays is not.
    let held = Controls {
        mix: 1.0,
        length_s: 0.040,
        density_hz: 200.0, // 8 overlapping at 40 ms — `loop_probe` measures this one as holding
        scatter: 0.5,
        position: 0.3,
        pitch_semitones: 0.0,
        reverse: 0.0,
        window_shape: 0.7,
        feedback: 1.0,
        freeze: false,
        randomisation: Randomisation::default(),
    };
    let decaying = Controls {
        density_hz: 12.5, // 0.5 overlapping: gaps between grains, and the loop eats itself
        scatter: 1.0,
        ..held
    };
    for &rate in &[8_000.0f32, 48_000.0, 192_000.0] {
        assert!(
            reaches_sustaining(rate, &held),
            "a held texture was never reported as sustaining at {rate} Hz"
        );
        assert!(
            !reaches_sustaining(rate, &decaying),
            "a decaying loop was advertised as sustaining at {rate} Hz"
        );
    }
}

#[test]
fn the_tail_declaration_moves_with_the_controls_that_change_the_loop() {
    // §7 states the tail as a rule rather than a list: every parameter that can move the latest
    // audible read or the recycled gain, at its worst in-domain draw. Feedback is the one with the
    // largest reach, and the declaration has to follow it rather than latch.
    let mut engine = GrainEngine::new(48_000.0);
    let input = stimulus(2_400);

    let tail_at = |engine: &mut GrainEngine, feedback: f32| -> f32 {
        let mut controls = busy();
        controls.feedback = feedback;
        engine.set_controls(&controls);
        let mut l = input.0.clone();
        let mut r = input.1.clone();
        match engine.process(&mut l, &mut r) {
            Activity::Decaying { tail_seconds } => tail_seconds,
            Activity::Sustaining => f32::INFINITY,
            Activity::Parked => 0.0,
        }
    };

    let low = tail_at(&mut engine, 0.1);
    let high = tail_at(&mut engine, 0.9);
    assert!(
        high > low,
        "more feedback must declare a longer tail: {low} -> {high}"
    );
    let back = tail_at(&mut engine, 0.1);
    assert!(back < high, "the declaration latched: {high} -> {back}");
}

#[test]
fn both_pool_policies_survive_an_exhausted_pool() {
    // Decision 8 is open and both alternatives are crate inputs, so the listening gate auditions
    // real behaviour. Neither may produce silence, a non-finite sample, or a stuck pool.
    for policy in [PoolPolicy::StealOldest, PoolPolicy::Drop] {
        let mut engine = GrainEngine::new(48_000.0);
        engine.set_pool_policy(policy);
        engine.set_controls(&Controls {
            mix: 1.0,
            length_s: MAX_LENGTH_S,
            density_hz: 500.0,
            scatter: 1.0,
            feedback: 0.2,
            ..busy()
        });
        let input = stimulus(48_000);
        let (l, r) = render(&mut engine, &input, 128);
        let energy: f32 = l.iter().chain(r.iter()).map(|v| v * v).sum();
        assert!(
            energy.is_finite() && energy > 1.0e-3,
            "{policy:?} produced nothing: {energy}"
        );
        assert!(
            l.iter().all(|v| v.is_finite()),
            "{policy:?} went non-finite"
        );
    }
}

#[test]
fn a_non_finite_input_cannot_poison_the_engine() {
    let mut engine = GrainEngine::new(48_000.0);
    engine.set_controls(&busy());
    let mut l = vec![f32::NAN, f32::INFINITY, -f32::INFINITY, 1.0e-42, 0.5];
    let mut r = l.clone();
    let _ = engine.process(&mut l, &mut r);
    let input = stimulus(4_800);
    let (l, r) = render(&mut engine, &input, 128);
    assert!(l.iter().all(|v| v.is_finite()), "left never recovered");
    assert!(r.iter().all(|v| v.is_finite()), "right never recovered");
}

#[test]
fn the_length_control_reaches_all_three_regions() {
    // §5: the domain must reach the sub-period region, the fusion region and the events region.
    // Reaching them is proved by the grain rate the length implies at a fixed density, which is the
    // quantity the psychoacoustic thresholds are stated against.
    let sample_rate = 48_000.0f32;
    let shortest = mxm_grain_fx_dsp::MIN_LENGTH_S;
    let longest = MAX_LENGTH_S;
    assert!(
        shortest * 1_000.0 < 13.0,
        "the short end must be sub-period: {shortest}s"
    );
    assert!(
        shortest * 1_000.0 <= 10.0,
        "and reach Roads' 10-20 ms grains: {shortest}s"
    );
    assert!(
        longest * 1_000.0 > 100.0,
        "the long end must be well past fusion: {longest}s"
    );

    // And the engine actually renders at both ends rather than refusing.
    for length_s in [shortest, 0.030, longest] {
        let mut engine = GrainEngine::new(sample_rate);
        engine.set_controls(&Controls {
            mix: 1.0,
            length_s,
            density_hz: 80.0,
            scatter: 0.0,
            feedback: 0.0,
            randomisation: Randomisation::default(),
            ..busy()
        });
        let input = stimulus(24_000);
        let (l, _) = render(&mut engine, &input, 128);
        let energy: f32 = l.iter().map(|v| v * v).sum();
        assert!(energy > 1.0e-3, "no output at length {length_s}s: {energy}");
    }
}

#[test]
fn a_longer_grain_at_the_same_density_overlaps_more() {
    // **The law the owner's ear corrected.** Density is a *rate*, so length and density are
    // independent and overlap is what falls out of them. Under the old law density *was* the
    // overlap, which made lengthening a grain do nothing but slow the onset rate — a longer grain
    // gave fewer grains rather than more overlap, and the smooth thickening a granular processor is
    // reached for could not be produced at all. Reported as "it just makes fewer grains".
    let input = stimulus(48_000 * 6);
    let mut counts = Vec::new();
    for length_s in [0.010f32, 0.020, 0.040, 0.080, 0.160] {
        let mut engine = GrainEngine::new(48_000.0);
        engine.set_controls(&Controls {
            mix: 1.0,
            length_s,
            density_hz: 100.0,
            scatter: 0.0,
            feedback: 0.0,
            randomisation: Randomisation::default(),
            ..busy()
        });
        let mut peak = 0usize;
        let mut l = input.0.clone();
        let mut r = input.1.clone();
        let mut at = 0;
        while at < l.len() {
            let take = 256.min(l.len() - at);
            let _ = engine.process(&mut l[at..at + take], &mut r[at..at + take]);
            peak = peak.max(engine.active_grains());
            at += take;
        }
        counts.push((length_s, peak));
    }
    for pair in counts.windows(2) {
        let (short, few) = pair[0];
        let (long, many) = pair[1];
        assert!(
            many > few,
            "doubling {short}s to {long}s did not raise the overlap: {few} -> {many}"
        );
    }
    // And roughly proportionally: sixteen times the length, near sixteen times the overlap.
    let (_, fewest) = counts[0];
    let (_, most) = counts[counts.len() - 1];
    assert!(
        most >= fewest * 8,
        "the overlap should track the length: {fewest} -> {most}"
    );
}

#[test]
fn overlapping_grains_decorrelate_when_the_spray_is_open() {
    // The second half of the same report. With every grain reading the *same* age at unity rate,
    // overlapping grains read identical material and the overlap-add reconstructs the input: the
    // effect degenerates to a plain delay however many grains are sounding. Measured as a purity of
    // 0.997 against a 440 Hz tone. A position spray is what makes them read different material, and
    // it is why the shipped default has one.
    let n = 48_000 * 6;
    let tone: Vec<f32> = (0..n)
        .map(|i| (i as f32 * 440.0 * core::f32::consts::TAU / 48_000.0).sin() * 0.5)
        .collect();

    let purity = |spray: f32| -> f32 {
        let mut engine = GrainEngine::new(48_000.0);
        engine.set_controls(&Controls {
            mix: 1.0,
            length_s: 0.060,
            density_hz: 67.0,
            scatter: 0.25,
            position: 0.15,
            pitch_semitones: 0.0,
            reverse: 0.0,
            window_shape: 0.75,
            feedback: 0.0,
            freeze: false,
            randomisation: Randomisation {
                position: spray,
                ..Randomisation::default()
            },
        });
        let mut l = tone.clone();
        let mut r = tone.clone();
        let mut at = 0;
        while at < n {
            let take = 256.min(n - at);
            let _ = engine.process(&mut l[at..at + take], &mut r[at..at + take]);
            at += take;
        }
        let tail = &l[n - 48_000..];
        let rms = (tail
            .iter()
            .map(|v| f64::from(*v) * f64::from(*v))
            .sum::<f64>()
            / tail.len() as f64)
            .sqrt() as f32;
        let w = core::f64::consts::TAU * 440.0 / 48_000.0;
        let (c, s) = (w.cos(), w.sin());
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &v in tail {
            let s0 = f64::from(v) + 2.0 * c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let (re, im) = (s1 - s2 * c, s2 * s);
        let carrier = ((re * re + im * im).sqrt() / (tail.len() as f64 / 2.0)) as f32;
        carrier / rms.max(1.0e-9) / core::f32::consts::SQRT_2
    };

    let shut = purity(0.0);
    let open = purity(0.3);
    assert!(
        shut > 0.9,
        "with no spray this should be a clean delay: {shut}"
    );
    assert!(
        open < 0.75,
        "an open spray must decorrelate the grains: {open} against {shut}"
    );
}

/// Render `seconds` at `controls` and return the onsets the engine recorded.
fn recorded_onsets(controls: &Controls, seconds: f32) -> Vec<Onset> {
    let rate = 48_000.0f32;
    let mut engine = GrainEngine::new(rate);
    engine.set_controls(controls);
    let input = stimulus_at(rate, seconds);
    let _ = render(&mut engine, &input, 256);
    let mut out = vec![
        Onset {
            at: 0,
            age_fraction: 0.0
        };
        ONSET_LOG
    ];
    let n = engine.onsets(&mut out);
    out.truncate(n);
    out
}

#[test]
fn the_onset_log_records_grains_that_actually_sounded() {
    // The display's data source. Three claims, because a display drawing the wrong one of them is
    // the failure this log exists to prevent.
    let base = Controls {
        mix: 1.0,
        length_s: 0.040,
        density_hz: 60.0,
        scatter: 0.0,
        position: 0.5,
        feedback: 0.0,
        ..Controls::default()
    };

    // 1. Something is recorded, its clock is monotone, and every age is inside the buffer.
    let onsets = recorded_onsets(&base, 2.0);
    assert!(onsets.len() > 20, "only {} onsets recorded", onsets.len());
    for pair in onsets.windows(2) {
        assert!(
            pair[0].at <= pair[1].at,
            "the log is out of order: {pair:?}"
        );
    }
    for onset in &onsets {
        assert!(
            (0.0..=1.0).contains(&onset.age_fraction),
            "age outside the buffer: {onset:?}"
        );
    }

    // 2. **A regular clock reads as a regular clock.** Every interval identical, which is what makes
    //    the periodic end of the control visible as an even comb.
    let intervals: Vec<u64> = onsets.windows(2).map(|p| p[1].at - p[0].at).collect();
    let first = intervals[0];
    assert!(
        intervals.iter().all(|i| *i == first),
        "a periodic scheduler recorded uneven onsets: {:?}",
        &intervals[..8.min(intervals.len())]
    );

    // 3. **And a handed-over one does not.** This is the whole point of the display: the two ends of
    //    the control have to be distinguishable from the marks alone.
    let scattered = recorded_onsets(
        &Controls {
            scatter: 1.0,
            ..base
        },
        2.0,
    );
    let scattered_intervals: Vec<u64> = scattered.windows(2).map(|p| p[1].at - p[0].at).collect();
    let distinct = scattered_intervals
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    assert!(
        distinct > scattered_intervals.len() / 4,
        "a stochastic scheduler recorded {distinct} distinct intervals out of {}",
        scattered_intervals.len()
    );
}

#[test]
fn the_position_spray_spreads_the_recorded_ages_and_a_shut_one_does_not() {
    // The display's other axis. With the depth shut every grain reads the same age, so the marks sit
    // on one line; opening it spreads them, and that separation on screen is the difference between
    // *when a grain starts* and *where it reads*.
    let base = Controls {
        mix: 1.0,
        length_s: 0.040,
        density_hz: 60.0,
        scatter: 0.0,
        position: 0.5,
        feedback: 0.0,
        ..Controls::default()
    };
    // **Only onsets after the buffer has filled.** For the first `BUFFER_S` the placement guard
    // clamps the read to the oldest material there is, so the age climbs from nothing to whatever
    // the control asked for — the wart the DSP's DOX records as *the first four seconds repeat
    // rather than delay*. Measured across the fill, a shut spray looks like a spray of 0.49. The
    // display will show that same climb after an insert, and it is right to: it is what the engine
    // is doing.
    let settled = |onsets: Vec<Onset>| -> f32 {
        let after = (mxm_grain_fx_dsp::BUFFER_S * 48_000.0) as u64;
        let ages: Vec<f32> = onsets
            .iter()
            .filter(|o| o.at > after)
            .map(|o| o.age_fraction)
            .collect();
        assert!(ages.len() > 20, "only {} settled onsets", ages.len());
        let lo = ages.iter().fold(f32::MAX, |a, v| a.min(*v));
        let hi = ages.iter().fold(f32::MIN, |a, v| a.max(*v));
        hi - lo
    };
    let shut = settled(recorded_onsets(&base, 6.0));
    let open = settled(recorded_onsets(
        &Controls {
            randomisation: Randomisation {
                position: 0.6,
                ..Randomisation::default()
            },
            ..base
        },
        6.0,
    ));
    assert!(shut < 0.01, "a shut spray still spread the ages by {shut}");
    assert!(open > 0.2, "an open spray only spread the ages by {open}");
}

#[test]
fn the_onset_log_survives_more_onsets_than_it_holds() {
    // It is a window, not a queue: at the density ceiling it drops the oldest rather than the
    // newest, and reading it does not empty it.
    let onsets = recorded_onsets(
        &Controls {
            mix: 1.0,
            length_s: 0.010,
            density_hz: 500.0,
            position: 0.5,
            ..Controls::default()
        },
        4.0,
    );
    assert_eq!(onsets.len(), ONSET_LOG, "the ring did not fill");
    for pair in onsets.windows(2) {
        assert!(
            pair[0].at <= pair[1].at,
            "the wrap lost the order: {pair:?}"
        );
    }
}

#[test]
fn onsets_since_returns_only_what_is_new_and_keeps_the_order() {
    // The telemetry path: a block asks for what it has not seen. Asking twice with the same clock
    // must give the same answer, and asking with the newest clock must give nothing.
    let mut engine = GrainEngine::new(48_000.0);
    engine.set_controls(&Controls {
        mix: 1.0,
        length_s: 0.040,
        density_hz: 60.0,
        scatter: 0.0,
        position: 0.5,
        ..Controls::default()
    });
    let input = stimulus_at(48_000.0, 1.0);
    let _ = render(&mut engine, &input, 256);

    let mut all = vec![
        Onset {
            at: 0,
            age_fraction: 0.0
        };
        ONSET_LOG
    ];
    let total = engine.onsets(&mut all);
    assert!(total > 10, "only {total} onsets");

    // Everything is newer than zero.
    let mut fresh = vec![
        Onset {
            at: 0,
            age_fraction: 0.0
        };
        ONSET_LOG
    ];
    assert_eq!(engine.onsets_since(0, &mut fresh), total);
    assert_eq!(&fresh[..total], &all[..total]);

    // Nothing is newer than the newest.
    let newest = all[total - 1].at;
    assert_eq!(engine.onsets_since(newest, &mut fresh), 0);

    // A cut in the middle returns the tail, in order, and the two halves rejoin.
    let cut = all[total / 2].at;
    let tail = engine.onsets_since(cut, &mut fresh);
    assert_eq!(tail, total - total / 2 - 1, "wrong number after the cut");
    assert_eq!(&fresh[..tail], &all[total - tail..total]);
}
