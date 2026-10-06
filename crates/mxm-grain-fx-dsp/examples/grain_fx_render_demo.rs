//! Renders the G3 audition set to WAV files, so the listening gate has something to listen to.
//!
//! `plans/plan-mxm-grain-fx.md` G3 is judged by ear by the owner and sits **before** the plugin
//! shell, so there is no plugin to load it into yet by design. This writes the same engine's output
//! to disk instead.
//!
//! ```text
//! cargo run -p mxm-grain-fx-dsp --release --example grain_fx_render_demo
//! ```
//!
//! Files land in `target/grain-fx-demo/`, which is build output and is never committed — the root
//! `AGENTS.md`'s pre-public checklist keeps audio out of the tree (the monorepo root's *Before this
//! repository is made public*; since the split in mxm-kit's `docs/collection-rules.md`).

use mxm_grain_fx_dsp::{Controls, GrainEngine, Randomisation};

const RATE: f32 = 48_000.0;

/// A short phrase, so the granulation is heard on material with pitch and transients rather than on
/// noise. A plain saw through an envelope: this is a test signal, not an instrument.
fn source(seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (RATE * seconds) as usize;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    // A minor-ish arpeggio, one note every 400 ms.
    let notes = [
        220.0f32, 261.63, 329.63, 440.0, 329.63, 261.63, 220.0, 174.61,
    ];
    let note_len = (RATE * 0.4) as usize;
    for (index, freq) in notes.iter().cycle().take(n / note_len + 1).enumerate() {
        let start = index * note_len;
        let mut phase = 0.0f32;
        for i in 0..note_len {
            let at = start + i;
            if at >= n {
                break;
            }
            let t = i as f32 / RATE;
            // Saw with a plucked envelope.
            phase += freq / RATE;
            phase -= phase.floor();
            let saw = phase * 2.0 - 1.0;
            let env = (-t * 6.0).exp() * (1.0 - (-t * 400.0).exp());
            let v = saw * env * 0.35;
            l[at] = v;
            r[at] = v;
        }
    }
    (l, r)
}

/// Writes the demo through `mxm-measure`'s encoder.
///
/// **The ninth hand-written WAV writer**, and the last outside shipped player code. It interleaved
/// by hand and truncated with `as i16`; the encoder rounds, so every sample moves by at most one LSB
/// and never toward zero — the same correction the six shared writers took.
fn write_wav(path: &str, l: &[f32], r: &[f32]) -> std::io::Result<()> {
    let frames = l.len().min(r.len());
    let interleaved: Vec<f32> = (0..frames).flat_map(|i| [l[i], r[i]]).collect();
    mxm_audio_file::write(
        path,
        &interleaved,
        2,
        RATE as u32,
        mxm_audio_file::Target::Wav(mxm_audio_file::Bits::Sixteen),
    )
    .map(|_| ())
    .map_err(std::io::Error::other)
}

struct Demo {
    name: &'static str,
    what: &'static str,
    controls: Controls,
    /// Freeze from this second to the end, if any.
    freeze_from: Option<f32>,
}

fn base() -> Controls {
    Controls {
        mix: 0.7,
        length_s: 0.060,
        density_hz: 67.0,
        scatter: 0.2,
        position: 0.15,
        pitch_semitones: 0.0,
        reverse: 0.0,
        window_shape: 0.75,
        feedback: 0.0,
        freeze: false,
        randomisation: Randomisation::default(),
    }
}

