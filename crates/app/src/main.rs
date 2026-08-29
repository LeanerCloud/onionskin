//! The Onionskin binary. Headless boot remains available in every build;
//! enabling `shell` also accepts one PDF path and opens the viewer window.

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
        #[cfg(feature = "shell")]
        [path] if !path.starts_with('-') => match onionskin_app::shell::run(path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("onionskin: {error}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("onionskin: usage: {}", usage());
            ExitCode::FAILURE
        }
    }
}

fn usage() -> &'static str {
    #[cfg(feature = "shell")]
    {
        "onionskin --headless-boot | onionskin <pdf-path>"
    }
    #[cfg(not(feature = "shell"))]
    {
        "onionskin --headless-boot (windowed PDF opening requires --features shell)"
    }
}
