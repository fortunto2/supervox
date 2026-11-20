//! Voiceprint naming on a hand-labelled ground truth: per-segment raw match (live today) against a
//! cluster seeded with the voiceprint whose centroid follows the segments it takes.
//!
//!     cargo run -p voxkit --release --features speaker,wav --example voiceprint_gt -- \
//!         model.onnx ~/.supervox/calls jsonl_dir crates/voxkit/examples/voiceprint_gt.tsv
//!
//! `jsonl_dir/<stamp>.full.jsonl` is `supervox replay recording-20261119-<stamp>.wav` output
//! (spans of every live segment); `voiceprint_gt.tsv` (19.11.2026 recordings) is `stamp<TAB>a<TAB>b<TAB>R|O<TAB>note` (R = Rustam, O = someone else), labelled by
//! content and by pitch, never by the voiceprint. Each recording is one session, in order.
use ndarray::Array1;
use std::collections::HashMap;
use std::path::Path;
use voxkit::agc::Agc;
use voxkit::paths::voices_dir;
use voxkit::speaker::{SpeakerEmbedder, load_voiceprints, normalize, raw_similarity};

/// The live audio path: 100 ms chunks -> 24 kHz i16 -> 16 kHz f32, then AGC.
fn live_audio(wav: &Path) -> Vec<f32> {
    let c = voxkit::types::read_wav_file(wav).expect("wav");
    let mut out = Vec::new();
    for ch in c.samples.chunks(c.sample_rate as usize / 10) {
        let i = voxkit::types::resample_to_24k_i16(ch, c.sample_rate);
        let f: Vec<f32> = i.iter().map(|&s| s as f32 / 32768.0).collect();
        out.extend(voxkit::types::resample(&f, 24000, 16000));
    }
    Agc::default().process(&mut out);
    out
}

struct Seeded {
    named: Array1<f32>,
    n: f32,
    anon: Vec<Array1<f32>>,
    join: f32,
}

impl Seeded {
    fn new(vp: &Array1<f32>, join: f32) -> Self {
        Self {
            named: vp.clone(),
            n: 3.0,
            anon: Vec::new(),
            join,
        }
    }
    /// True when the segment is taken by the named cluster.
    fn assign(&mut self, e: &Array1<f32>) -> bool {
        let s0 = raw_similarity(e, &self.named);
        let best_anon = self
            .anon
            .iter()
            .map(|c| raw_similarity(e, c))
            .fold(f32::MIN, f32::max);
        if s0 >= self.join && s0 >= best_anon {
            self.named = normalize(&self.named * self.n + e);
            self.n = (self.n + 1.0).min(20.0);
            return true;
        }
        match self
            .anon
            .iter()
            .position(|c| raw_similarity(e, c) == best_anon && best_anon >= 0.6)
        {
            Some(i) => self.anon[i] = normalize(&self.anon[i] + e),
            None => self.anon.push(e.clone()),
        }
        false
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut emb = SpeakerEmbedder::new(Path::new(&a[1])).expect("model");
    let vp = load_voiceprints(&voices_dir(), &mut emb)
        .into_iter()
        .next()
        .expect("voiceprint")
        .emb;
    let gt: Vec<Vec<String>> = std::fs::read_to_string(&a[4])
        .unwrap()
        .lines()
        .map(|l| l.split('\t').map(str::to_owned).collect())
        .collect();
    let mut stamps: Vec<String> = gt.iter().map(|r| r[0].clone()).collect();
    stamps.dedup();
    stamps.sort();
    stamps.dedup();

    // (is_rustam, per-segment sim, seeded verdict per join threshold)
    let joins = [0.5f32, 0.55, 0.6, 0.65];
    let mut rows: Vec<(bool, f32, Vec<bool>, String)> = Vec::new();
    for st in &stamps {
        let audio = live_audio(&Path::new(&a[2]).join(format!("recording-20261119-{st}.wav")));
        let jl =
            std::fs::read_to_string(Path::new(&a[3]).join(format!("{st}.full.jsonl"))).unwrap();
        let gts: HashMap<i64, (bool, String)> = gt
            .iter()
            .filter(|r| &r[0] == st)
            .map(|r| {
                (
                    (r[1].parse::<f32>().unwrap() * 100.0).round() as i64,
                    (r[3] == "R", r[4].clone()),
                )
            })
            .collect();
        let mut seeded: Vec<Seeded> = joins.iter().map(|j| Seeded::new(&vp, *j)).collect();
        for l in jl.lines() {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            let (s, e) = (
                v["a"].as_f64().unwrap() as f32,
                v["b"].as_f64().unwrap() as f32,
            );
            let (lo, hi) = (
                (s * 16000.0) as usize,
                ((e * 16000.0) as usize).min(audio.len()),
            );
            if (hi - lo) as f32 / 16000.0 < voxkit::speaker::MIN_EMBED_SECS {
                continue;
            }
            let u = normalize(Array1::from_vec(emb.embed(&audio[lo..hi]).unwrap()));
            let sim = raw_similarity(&u, &vp);
            let verdicts: Vec<bool> = seeded.iter_mut().map(|c| c.assign(&u)).collect();
            if let Some((is_r, note)) = gts.get(&((s * 100.0).round() as i64)) {
                rows.push((*is_r, sim, verdicts, format!("{st} {s:.0}s {note}")));
            }
        }
    }
    let (pos, neg): (Vec<_>, Vec<_>) = rows.iter().partition(|r| r.0);
    println!("GT: {} Rustam, {} others", pos.len(), neg.len());
    println!("method               accepted/Rustam  false-accepts/others");
    for t in [0.45f32, 0.5, 0.55, 0.6, 0.65, 0.7] {
        println!(
            "per-segment >= {t:.2}    {:>2}/{}            {:>2}/{}",
            pos.iter().filter(|r| r.1 >= t).count(),
            pos.len(),
            neg.iter().filter(|r| r.1 >= t).count(),
            neg.len()
        );
    }
    for (i, j) in joins.iter().enumerate() {
        println!(
            "seeded cluster {j:.2}    {:>2}/{}            {:>2}/{}",
            pos.iter().filter(|r| r.2[i]).count(),
            pos.len(),
            neg.iter().filter(|r| r.2[i]).count(),
            neg.len()
        );
    }
    println!("-- misses at per-segment 0.65 / seeded 0.55, and every false accept");
    for r in &rows {
        if (r.0 && (r.1 < 0.65 || !r.2[1])) || (!r.0 && (r.1 >= 0.5 || r.2[1])) {
            println!(
                "{} sim={:.2} seeded={:?} {}",
                if r.0 { "R" } else { "O" },
                r.1,
                r.2,
                r.3
            );
        }
    }
}
