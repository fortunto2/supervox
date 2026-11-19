//! Calibrate the voiceprint threshold: cluster a room recording, then print how each
//! cluster scores against each voiceprint in ~/.supervox/voices (session-centered and raw).
//!
//!     ffmpeg -i room.wav -ac 1 -ar 16000 -f f32le clip.f32
//!     cargo run -p voxkit --release --features speaker --example voiceprint_calibrate -- clip.f32 ~/.supervox/models/wespeaker_en_voxceleb_CAM++.onnx

use std::path::Path;
use voxkit::speaker::{SpeakerEmbedder, SpeakerTracker, load_voiceprints, voices_dir};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let raw = std::fs::read(&args[1]).expect("clip.f32");
    let samples = voxkit::types::f32_le_samples(&raw);
    let mut embedder = SpeakerEmbedder::new(Path::new(&args[2])).expect("speaker model");
    let vps = load_voiceprints(&voices_dir(), &mut embedder);
    println!(
        "voiceprints: {:?}",
        vps.iter()
            .map(|v| (&v.name, v.emb.len()))
            .collect::<Vec<_>>()
    );
    let win = 16_000 * 3;
    let mut embs = Vec::new();
    for w in samples.chunks(win).filter(|w| w.len() == win) {
        if voxkit::types::AudioChunk::new(w.to_vec(), 16_000).rms() < 0.005 {
            continue;
        }
        if let Ok(e) = embedder.embed(w) {
            embs.push(e);
        }
    }
    let mut tr = SpeakerTracker::new(6, 0.2).far_field();
    let mut counts = [0usize; 8];
    let mut ids = Vec::new();
    for e in &embs {
        let c = tr.assign(e).min(7);
        counts[c] += 1;
        ids.push(c);
    }
    let unit = |v: &[f32]| {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect::<Vec<f32>>()
    };
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    for vp in &vps {
        let v = vp.emb.to_vec();
        let mut per: Vec<Vec<f32>> = vec![Vec::new(); 8];
        for (e, c) in embs.iter().zip(&ids) {
            per[*c].push(dot(&unit(e), &v));
        }
        for (c, xs) in per.iter().enumerate().skip(1) {
            if xs.is_empty() {
                continue;
            }
            let mut ys = xs.clone();
            ys.sort_by(|a, b| a.total_cmp(b));
            println!(
                "raw {} S{c}: mean {:.3} p90 {:.3} max {:.3}",
                vp.name,
                xs.iter().sum::<f32>() / xs.len() as f32,
                ys[ys.len() * 9 / 10],
                ys[ys.len() - 1]
            );
        }
    }
    println!(
        "{} windows; clusters {}: {:?}",
        embs.len(),
        tr.speaker_count(),
        counts
    );
    for vp in &vps {
        let sims: Vec<String> = (0..tr.speaker_count())
            .map(|c| {
                let s = tr.voiceprint_similarity(c, vp).unwrap_or(f32::NAN);
                format!("S{}={s:.3}(n={})", c + 1, counts[c + 1])
            })
            .collect();
        println!("{:>8}: {}", vp.name, sims.join("  "));
    }
}
