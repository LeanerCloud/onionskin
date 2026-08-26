//! The Onionskin binary. M0 has no window: `--headless-boot` assembles
//! the registry, prints what it holds and exits.

use std::process::ExitCode;

const HEADLESS_BOOT: &str = "--headless-boot";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == HEADLESS_BOOT => {
            let registry = onionskin_app::build_registry();
            println!("{}", onionskin_app::boot_summary(&registry));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "onionskin: the windowed shell lands in M1; usage: onionskin {HEADLESS_BOOT}"
            );
            ExitCode::FAILURE
        }
    }
}
