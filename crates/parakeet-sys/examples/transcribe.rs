// cargo run --release --example transcribe -- model.bin audio-16k-mono.wav
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&args[2]).unwrap();
    // Minimal WAV reader: 16-bit PCM mono at 16 kHz, data after the 44-byte header.
    let samples: Vec<f32> = bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    let t = std::time::Instant::now();
    let mut m = parakeet_sys::Model::load(std::path::Path::new(&args[1]), true).unwrap();
    println!("load {:?} | {}", t.elapsed(), parakeet_sys::system_info());
    let t = std::time::Instant::now();
    let text = m.transcribe(&samples, 8).unwrap();
    println!("{:?}: {text}", t.elapsed());
}
