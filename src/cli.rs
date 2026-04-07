//! Minimal CLI argument parser for rudo (no external dependencies).

use crate::defaults::{APP_NAME, VERSION};

pub struct CliArgs {
    pub app_id: Option<String>,
    pub title: Option<String>,
}

impl CliArgs {
    pub fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut cli = CliArgs {
            app_id: None,
            title: None,
        };

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => {
                    print!("{}", usage());
                    std::process::exit(0);
                }
                "-v" | "--version" => {
                    println!("{APP_NAME} {VERSION}");
                    std::process::exit(0);
                }
                "-a" => {
                    cli.app_id = Some(expect_value(&arg, &mut args));
                }
                "--app-id" => {
                    cli.app_id = Some(expect_value(&arg, &mut args));
                }
                "-t" => {
                    cli.title = Some(expect_value(&arg, &mut args));
                }
                "--title" => {
                    cli.title = Some(expect_value(&arg, &mut args));
                }
                _ if arg.starts_with("--app-id=") => {
                    cli.app_id = Some(arg["--app-id=".len()..].to_string());
                }
                _ if arg.starts_with("--title=") => {
                    cli.title = Some(arg["--title=".len()..].to_string());
                }
                other => {
                    eprintln!("{APP_NAME}: unknown option '{other}'");
                    eprint!("{}", usage());
                    std::process::exit(1);
                }
            }
        }

        cli
    }
}

fn usage() -> String {
    format!(
        "Usage: {APP_NAME} [OPTIONS]\n\nOptions:\n  -a, --app-id ID     Set the Wayland app-id (default: from config or \"{APP_NAME}\")\n  -t, --title TITLE   Set the initial window title\n  -h, --help          Print this help message and exit\n  -v, --version       Print version and exit\n"
    )
}

fn expect_value(flag: &str, args: &mut impl Iterator<Item = String>) -> String {
    args.next().unwrap_or_else(|| {
        eprintln!("{APP_NAME}: option '{flag}' requires a value");
        std::process::exit(1);
    })
}