fn main() -> std::io::Result<()> {
    let dir = "target/grain-fx-demo";
    std::fs::create_dir_all(dir)?;

    let demos = vec![
        Demo {
            name: "00-source",
            what: "the dry phrase, for reference",
            controls: Controls { mix: 0.0, ..base() },
            freeze_from: None,
        },
        Demo {
            name: "01-fusion-wash",
            what: "60 ms grains, dense and lightly scattered — the middle of the range",
            controls: base(),
            freeze_from: None,
        },
        Demo {
            name: "02-sub-period-buzz",
            what: "3 ms grains: shorter than a period, so the result is broadband buzz",
            controls: Controls {
                length_s: 0.003,
                density_hz: 160.0,
                mix: 0.85,
                ..base()
            },
            freeze_from: None,
        },
        Demo {
            name: "03-periodic-AM",
            what: "grains back to back with no scatter — the amplitude modulation Truax describes",
            controls: Controls {
                length_s: 0.030,
                density_hz: 20.0,
                scatter: 0.0,
                window_shape: 0.5,
                mix: 1.0,
                ..base()
            },
            freeze_from: None,
        },
        Demo {
            name: "04-boxcar-glitch",
            what: "the same, with a near-rectangular window: the clicks are the setting",
            controls: Controls {
                length_s: 0.030,
                density_hz: 20.0,
                scatter: 0.0,
                window_shape: 0.0,
                mix: 1.0,
                ..base()
            },
            freeze_from: None,
        },
        Demo {
            name: "05-cloud",
            what: "full scatter with a position spray — the asynchronous branch, decorrelated",
            controls: Controls {
                density_hz: 240.0,
                scatter: 1.0,
                mix: 0.9,
                randomisation: Randomisation {
                    position: 0.6,
                    rate: 0.1,
                    pan: 0.8,
                    ..Randomisation::default()
                },
                ..base()
            },
            freeze_from: None,
        },
        Demo {
            name: "06-octave-up-recycled",
            what: "+12 semitones with feedback, so each pass climbs again",
            controls: Controls {
                pitch_semitones: 12.0,
                feedback: 0.65,
                density_hz: 120.0,
                scatter: 0.4,
                mix: 0.8,
                ..base()
            },
            freeze_from: None,
        },
        Demo {
            name: "07-reverse",
            what: "half the grains read backwards",
            controls: Controls {
                reverse: 0.6,
                length_s: 0.120,
                density_hz: 60.0,
                mix: 0.85,
                ..base()
            },
            freeze_from: None,
        },
        Demo {
            name: "08-scatter-delay",
            what: "long grains with gaps — the events region, a scatter delay rather than a texture",
            controls: Controls {
                length_s: 0.400,
                density_hz: 12.0,
                scatter: 0.7,
                position: 0.5,
                mix: 0.8,
                randomisation: Randomisation {
                    position: 0.5,
                    ..Randomisation::default()
                },
                ..base()
            },
            freeze_from: None,
        },
        Demo {
            name: "09-collapse",
            what: "gaps plus full feedback: the material subdivides itself to dust — the wart",
            controls: Controls {
                length_s: 0.080,
                density_hz: 10.0,
                scatter: 1.0,
                feedback: 1.0,
                mix: 1.0,
                ..base()
            },
            freeze_from: None,
        },
        Demo {
            name: "10-freeze",
            what: "frozen after 3 s: the buffer holds, the dry keeps passing",
            controls: Controls {
                density_hz: 160.0,
                scatter: 0.5,
                mix: 0.85,
                ..base()
            },
            freeze_from: Some(3.0),
        },
    ];

    println!("mxm-grain-fx — G3 audition set, {RATE} Hz stereo\n");
    for demo in &demos {
        let seconds = 8.0f32;
        let (mut l, mut r) = source(seconds);
        let mut engine = GrainEngine::new(RATE);
        engine.set_controls(&demo.controls);

        let block = 128usize;
        let freeze_at = demo
            .freeze_from
            .map(|s| (s * RATE) as usize)
            .unwrap_or(usize::MAX);
        let mut at = 0;
        let mut frozen = false;
        while at < l.len() {
            if !frozen && at >= freeze_at {
                let mut c = demo.controls;
                c.freeze = true;
                engine.set_controls(&c);
                frozen = true;
            }
            let take = block.min(l.len() - at);
            let _ = engine.process(&mut l[at..at + take], &mut r[at..at + take]);
            at += take;
        }

        let peak = l.iter().chain(r.iter()).fold(0.0f32, |a, v| a.max(v.abs()));
        let path = format!("{dir}/{}.wav", demo.name);
        write_wav(&path, &l, &r)?;
        println!(
            "  {:<22} peak {:>6.2} dBFS   {}",
            demo.name,
            20.0 * peak.max(1e-9).log10(),
            demo.what
        );
    }
    println!("\nWritten to {dir}/ — build output, never committed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This demo's own writer, the ninth of nine the census eventually found.
    ///
    /// The shared encoder is proved in `mxm-measure` against fixed header and payload bytes. What
    /// that cannot see is *this* file later writing the wrong channel count or rate, so the
    /// expectations here are literals — the facts about this instrument.
    #[test]
    fn the_demo_writer_produces_a_playable_file() {
        let frames = 256;
        let interleaved: Vec<f32> = (0..frames * 2)
            .map(|i| {
                let t = i as f32 / 48_000 as f32;
                0.8 * (std::f32::consts::TAU * 220.0 * t).sin()
            })
            .collect();

        let mut path = std::env::temp_dir();
        path.push(format!("grain-fx-render-writer-{}.wav", std::process::id()));
        let (l, r): (Vec<f32>, Vec<f32>) =
            interleaved.chunks_exact(2).map(|c| (c[0], c[1])).unzip();
        write_wav(path.to_str().expect("a utf-8 path"), &l, &r).expect("the demo is written");

        let read = mxm_audio_file_decode::decode_file(
            &path,
            &mxm_audio_file_decode::Limits::new(
                usize::MAX,
                mxm_audio_file_decode::AtLimit::Refuse,
                mxm_audio_file_decode::Keep::AllUpTo(2),
            ),
        )
        .expect("the demo file parses");
        assert_eq!(read.channels, 2, "the demo wrote the wrong channel count");
        assert_eq!(
            read.sample_rate, 48_000,
            "the demo wrote the wrong sample rate"
        );
        assert_eq!(read.frames(), frames, "the demo dropped or invented frames");

        // This writer applies no headroom law — it hands the samples to the encoder, which clamps —
        // so a 0.8 source arrives at 0.8 rather than scaled.
        let peak = mxm_measure::level::peak(&read.interleaved).expect("a finite file");
        assert!(
            (peak - 0.8).abs() < 0.01,
            "the writer changed the level: peak {peak}"
        );

        std::fs::remove_file(&path).ok();
    }
}
