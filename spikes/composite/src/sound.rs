//! Sound: cuelume's seventeen cues (Daniel Belyi, MIT), ported from Web
//! Audio recipes to a small native synth — tone and filtered-noise layers
//! with exponential envelopes, optional glide and a feedback-delay shimmer,
//! one soft output stage — rendered once per cue, cached, and mixed on a
//! cpal stream. Events map to cues (or to quiet); rules.luau's `on_event`
//! can override any of it.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};

// ── Recipes ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wave {
    Sine,
    Triangle,
    Square,
    Saw,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filter {
    Lowpass,
    Bandpass,
}

#[derive(Clone, Debug)]
pub enum Layer {
    Tone { wave: Wave, freq: f32, glide_to: Option<f32>, glide_time: Option<f32>, detune: f32, offset: f32, attack: f32, decay: f32, peak: f32 },
    Noise { filter: Filter, freq: f32, q: f32, offset: f32, attack: f32, decay: f32, peak: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Shimmer {
    pub delay: f32,
    pub feedback: f32,
    pub wet: f32,
    pub lowpass: f32,
}

#[derive(Clone, Debug)]
pub struct Recipe {
    pub master: f32,
    pub layers: Vec<Layer>,
    pub shimmer: Option<Shimmer>,
}

fn tone(wave: Wave, freq: f32, offset: f32, attack: f32, decay: f32, peak: f32) -> Layer {
    Layer::Tone { wave, freq, glide_to: None, glide_time: None, detune: 0.0, offset, attack, decay, peak }
}
fn glide(wave: Wave, freq: f32, to: f32, time: f32, offset: f32, attack: f32, decay: f32, peak: f32) -> Layer {
    Layer::Tone { wave, freq, glide_to: Some(to), glide_time: Some(time), detune: 0.0, offset, attack, decay, peak }
}
fn noise(filter: Filter, freq: f32, q: f32, offset: f32, attack: f32, decay: f32, peak: f32) -> Layer {
    Layer::Noise { filter, freq, q, offset, attack, decay, peak }
}
fn sh(delay: f32, feedback: f32, wet: f32, lowpass: f32) -> Option<Shimmer> {
    Some(Shimmer { delay, feedback, wet, lowpass })
}

pub const NAMES: [&str; 17] = [
    "chime", "sparkle", "droplet", "bloom", "whisper", "tick", "press", "release", "toggle", "success", "error", "page", "loading", "ready", "pulse", "scan", "arrival",
];

/// One line each, in the words of the palette.
pub fn describe(name: &str) -> &'static str {
    match name {
        "chime" => "soft two-note bell",
        "sparkle" => "four-note twinkle",
        "droplet" => "one note gliding down",
        "bloom" => "warm slow swell",
        "whisper" => "hush with a falling tone",
        "tick" => "crisp instant tick",
        "press" => "dull muted knock",
        "release" => "brighter springy tick",
        "toggle" => "mechanical click-clack",
        "success" => "warm three-note confirmation",
        "error" => "knock and descending refusal",
        "page" => "papery flick, glass tick",
        "loading" => "brief rising shimmer",
        "ready" => "lock-on with a clear resolve",
        "pulse" => "compact synthetic chirp",
        "scan" => "three-step locator",
        "arrival" => "rising harmonic portal",
        _ => "",
    }
}

pub fn recipe(name: &str) -> Option<Recipe> {
    use Filter::*;
    use Wave::*;
    Some(match name {
        "chime" => Recipe { master: 0.5, layers: vec![tone(Sine, 1046.5, 0.0, 0.006, 0.22, 0.09), tone(Sine, 1568.0, 0.09, 0.006, 0.26, 0.08)], shimmer: sh(0.12, 0.25, 0.18, 4000.0) },
        "sparkle" => Recipe {
            master: 0.5,
            layers: vec![tone(Sine, 1760.0, 0.0, 0.003, 0.09, 0.045), tone(Sine, 2217.0, 0.045, 0.003, 0.09, 0.04), tone(Sine, 2637.0, 0.09, 0.003, 0.1, 0.038), tone(Sine, 3520.0, 0.135, 0.003, 0.12, 0.032)],
            shimmer: sh(0.07, 0.35, 0.22, 6000.0),
        },
        "droplet" => Recipe { master: 0.55, layers: vec![glide(Sine, 1200.0, 550.0, 0.14, 0.0, 0.004, 0.2, 0.075)], shimmer: sh(0.09, 0.2, 0.15, 3000.0) },
        "bloom" => Recipe {
            master: 0.5,
            layers: vec![tone(Sine, 528.0, 0.0, 0.06, 0.32, 0.06), Layer::Tone { wave: Sine, freq: 528.0, glide_to: None, glide_time: None, detune: 12.0, offset: 0.0, attack: 0.06, decay: 0.34, peak: 0.05 }],
            shimmer: sh(0.15, 0.2, 0.12, 2500.0),
        },
        "whisper" => Recipe { master: 0.48, layers: vec![noise(Lowpass, 1600.0, 0.7, 0.0, 0.025, 0.13, 0.04), glide(Sine, 880.0, 660.0, 0.14, 0.01, 0.012, 0.14, 0.025)], shimmer: None },
        "tick" => Recipe { master: 0.4, layers: vec![noise(Bandpass, 5400.0, 1.8, 0.0, 0.001, 0.018, 0.14), tone(Sine, 2600.0, 0.0, 0.001, 0.012, 0.018)], shimmer: None },
        "press" => Recipe { master: 0.4, layers: vec![noise(Bandpass, 1700.0, 1.4, 0.0, 0.001, 0.02, 0.13)], shimmer: None },
        "release" => Recipe { master: 0.4, layers: vec![noise(Bandpass, 4600.0, 1.8, 0.0, 0.001, 0.016, 0.12), tone(Sine, 3200.0, 0.006, 0.001, 0.05, 0.02)], shimmer: None },
        "toggle" => Recipe { master: 0.4, layers: vec![noise(Bandpass, 2200.0, 1.6, 0.0, 0.001, 0.016, 0.12), noise(Bandpass, 3800.0, 1.6, 0.024, 0.001, 0.02, 0.1)], shimmer: None },
        "success" => Recipe {
            master: 0.5,
            layers: vec![tone(Sine, 880.0, 0.0, 0.004, 0.09, 0.06), tone(Sine, 1108.73, 0.06, 0.004, 0.1, 0.06), tone(Sine, 1318.51, 0.12, 0.004, 0.18, 0.07)],
            shimmer: sh(0.1, 0.22, 0.16, 4500.0),
        },
        "error" => Recipe {
            master: 0.42,
            layers: vec![noise(Bandpass, 850.0, 1.1, 0.0, 0.001, 0.035, 0.13), tone(Triangle, 440.0, 0.025, 0.004, 0.09, 0.045), tone(Triangle, 349.23, 0.1, 0.004, 0.14, 0.04)],
            shimmer: None,
        },
        "page" => Recipe {
            master: 0.38,
            layers: vec![noise(Lowpass, 1800.0, 0.7, 0.0, 0.006, 0.08, 0.11), noise(Bandpass, 4200.0, 1.2, 0.04, 0.004, 0.065, 0.08), tone(Sine, 2400.0, 0.075, 0.002, 0.045, 0.02)],
            shimmer: None,
        },
        "loading" => Recipe { master: 0.42, layers: vec![noise(Lowpass, 1400.0, 0.6, 0.0, 0.035, 0.14, 0.035), glide(Sine, 420.0, 630.0, 0.18, 0.0, 0.025, 0.18, 0.05)], shimmer: sh(0.11, 0.18, 0.12, 2800.0) },
        "ready" => Recipe {
            master: 0.48,
            layers: vec![noise(Bandpass, 3600.0, 1.8, 0.0, 0.001, 0.02, 0.11), glide(Triangle, 330.0, 660.0, 0.12, 0.012, 0.004, 0.16, 0.055), tone(Sine, 990.0, 0.13, 0.004, 0.22, 0.06)],
            shimmer: sh(0.1, 0.16, 0.1, 4200.0),
        },
        "pulse" => Recipe { master: 0.42, layers: vec![noise(Bandpass, 2600.0, 2.4, 0.0, 0.001, 0.022, 0.08), glide(Triangle, 620.0, 1240.0, 0.07, 0.0, 0.002, 0.085, 0.055)], shimmer: None },
        "scan" => Recipe {
            master: 0.4,
            layers: vec![tone(Sine, 740.0, 0.0, 0.002, 0.055, 0.05), tone(Sine, 1110.0, 0.045, 0.002, 0.055, 0.045), tone(Sine, 1665.0, 0.09, 0.002, 0.07, 0.04)],
            shimmer: sh(0.065, 0.16, 0.1, 4200.0),
        },
        "arrival" => Recipe {
            master: 0.44,
            layers: vec![
                noise(Lowpass, 900.0, 0.8, 0.0, 0.05, 0.24, 0.035),
                glide(Sine, 220.0, 440.0, 0.32, 0.0, 0.04, 0.34, 0.055),
                tone(Sine, 659.25, 0.12, 0.045, 0.32, 0.04),
                tone(Sine, 987.77, 0.19, 0.045, 0.34, 0.032),
            ],
            shimmer: sh(0.16, 0.28, 0.18, 3200.0),
        },
        _ => return None,
    })
}

// ── Synthesis ────────────────────────────────────────────────────────────

const OUTPUT_GAIN: f32 = 4.0;
const FLOOR: f32 = 0.0001;

/// RBJ biquad, the same shapes Web Audio's BiquadFilterNode uses.
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn new(filter: Filter, freq: f32, q: f32, rate: f32) -> Biquad {
        let w0 = std::f32::consts::TAU * freq.min(rate * 0.45) / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q.max(0.01));
        let (b0, b1, b2, a0, a1, a2) = match filter {
            Filter::Lowpass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            Filter::Bandpass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        };
        Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Web Audio's exponential ramps: floor → peak over attack, peak → floor over decay.
fn envelope(t: f32, attack: f32, decay: f32, peak: f32) -> f32 {
    if t < 0.0 {
        0.0
    } else if t < attack {
        FLOOR * (peak / FLOOR).powf(t / attack.max(1e-4))
    } else if t < attack + decay {
        peak * (FLOOR / peak).powf((t - attack) / decay.max(1e-4))
    } else {
        0.0
    }
}

fn wave(w: Wave, phase: f32) -> f32 {
    let p = phase.rem_euclid(1.0);
    match w {
        Wave::Sine => (p * std::f32::consts::TAU).sin(),
        Wave::Triangle => 4.0 * (p - 0.5).abs() - 1.0,
        Wave::Square => if p < 0.5 { 1.0 } else { -1.0 },
        Wave::Saw => 2.0 * p - 1.0,
    }
}

/// Render a recipe to mono samples at `rate`.
pub fn render(r: &Recipe, rate: u32) -> Vec<f32> {
    let rate_f = rate as f32;
    let end = r
        .layers
        .iter()
        .map(|l| match l {
            Layer::Tone { offset, attack, decay, .. } | Layer::Noise { offset, attack, decay, .. } => offset + attack + decay + 0.05,
        })
        .fold(0.0f32, f32::max);
    let tail = r.shimmer.map(|s| if s.feedback <= 0.0 { 0.0 } else { s.delay * (1.0 + (FLOOR.ln() / s.feedback.ln()).ceil()) }).unwrap_or(0.0);
    let n = ((end + tail + 0.05) * rate_f) as usize;
    let mut dry = vec![0.0f32; n];
    let mut seed = 0x9e37_79b9u32;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    for layer in &r.layers {
        match *layer {
            Layer::Tone { wave: w, freq, glide_to, glide_time, detune, offset, attack, decay, peak } => {
                let f0 = freq * 2f32.powf(detune / 1200.0);
                let gt = glide_time.unwrap_or(attack + decay).max(1e-4);
                let mut phase = 0.0f32;
                let start = (offset * rate_f) as usize;
                let len = ((attack + decay + 0.05) * rate_f) as usize;
                for i in 0..len {
                    let k = start + i;
                    if k >= n {
                        break;
                    }
                    let t = i as f32 / rate_f;
                    // Exponential glide, as exponentialRampToValueAtTime.
                    let f = match glide_to {
                        Some(to) => f0 * (to / f0).powf((t / gt).min(1.0)),
                        None => f0,
                    };
                    phase += f / rate_f;
                    dry[k] += wave(w, phase) * envelope(t, attack, decay, peak);
                }
            }
            Layer::Noise { filter, freq, q, offset, attack, decay, peak } => {
                let mut bq = Biquad::new(filter, freq, q, rate_f);
                let start = (offset * rate_f) as usize;
                let len = ((attack + decay + 0.05) * rate_f) as usize;
                for i in 0..len {
                    let k = start + i;
                    if k >= n {
                        break;
                    }
                    let t = i as f32 / rate_f;
                    dry[k] += bq.run(rnd()) * envelope(t, attack, decay, peak);
                }
            }
        }
    }
    for v in dry.iter_mut() {
        *v *= r.master;
    }
    // Shimmer: a feedback delay through a lowpass, mixed back in.
    let mut out = dry.clone();
    if let Some(s) = r.shimmer {
        let d = ((s.delay * rate_f) as usize).max(1);
        let mut line = vec![0.0f32; n + d];
        let mut lp = Biquad::new(Filter::Lowpass, s.lowpass, 0.7, rate_f);
        for i in 0..n {
            let delayed = if i >= d { line[i - d] } else { 0.0 };
            let filtered = lp.run(delayed);
            line[i] = dry[i] + filtered * s.feedback;
            out[i] += filtered * s.wet;
        }
    }
    // Output stage: gain, then a soft knee standing in for the compressor.
    for v in out.iter_mut() {
        let x = *v * OUTPUT_GAIN;
        *v = if x.abs() < 0.4 { x } else { x.signum() * (0.4 + (x.abs() - 0.4).tanh() * 0.6) };
    }
    out
}

// ── Playback ─────────────────────────────────────────────────────────────

struct Voice {
    samples: Arc<Vec<f32>>,
    pos: usize,
    gain: f32,
}

#[derive(Default)]
struct Mixer {
    voices: Vec<Voice>,
}

pub struct Player {
    _stream: cpal::Stream,
    mixer: Arc<Mutex<Mixer>>,
    rate: u32,
    channels: usize,
    cache: HashMap<String, Arc<Vec<f32>>>,
}

impl Player {
    /// Open the default output. None when there is no audio device.
    pub fn open() -> Option<Player> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let config = device.default_output_config().ok()?;
        let rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let mixer = Arc::new(Mutex::new(Mixer::default()));
        let m = mixer.clone();
        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| {
                    for frame in data.chunks_mut(channels) {
                        frame.fill(0.0);
                    }
                    let Ok(mut mx) = m.lock() else { return };
                    for frame in data.chunks_mut(channels) {
                        let mut s = 0.0;
                        for v in mx.voices.iter_mut() {
                            if v.pos < v.samples.len() {
                                s += v.samples[v.pos] * v.gain;
                                v.pos += 1;
                            }
                        }
                        for c in frame.iter_mut() {
                            *c = s.clamp(-1.0, 1.0);
                        }
                    }
                    mx.voices.retain(|v| v.pos < v.samples.len());
                },
                |e| tracing::warn!("audio: {e}"),
                None,
            )
            .ok()?;
        stream.play().ok()?;
        Some(Player { _stream: stream, mixer, rate, channels, cache: HashMap::new() })
    }

    pub fn play(&mut self, name: &str, gain: f32) {
        let samples = match self.cache.get(name) {
            Some(s) => s.clone(),
            None => {
                let Some(r) = recipe(name) else { return };
                let s = Arc::new(render(&r, self.rate));
                self.cache.insert(name.to_string(), s.clone());
                s
            }
        };
        if let Ok(mut m) = self.mixer.lock() {
            if m.voices.len() > 8 {
                m.voices.remove(0);
            }
            m.voices.push(Voice { samples, pos: 0, gain });
        }
        let _ = self.channels;
    }
}

