//! cargo run --release -p analysis --example analyze -- [file.wav]
//! Without a file: analyzes a synthetic bass line and writes it to /tmp/analysis-bass.wav.
use analysis::{Frame, analyze};

fn main() {
    let (frames, sr) = match std::env::args().nth(1) {
        Some(path) => read_wav(&path),
        None => {
            let sr = 48_000.0;
            let f = bass_line(12.0, sr);
            write_wav("/tmp/analysis-bass.wav", &f, sr);
            eprintln!("wrote /tmp/analysis-bass.wav");
            (f, sr)
        }
    };
    let t = std::time::Instant::now();
    let a = analyze(&frames, sr);
    eprintln!("analysis took {:?}", t.elapsed());
    println!("{}", serde_json::to_string_pretty(&a).unwrap());
}

fn bass_line(secs: f32, sr: f32) -> Vec<Frame> {
    let notes = [41.2, 41.2, 55.0, 49.0, 41.2, 61.7, 55.0, 36.7];
    let mut phase = 0.0f32;
    (0..(secs * sr) as usize)
        .map(|i| {
            let t = i as f32 / sr;
            let step = (t / 0.25) as usize;
            let local = t - step as f32 * 0.25;
            phase = (phase + notes[step % notes.len()] / sr).fract();
            let vel = [1.0, 0.6, 0.9, 0.5][step % 4];
            let x = 0.5 * vel * (2.0 * phase - 1.0) * (-local * 6.0).exp() * (1.0 - (-local * 800.0).exp());
            [x, x * 0.9]
        })
        .collect()
}

fn write_wav(path: &str, frames: &[Frame], sr: f32) {
    let spec = hound::WavSpec { channels: 2, sample_rate: sr as u32, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for f in frames {
        w.write_sample(f[0]).unwrap();
        w.write_sample(f[1]).unwrap();
    }
    w.finalize().unwrap();
}

fn read_wav(path: &str) -> (Vec<Frame>, f32) {
    let mut r = hound::WavReader::open(path).unwrap();
    let spec = r.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(Result::unwrap).collect(),
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.unwrap() as f32 / scale).collect()
        }
    };
    let ch = spec.channels as usize;
    let frames = samples.chunks(ch).map(|c| [c[0], c[ch.min(2) - 1]]).collect();
    (frames, spec.sample_rate as f32)
}
