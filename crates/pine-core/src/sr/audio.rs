//! The audio engine: decodes a song held in memory and plays it on the
//! default output device.
//!
//! One thread owns everything (the output stream is not `Send` on every
//! platform). It decodes ahead into a short buffer, about a third of a
//! second, that the device's callback drains, so a seek or a skip is heard at
//! once and memory stays small. Decoding is Symphonia (pure Rust, MP4 and
//! AAC only); output is cpal. When the device goes away (headphones
//! unplugged, the default device changed), the stream is opened again on
//! whatever device is default now and the song carries on from where it was.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};
use tokio::sync::mpsc::UnboundedSender;

/// Seconds of audio decoded ahead of the speakers.
const AHEAD_S: f64 = 0.35;
const POSITION_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub enum Command {
    /// Load a song (MP4/AAC bytes) at a position, playing or paused.
    Load { bytes: Vec<u8>, start_ms: u64, play: bool },
    Play,
    Pause,
    Seek(u64),
    /// 0 to 100.
    Volume(u8),
    /// Unload the song.
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Loaded { duration_ms: u64 },
    /// Where playback is, a few times a second while a song is loaded.
    Position { ms: u64, playing: bool },
    /// The song played to its end.
    Ended,
    Failed(String),
}

/// A handle to the engine thread. Dropping it stops the thread.
pub struct AudioEngine {
    tx: mpsc::Sender<Command>,
}

impl AudioEngine {
    pub fn start(events: UnboundedSender<Event>) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("sylva-audio".into())
            .spawn(move || Engine::new(events).run(rx))
            .expect("could not start the audio thread");
        AudioEngine { tx }
    }

    pub fn send(&self, c: Command) {
        let _ = self.tx.send(c);
    }
}

/// What the device callback shares with the engine thread.
struct Shared {
    buf: Mutex<VecDeque<f32>>,
    /// f32 bits of the gain.
    gain: AtomicU32,
    paused: AtomicBool,
    /// Frames that reached the device since the last reset.
    played: AtomicU64,
    /// The stream reported an error, such as the device going away.
    broken: AtomicBool,
}

struct Output {
    _stream: cpal::Stream,
    rate: u32,
    channels: usize,
}

struct Source {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track: u32,
    time_base: Option<TimeBase>,
    rate: u32,
    duration_ms: u64,
    eof: bool,
}

struct Engine {
    events: UnboundedSender<Event>,
    shared: Arc<Shared>,
    out: Option<Output>,
    src: Option<Source>,
    resampler: Resampler,
    /// Position of the first frame the device was given since the last reset.
    base_ms: u64,
    playing: bool,
    ended: bool,
    device_failed_at: Option<Instant>,
}

