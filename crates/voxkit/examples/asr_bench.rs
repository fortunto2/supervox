//! Per-segment latency on a recording: `cargo run --release -p voxkit --features "parakeet smart-turn [coreml]" --example asr_bench -- <model_dir> <file.wav> [smart|vad] [coreml]`.
//! Prints, per segment, how long the cut waited for silence (`wait_ms`), `asr_ms`, the text.
use std::time::Instant;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (dir, wav) = (std::path::Path::new(&a[1]), std::path::Path::new(&a[2]));
    let mode = a.get(3).map(String::as_str).unwrap_or("smart");
    let coreml = a.get(4).map(String::as_str) == Some("coreml");
    let chunk = voxkit::types::read_wav_file(wav).expect("read wav");
    let t = Instant::now();
    let rows = voxkit::parakeet_stt::bench_file(
        dir,
        &chunk.samples,
        chunk.sample_rate,
        mode == "smart",
        coreml,
    );
    let (mut sum, mut n, mut wait) = (0u128, 0, 0u32);
    for (secs, wait_ms, asr_ms, text) in &rows {
        println!("secs={secs:.1} wait_ms={wait_ms} asr_ms={asr_ms}: {text}");
        sum += asr_ms;
        n += 1;
        wait += wait_ms;
    }
    println!(
        "MODE {mode}{} segments={n} mean_asr_ms={} mean_wait_ms={} total_s={:.1}",
        if coreml { "+coreml" } else { "" },
        sum / n.max(1) as u128,
        wait / n.max(1),
        t.elapsed().as_secs_f32()
    );
}
