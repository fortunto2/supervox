//! Sweep the speaker-clustering threshold over a recording to see how many speakers each value finds.
//!
//!     ffmpeg -i call.wav -ac 1 -ar 16000 -f f32le clip.f32
//!     cargo run -p voxkit --features speaker --example diarize_sweep -- clip.f32 ~/.supervox/models/wespeaker_en_voxceleb_CAM++.onnx
//!
//! Cuts the clip into 3 s windows, skips quiet ones, embeds each, then replays the online tracker.

use std::path::Path;
use voxkit::speaker::{SpeakerEmbedder, SpeakerTracker};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let raw = std::fs::read(&args[1]).expect("clip.f32");
    let samples: Vec<f32> = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let mut embedder = SpeakerEmbedder::new(Path::new(&args[2])).expect("speaker model");
    let win = 16_000 * 3;
    let mut embs = Vec::new();
    for w in samples.chunks(win).filter(|w| w.len() == win) {
        let rms = (w.iter().map(|x| x * x).sum::<f32>() / w.len() as f32).sqrt();
        if rms < 0.005 {
            continue;
        }
        if let Ok(e) = embedder.embed(w) {
            embs.push(e);
        }
    }
    println!("{} voiced windows of 3 s", embs.len());
    let cos = |a: &[f32], b: &[f32]| {
        let d: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        d / (a.iter().map(|x| x * x).sum::<f32>().sqrt()
            * b.iter().map(|x| x * x).sum::<f32>().sqrt())
    };
    if embs.len() > 20 {
        println!("dim {} first {:?}", embs[0].len(), &embs[0][..4]);
        println!(
            "cos(0,1)={:.3} cos(0,last)={:.3} cos(0,mid)={:.3}",
            cos(&embs[0], &embs[1]),
            cos(&embs[0], embs.last().unwrap()),
            cos(&embs[0], &embs[embs.len() / 2])
        );
    }
    if std::env::var("CENTER").is_ok() {
        let dim = embs[0].len();
        let mut mean = vec![0f32; dim];
        for e in &embs {
            for (m, x) in mean.iter_mut().zip(e) {
                *m += x / embs.len() as f32;
            }
        }
        for e in embs.iter_mut() {
            for (x, m) in e.iter_mut().zip(&mean) {
                *x -= m;
            }
        }
        println!(
            "centered: cos(0,1)={:.3} cos(0,mid)={:.3}",
            cos(&embs[0], &embs[1]),
            cos(&embs[0], &embs[embs.len() / 2])
        );
    }
    for t in [
        0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9,
    ] {
        let mut tr = SpeakerTracker::new(8, t);
        let mut hist = [0usize; 9];
        for e in &embs {
            hist[tr.assign(e).min(8)] += 1;
        }
        let used: Vec<String> = hist
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .map(|(i, n)| format!("S{i}:{n}"))
            .collect();
        println!(
            "threshold {t:.2}: {} speakers  {}",
            tr.speaker_count(),
            used.join(" ")
        );
    }
}