impl Engine {
    fn new(events: UnboundedSender<Event>) -> Self {
        Engine {
            events,
            shared: Arc::new(Shared {
                buf: Mutex::new(VecDeque::new()),
                gain: AtomicU32::new(gain(60).to_bits()),
                paused: AtomicBool::new(true),
                played: AtomicU64::new(0),
                broken: AtomicBool::new(false),
            }),
            out: None,
            src: None,
            resampler: Resampler::default(),
            base_ms: 0,
            playing: false,
            ended: false,
            device_failed_at: None,
        }
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    fn run(mut self, rx: mpsc::Receiver<Command>) {
        let mut last_pos = Instant::now();
        loop {
            let wait = if self.wants_data() { Duration::ZERO } else { Duration::from_millis(40) };
            match rx.recv_timeout(wait) {
                Ok(c) => {
                    self.handle(c);
                    while let Ok(c) = rx.try_recv() {
                        self.handle(c);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.keep_output();
            while self.wants_data() {
                if !self.decode_one() {
                    break;
                }
            }
            if self.src.as_ref().is_some_and(|s| s.eof)
                && self.playing
                && !self.ended
                && self.shared.buf.lock().map(|b| b.is_empty()).unwrap_or(true)
            {
                self.ended = true;
                self.set_playing(false);
                self.emit(Event::Ended);
            }
            if self.src.is_some() && last_pos.elapsed() >= POSITION_EVERY {
                last_pos = Instant::now();
                self.emit(Event::Position { ms: self.position_ms(), playing: self.playing });
            }
        }
    }

    fn position_ms(&self) -> u64 {
        let rate = self.out.as_ref().map_or(48_000, |o| o.rate) as u64;
        let pos = self.base_ms + self.shared.played.load(Ordering::Relaxed) * 1000 / rate;
        match &self.src {
            Some(s) if s.duration_ms > 0 => pos.min(s.duration_ms),
            _ => pos,
        }
    }

    fn set_playing(&mut self, on: bool) {
        self.playing = on;
        self.shared.paused.store(!on, Ordering::Relaxed);
    }

    /// Forget buffered audio and count position from `ms`.
    fn reset_at(&mut self, ms: u64) {
        if let Ok(mut b) = self.shared.buf.lock() {
            b.clear();
        }
        self.shared.played.store(0, Ordering::Relaxed);
        self.base_ms = ms;
        self.resampler.reset();
    }

    fn handle(&mut self, c: Command) {
        match c {
            Command::Load { bytes, start_ms, play } => {
                self.src = None;
                self.reset_at(0);
                self.ended = false;
                match open(bytes) {
                    Ok(s) => {
                        let duration_ms = s.duration_ms;
                        self.src = Some(s);
                        if start_ms > 0 {
                            self.seek(start_ms);
                        }
                        self.emit(Event::Loaded { duration_ms });
                        self.set_playing(play);
                    }
                    Err(e) => {
                        self.set_playing(false);
                        self.emit(Event::Failed(e));
                    }
                }
            }
            Command::Play => {
                if self.src.is_some() {
                    if self.ended {
                        self.ended = false;
                        self.seek(0);
                    }
                    self.set_playing(true);
                }
            }
            Command::Pause => self.set_playing(false),
            Command::Seek(ms) => {
                self.ended = false;
                self.seek(ms);
            }
            Command::Volume(v) => self.shared.gain.store(gain(v).to_bits(), Ordering::Relaxed),
            Command::Stop => {
                self.src = None;
                self.set_playing(false);
                self.reset_at(0);
            }
        }
    }

    fn seek(&mut self, ms: u64) {
        let Some(s) = self.src.as_mut() else { return };
        let to = SeekTo::Time { time: Time::new(ms / 1000, (ms % 1000) as f64 / 1000.0), track_id: Some(s.track) };
        let at = match s.format.seek(SeekMode::Coarse, to) {
            Ok(done) => match s.time_base {
                Some(tb) => {
                    let t = tb.calc_time(done.actual_ts);
                    t.seconds * 1000 + (t.frac * 1000.0) as u64
                }
                None => ms,
            },
            Err(_) => {
                // Past the end: let it finish.
                s.eof = true;
                ms
            }
        };
        s.decoder.reset();
        if at < s.duration_ms || s.duration_ms == 0 {
            s.eof = false;
        }
        self.reset_at(at);
    }

    /// Open the output when a song needs it, and again after the device broke.
    fn keep_output(&mut self) {
        if self.shared.broken.swap(false, Ordering::Relaxed) && self.out.is_some() {
            // Carry the position over to the new device.
            let pos = self.position_ms();
            self.out = None;
            self.seek(pos);
        }
        if self.out.is_some() || self.src.is_none() {
            return;
        }
        if self.device_failed_at.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
            return;
        }
        match open_output(self.shared.clone()) {
            Ok(o) => {
                self.device_failed_at = None;
                self.out = Some(o);
                self.resampler.reset();
            }
            Err(e) => {
                if self.device_failed_at.is_none() {
                    self.emit(Event::Failed(format!("no sound device: {e}")));
                }
                self.device_failed_at = Some(Instant::now());
            }
        }
    }

    fn wants_data(&self) -> bool {
        let (Some(o), Some(s)) = (&self.out, &self.src) else { return false };
        if s.eof {
            return false;
        }
        let target = (o.rate as f64 * AHEAD_S) as usize * o.channels;
        self.shared.buf.lock().map(|b| b.len() < target).unwrap_or(false)
    }

    /// Decode one packet into the buffer. False at the end or on an error.
    fn decode_one(&mut self) -> bool {
        let (Some(o), Some(s)) = (&self.out, self.src.as_mut()) else { return false };
        let packet = match s.format.next_packet() {
            Ok(p) => p,
            Err(_) => {
                s.eof = true;
                return false;
            }
        };
        if packet.track_id() != s.track {
            return true;
        }
        let decoded = match s.decoder.decode(&packet) {
            Ok(d) => d,
            // A damaged packet: skip it and keep going.
            Err(symphonia::core::errors::Error::DecodeError(_)) => return true,
            Err(_) => {
                s.eof = true;
                return false;
            }
        };
        let spec = *decoded.spec();
        let in_ch = spec.channels.count().max(1);
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        samples.copy_interleaved_ref(decoded);
        let mapped = remix(samples.samples(), in_ch, o.channels);
        let mut out = Vec::with_capacity(mapped.len() * o.rate as usize / s.rate.max(1) as usize + 8);
        self.resampler.process(&mapped, o.channels, s.rate as f64 / o.rate as f64, &mut out);
        if let Ok(mut b) = self.shared.buf.lock() {
            b.extend(out);
        }
        true
    }
}

/// Volume 0 to 100 as a gain that sounds even across the range.
fn gain(v: u8) -> f32 {
    let x = v.min(100) as f32 / 100.0;
    x * x
}

fn open(bytes: Vec<u8>) -> Result<Source, String> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("m4a");
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
        .map_err(|e| format!("could not read the audio: {e}"))?;
    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("the file has no audio")?;
    let params = track.codec_params.clone();
    let decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| format!("could not decode the audio: {e}"))?;
    let rate = params.sample_rate.unwrap_or(44_100);
    let duration_ms = match (params.n_frames, params.time_base) {
        (Some(n), Some(tb)) => {
            let t = tb.calc_time(n);
            t.seconds * 1000 + (t.frac * 1000.0) as u64
        }
        (Some(n), None) => n * 1000 / rate as u64,
        _ => 0,
    };
    Ok(Source { track: track.id, format, decoder, time_base: params.time_base, rate, duration_ms, eof: false })
}

fn open_output(shared: Arc<Shared>) -> Result<Output, String> {
    let device = cpal::default_host().default_output_device().ok_or("none found")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = config.channels as usize;
    let on_error = {
        let shared = shared.clone();
        move |_e: cpal::StreamError| shared.broken.store(true, Ordering::Relaxed)
    };
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| fill(&shared, data, channels, |s| s),
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                fill(&shared, data, channels, |s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config,
            move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                fill(&shared, data, channels, |s| ((s.clamp(-1.0, 1.0) + 1.0) * 0.5 * u16::MAX as f32) as u16)
            },
            on_error,
            None,
        ),
        other => return Err(format!("the sound device wants {other} samples")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Output { _stream: stream, rate: config.sample_rate.0, channels })
}

