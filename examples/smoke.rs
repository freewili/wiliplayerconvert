// Headless conversion smoke test: convert one input to a temp dir and report.
// Verifies the decode->pack->write path (esp. AV1 via libdav1d) without the GUI.
//   cargo run --release --example smoke -- <input> [more inputs...]
use std::path::PathBuf;

fn main() {
    let inputs: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if inputs.is_empty() {
        eprintln!("usage: smoke <input.mp4> [...]");
        std::process::exit(2);
    }
    let dest = std::env::temp_dir().join("wiliplayerconvert_smoke");
    std::fs::create_dir_all(&dest).unwrap();

    let mut failures = 0;
    for input in &inputs {
        let name = input.file_name().unwrap().to_string_lossy().into_owned();
        match wiliplayerconvert::convert::convert_file(input, &dest, |_| {}) {
            Ok(out) => {
                let sz = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                println!("OK    {name} -> {} ({} bytes)", out.display(), sz);
            }
            Err(e) => {
                failures += 1;
                println!("FAIL  {name}: {e}");
            }
        }
    }
    if failures > 0 {
        std::process::exit(1);
    }
}
