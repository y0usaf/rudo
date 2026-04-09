//! Minimal CLI argument parser for rudo (no external dependencies).

use crate::defaults::{APP_NAME, VERSION};

#[derive(Default)]
pub struct CliArgs {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub command: Vec<String>,
}

impl CliArgs {
    pub fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut cli = CliArgs::default();

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
                    cli.app_id = Some(expect_inline_value("--app-id", &arg["--app-id=".len()..]));
                }
                _ if arg.starts_with("--title=") => {
                    cli.title = Some(expect_inline_value("--title", &arg["--title=".len()..]));
                }
                "--" | "-e" => {
                    // -e is accepted for xterm compatibility; it simply stops
                    // option parsing. All remaining args become the command.
                    cli.command = args.collect();
                    return cli;
                }
                other if other.starts_with('-') => {
                    eprintln!("{APP_NAME}: unknown option '{other}'");
                    eprint!("{}", usage());
                    std::process::exit(1);
                }
                _ => {
                    // First positional argument starts the command.
                    cli.command.push(arg);
                    cli.command.extend(args);
                    return cli;
                }
            }
        }

        cli
    }
}

fn usage() -> String {
    format!(
        "Usage: {APP_NAME} [OPTIONS] [--] [command [ARGS...]]\n\nOptions:\n  -a, --app-id ID     Set the Wayland app-id (default: from config or \"{APP_NAME}\")\n  -t, --title TITLE   Set the initial window title\n  -e                   Ignored (xterm compat); stops option parsing\n      --               Stop option parsing; remaining args become the command\n  -h, --help          Print this help message and exit\n  -v, --version       Print version and exit\n"
    )
}

fn expect_value(flag: &str, args: &mut impl Iterator<Item = String>) -> String {
    args.next()
        .map(|value| expect_inline_value(flag, &value))
        .unwrap_or_else(|| {
            eprintln!("{APP_NAME}: option '{flag}' requires a value");
            std::process::exit(1);
        })
}

fn expect_inline_value(flag: &str, value: &str) -> String {
    if value.is_empty() {
        eprintln!("{APP_NAME}: option '{flag}' requires a non-empty value");
        std::process::exit(1);
    }

    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_args_are_empty() {
        let cli = CliArgs::default();
        assert!(cli.app_id.is_none());
        assert!(cli.title.is_none());
        assert!(cli.command.is_empty());
    }

    #[test]
    fn inline_value_helper_accepts_non_empty_values() {
        assert_eq!(expect_inline_value("--title", "rudo"), "rudo");
    }
}
