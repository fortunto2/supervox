//! Probe: cosine of 4 s windows of a recording vs the voiceprints, raw audio and after the live AGC.
//!
//!     cargo run -p voxkit --release --features speaker,wav --example sim_probe -- rec.wav model.onnx
use std::path::Path;
use voxkit::agc::Agc;
use voxkit::speaker::{SpeakerEmbedder, embed_voiceprint, voices_dir};

fn unit(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter().map(|x| x / n).collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = voxkit::types::read_wav_file(Path::new(&a[1])).expect("wav");
    let s = voxkit::types::resample(&c.samples, c.sample_rate, 16_000);
    let mut emb = SpeakerEmbedder::new(Path::new(&a[2])).expect("model");
    let vwav = voxkit::types::read_wav_file(&voices_dir().join("Рустам.wav")).expect("vp wav");
    let vs = voxkit::types::resample(&vwav.samples, vwav.sample_rate, 16_000);
    let vp = embed_voiceprint(&mut emb, &vs).expect("vp").to_vec();
    let mut agc_s = s.clone();
    Agc::default().process(&mut agc_s);
    // Optional third argument: a TSV of `id<TAB>start<TAB>end` (seconds): one similarity per span.
    if let Some(tsv) = a.get(3) {
        for line in std::fs::read_to_string(tsv).expect("tsv").lines() {
            let f: Vec<&str> = line.split('\t').collect();
            let (st, en): (f32, f32) = (f[1].parse().unwrap(), f[2].parse().unwrap());
            let (lo, hi) = (
                (st * 16000.0) as usize,
                ((en * 16000.0) as usize).min(s.len()),
            );
            if hi <= lo + 8000 {
                continue;
            }
            let w = &s[lo..hi];
            let sim = unit(&emb.embed(w).unwrap())
                .iter()
                .zip(&vp)
                .map(|(x, y)| x * y)
                .sum::<f32>();
            println!("{}\t{sim:.3}", f[0]);
        }
        return;
    }
    let win = 16_000 * 4;
    let mut i = 0;
    while i + win <= s.len() {
        let w = &s[i..i + win];
        let rms = (w.iter().map(|x| x * x).sum::<f32>() / w.len() as f32).sqrt();
        if rms > 0.003 {
            let raw = unit(&emb.embed(w).unwrap())
                .iter()
                .zip(&vp)
                .map(|(x, y)| x * y)
                .sum::<f32>();
            let ag = unit(&emb.embed(&agc_s[i..i + win]).unwrap())
                .iter()
                .zip(&vp)
                .map(|(x, y)| x * y)
                .sum::<f32>();
            println!(
                "t={:>5.1}s rms={rms:.4} raw={raw:.3} agc={ag:.3}",
                i as f32 / 16000.0
            );
        }
        i += win / 2;
    }
}