/// The device callback: buffered audio times the gain, or silence.
fn fill<T: Copy>(shared: &Shared, data: &mut [T], channels: usize, conv: impl Fn(f32) -> T) {
    let silence = conv(0.0);
    if shared.paused.load(Ordering::Relaxed) {
        data.fill(silence);
        return;
    }
    let g = f32::from_bits(shared.gain.load(Ordering::Relaxed));
    // Never wait on the decoder: a missed lock is one buffer of silence.
    let Ok(mut buf) = shared.buf.try_lock() else {
        data.fill(silence);
        return;
    };
    let mut taken = 0usize;
    for d in data.iter_mut() {
        match buf.pop_front() {
            Some(s) => {
                *d = conv(s * g);
                taken += 1;
            }
            None => *d = silence,
        }
    }
    shared.played.fetch_add((taken / channels.max(1)) as u64, Ordering::Relaxed);
}

/// Interleaved samples from `from` channels to `to` channels.
fn remix(input: &[f32], from: usize, to: usize) -> Vec<f32> {
    if from == to {
        return input.to_vec();
    }
    let frames = input.len() / from;
    let mut out = Vec::with_capacity(frames * to);
    for f in input.chunks_exact(from) {
        if to == 1 {
            out.push(f.iter().sum::<f32>() / from as f32);
        } else {
            for c in 0..to {
                out.push(f[c.min(from - 1)]);
            }
        }
    }
    out
}

/// Linear interpolation between sample rates (44.1 kHz songs on 48 kHz
/// devices). Keeps the last frame so packets join without a click.
#[derive(Default)]
struct Resampler {
    pos: f64,
    prev: Vec<f32>,
}

impl Resampler {
    fn reset(&mut self) {
        self.pos = 0.0;
        self.prev.clear();
    }

    fn process(&mut self, input: &[f32], ch: usize, ratio: f64, out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if (ratio - 1.0).abs() < 1e-9 {
            out.extend_from_slice(input);
            return;
        }
        if self.prev.len() != ch {
            self.prev = input[..ch].to_vec();
        }
        let frames = input.len() / ch + 1;
        let at = |i: usize, c: usize| if i == 0 { self.prev[c] } else { input[(i - 1) * ch + c] };
        while self.pos + 1.0 < frames as f64 {
            let i = self.pos as usize;
            let t = (self.pos - i as f64) as f32;
            for c in 0..ch {
                out.push(at(i, c) * (1.0 - t) + at(i + 1, c) * t);
            }
            self.pos += ratio;
        }
        self.pos -= (frames - 1) as f64;
        self.prev = input[input.len() - ch..].to_vec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_to_the_right_length() {
        let mut r = Resampler::default();
        let mut out = Vec::new();
        let input: Vec<f32> = (0..44_100 * 2).map(|i| (i / 2) as f32).collect();
        for chunk in input.chunks(2048) {
            r.process(chunk, 2, 44_100.0 / 48_000.0, &mut out);
        }
        let frames = out.len() / 2;
        assert!((47_990..=48_010).contains(&frames), "{frames}");
        // Rising input stays rising across packet joins.
        assert!(out.chunks(2).zip(out.chunks(2).skip(1)).all(|(a, b)| b[0] >= a[0]));
    }

    #[test]
    fn remixes_channels() {
        assert_eq!(remix(&[1.0, 3.0], 2, 1), vec![2.0]);
        assert_eq!(remix(&[1.0, 2.0], 1, 2), vec![1.0, 1.0, 2.0, 2.0]);
    }

    #[test]
    fn plays_silence_while_paused() {
        let shared = Shared {
            buf: Mutex::new(VecDeque::from(vec![0.5; 8])),
            gain: AtomicU32::new(1f32.to_bits()),
            paused: AtomicBool::new(true),
            played: AtomicU64::new(0),
            broken: AtomicBool::new(false),
        };
        let mut data = [1.0f32; 4];
        fill(&shared, &mut data, 2, |s| s);
        assert_eq!(data, [0.0; 4]);
        shared.paused.store(false, Ordering::Relaxed);
        fill(&shared, &mut data, 2, |s| s);
        assert_eq!(data, [0.5; 4]);
        assert_eq!(shared.played.load(Ordering::Relaxed), 2);
    }
}
