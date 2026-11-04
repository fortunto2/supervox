//! Transcribe one WAV with local Parakeet: `cargo run -p voxkit --features parakeet --example parakeet_file -- <model_dir> <file.wav> [lang]`.
//! A known-answer check for a model directory (v2 English, v3 multilingual).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, wav) = (
        std::path::Path::new(&args[1]),
        std::path::Path::new(&args[2]),
    );
    let lang = args.get(3).map(String::as_str).unwrap_or("ru");
    let chunk = voxkit::types::read_wav_file(wav).expect("read wav");
    let t =
        voxkit::parakeet_stt::ParakeetStt::transcribe_file(dir, &chunk, lang).expect("transcribe");
    println!("{}", t.text);
}