// ── Events ───────────────────────────────────────────────────────────────

/// Things the app does that can make a sound. The name is what rules see.
pub const EVENTS: [(&str, &str, &str); 15] = [
    ("launch", "arrival", "with the splash"),
    ("mercury.claim", "arrival", "earning the silver n"),
    ("tab.switch", "page", ""),
    ("tab.close", "droplet", ""),
    ("sidebar.reveal", "bloom", "once per reveal"),
    ("palette.open", "scan", ""),
    ("palette.move", "tick", ""),
    ("control.press", "press", "chips and buttons"),
    ("control.release", "release", ""),
    ("toggle", "toggle", ""),
    ("page.ready", "ready", "only when it took over a second"),
    ("copied", "success", ""),
    ("bell", "error", "inactive tabs only"),
    ("onboarding.tick", "sparkle", ""),
    ("hover", "", "quiet unless you say so · 150ms throttle"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SoundPrefs {
    pub enabled: bool,
    pub volume: f32,
    /// event → cue name; "" means quiet. Missing = the default.
    #[serde(default)]
    pub map: BTreeMap<String, String>,
}

impl Default for SoundPrefs {
    fn default() -> Self {
        SoundPrefs { enabled: true, volume: 0.6, map: BTreeMap::new() }
    }
}

impl SoundPrefs {
    pub fn cue_for(&self, event: &str) -> Option<String> {
        if let Some(c) = self.map.get(event) {
            return if c.is_empty() { None } else { Some(c.clone()) };
        }
        EVENTS.iter().find(|(e, _, _)| *e == event).and_then(|(_, d, _)| if d.is_empty() { None } else { Some(d.to_string()) })
    }
}

/// The app's sound: player, prefs, hover throttle.
pub struct Sound {
    pub player: Option<Player>,
    pub prefs: SoundPrefs,
    last_hover: Instant,
    /// The cue that played last and when, for the settings preview.
    pub last: Option<(String, Instant)>,
}

impl Sound {
    pub fn new(prefs: SoundPrefs) -> Sound {
        Sound { player: Player::open(), prefs, last_hover: crate::clock::now(), last: None }
    }

    /// Play the cue for an event, after the rules have had their say.
    /// `over` is what rules.luau returned: Some(Some(name)) to pick,
    /// Some(None) for quiet, None for the default.
    pub fn event(&mut self, event: &str, over: Option<Option<String>>) {
        if !self.prefs.enabled {
            return;
        }
        if event == "hover" {
            if crate::clock::since(self.last_hover).as_millis() < 150 {
                return;
            }
            self.last_hover = crate::clock::now();
        }
        let cue = match over {
            Some(o) => o,
            None => self.prefs.cue_for(event),
        };
        if let Some(c) = cue {
            self.cue(&c);
        }
    }

    /// Play a cue by name, regardless of events (the Sound page's ▷).
    pub fn cue(&mut self, name: &str) {
        self.last = Some((name.to_string(), crate::clock::now()));
        let v = self.prefs.volume;
        if let Some(p) = self.player.as_mut() {
            p.play(name, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cue_renders_audibly() {
        for n in NAMES {
            let r = recipe(n).unwrap();
            let s = render(&r, 44_100);
            let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.02 && peak <= 1.0, "{n}: peak {peak}");
            assert!(s.len() > 400, "{n} too short");
        }
    }

    #[test]
    fn defaults_map() {
        let p = SoundPrefs::default();
        assert_eq!(p.cue_for("tab.close").as_deref(), Some("droplet"));
        assert_eq!(p.cue_for("hover"), None);
    }
}
