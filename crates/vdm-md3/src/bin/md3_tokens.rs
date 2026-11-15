//! `md3-tokens` — export Material You scheme tokens as JSON.
//!
//! The non-Rust frontends (Swift/AppKit, Kotlin) consume these tables
//! either at build time (checked-in generated sources with a
//! provenance comment) or at runtime. Same engine as the in-process
//! frontends (`vdm-md3`), so every OS renders the same roles from one
//! seed.
//!
//! Usage:
//!   md3-tokens --seed 4156A4 [--dark | --light | --both]

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut seed: u32 = vdm_md3::DEFAULT_SEED;
    let mut mode = "both";
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seed" => match args.next().and_then(|s| vdm_md3::Scheme::parse_hex(&s)) {
                Some(v) => seed = v,
                None => {
                    eprintln!("--seed expects a hex color like 4156A4 or #4156A4");
                    return ExitCode::FAILURE;
                }
            },
            "--dark" => mode = "dark",
            "--light" => mode = "light",
            "--both" => mode = "both",
            other => {
                eprintln!("unknown argument: {other}");
                eprintln!("usage: md3-tokens --seed 4156A4 [--dark | --light | --both]");
                return ExitCode::FAILURE;
            }
        }
    }

    let emit = |dark: bool| {
        let s = vdm_md3::Scheme::from_seed_hex(seed, dark);
        match serde_json::to_string_pretty(&s) {
            Ok(j) => println!("{}", j),
            Err(e) => {
                eprintln!("serialize: {e}");
                std::process::exit(2);
            }
        }
    };

    match mode {
        "dark" => emit(true),
        "light" => emit(false),
        _ => {
            println!("// seed #{:06X} light", seed);
            emit(false);
            println!();
            println!("// seed #{:06X} dark", seed);
            emit(true);
        }
    }
    ExitCode::SUCCESS
}
