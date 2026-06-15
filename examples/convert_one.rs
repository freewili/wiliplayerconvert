// Headless one-shot conversion — useful for scripting/demoing the pipeline
// without the GUI. Usage: convert_one <input-video> <dest-dir>
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: convert_one <input-video> <dest-dir>");
        std::process::exit(2);
    }
    let input = Path::new(&args[1]);
    let dest = Path::new(&args[2]);
    std::fs::create_dir_all(dest).expect("create dest dir");

    let res = wiliplayerconvert::convert::convert_file(input, dest, |f| {
        eprintln!("progress {:.0}%", f * 100.0);
    });
    match res {
        Ok(out) => println!("wrote {}", out.display()),
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    }
}
