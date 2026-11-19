//! Smart-turn probability for WAV files (known-answer check against talk/scripts/turns.py).
fn main() {
    let mut st = voxkit::smart_turn::SmartTurn::new(&voxkit::smart_turn::default_model_path())
        .expect("model");
    for p in std::env::args().skip(1) {
        let c = voxkit::types::read_wav_file(std::path::Path::new(&p)).expect("wav");
        println!(
            "{} p={:.4}",
            p.rsplit('/').next().unwrap(),
            st.probability(&c.samples).unwrap()
        );
    }
}
