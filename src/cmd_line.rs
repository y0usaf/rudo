use std::{ffi::OsString, iter};

use crate::{dimensions::Dimensions, frame::Frame, settings::*, version::BUILD_VERSION};

use anyhow::Result;
use clap::{
    ArgAction, Parser, ValueEnum,
    builder::{FalseyValueParser, Styles, styling},
};
#[cfg(target_os = "macos")]
use clap::{CommandFactory, parser::ValueSource};
use winit::window::CursorIcon;


#[cfg(target_os = "windows")]
pub const SRGB_DEFAULT: &str = "1";
#[cfg(not(target_os = "windows"))]
pub const SRGB_DEFAULT: &str = "0";

fn get_styles() -> Styles {
    styling::Styles::styled()
        .header(styling::AnsiColor::Green.on_default() | styling::Effects::BOLD)
        .usage(styling::AnsiColor::Green.on_default() | styling::Effects::BOLD)
        .literal(styling::AnsiColor::Blue.on_default() | styling::Effects::BOLD)
        .placeholder(styling::AnsiColor::Cyan.on_default())
}

#[derive(Clone, Debug, Parser)]
#[command(version = BUILD_VERSION, about, long_about = None, styles = get_styles())]
pub struct CmdLineSettings {
    /// If to enable logging to a file in the current directory
    #[arg(long = "log")]
    pub log_to_file: bool,

    /// Which window decorations to use (do note that the window might not be resizable
    /// if this is "none")
    #[arg(long, env = "TERMVIDE_FRAME", default_value_t)]
    pub frame: Frame,

    /// Which mouse cursor icon to use
    #[arg(long = "mouse-cursor-icon", env = "TERMVIDE_MOUSE_CURSOR_ICON", default_value = "arrow")]
    pub mouse_cursor_icon: MouseCursorIcon,

    /// Sets title hidden for the window
    #[arg(long = "title-hidden", env = "TERMVIDE_TITLE_HIDDEN", value_parser = FalseyValueParser::new())]
    pub title_hidden: bool,

    /// Spawn a child process and leak it
    #[arg(long = "fork", env = "TERMVIDE_FORK", action = ArgAction::SetTrue, default_value = "0", value_parser = FalseyValueParser::new())]
    pub fork: bool,

    /// Be "blocking" and let the shell persist as parent process. Takes precedence over `--fork`. [DEFAULT]
    #[arg(long = "no-fork", action = ArgAction::SetTrue, value_parser = FalseyValueParser::new())]
    _no_fork: bool,

    /// Request sRGB when initializing the window, may help with GPUs with weird pixel
    /// formats. Default on Windows.
    #[arg(long = "srgb", env = "TERMVIDE_SRGB", action = ArgAction::SetTrue, default_value = SRGB_DEFAULT, value_parser = FalseyValueParser::new())]
    pub srgb: bool,

    /// Do not request sRGB when initializing the window, may help with GPUs with weird pixel
    /// formats. Default on Linux and macOS.
    #[arg(long = "no-srgb", action = ArgAction::SetTrue, value_parser = FalseyValueParser::new())]
    _no_srgb: bool,

    /// Request VSync on the window [DEFAULT]
    #[arg(long = "vsync", env = "TERMVIDE_VSYNC", action = ArgAction::SetTrue, default_value = "1", value_parser = FalseyValueParser::new())]
    pub vsync: bool,

    /// Do not try to request VSync on the window
    #[arg(long = "no-vsync", action = ArgAction::SetTrue, value_parser = FalseyValueParser::new())]
    _no_vsync: bool,

    /// The app ID to show to the compositor (Wayland only, useful for setting WM rules)
    #[arg(long = "wayland_app_id", env = "TERMVIDE_APP_ID", default_value = "termvide")]
    pub wayland_app_id: String,

    /// The class part of the X11 WM_CLASS property (X only, useful for setting WM rules)
    #[arg(long = "x11-wm-class", env = "TERMVIDE_WM_CLASS", default_value = "termvide")]
    pub x11_wm_class: String,

    /// The instance part of the X11 WM_CLASS property (X only, useful for setting WM rules)
    #[arg(
        long = "x11-wm-class-instance",
        env = "TERMVIDE_WM_CLASS_INSTANCE",
        default_value = "termvide"
    )]
    pub x11_wm_class_instance: String,

    /// The custom icon to use for the app.
    #[arg(long, env = "TERMVIDE_ICON")]
    pub icon: Option<String>,

    #[command(flatten)]
    pub geometry: GeometryArgs,

    /// Force opengl on Windows or macOS
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[arg(long = "opengl", env = "TERMVIDE_OPENGL", action = ArgAction::SetTrue, value_parser = FalseyValueParser::new())]
    pub opengl: bool,

    /// Change to this directory during startup.
    #[arg(long = "chdir", env = "TERMVIDE_CHDIR")]
    pub chdir: Option<String>,

    /// Ignored (for compatibility with xterm -e)
    #[arg(short = 'e', hide = true)]
    _xterm_compat: bool,

    /// Command to execute instead of the default shell
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<OsString>,
}

// geometry, size and maximized are mutually exclusive
#[derive(Clone, Debug, Args, PartialEq)]
#[group(required = false, multiple = false)]
pub struct GeometryArgs {
    /// The initial grid size of the window [<columns>x<lines>]. Defaults to columns/lines from init.vim/lua if no value is given.
    /// If --grid is not set then it's inferred from the window size
    #[arg(long, env = "TERMVIDE_GRID")]
    pub grid: Option<Option<Dimensions>>,

    /// The size of the window in pixels.
    #[arg(long, env = "TERMVIDE_SIZE")]
    pub size: Option<Dimensions>,

    /// Maximize the window on startup (not equivalent to fullscreen)
    #[arg(long, env = "TERMVIDE_MAXIMIZED", value_parser = FalseyValueParser::new())]
    pub maximized: bool,
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum MouseCursorIcon {
    Arrow,
    IBeam,
}

impl MouseCursorIcon {
    pub fn from_config(value: Option<&str>) -> Result<Self, String> {
        value.map_or(Ok(Self::Arrow), |value| <Self as ValueEnum>::from_str(value, false))
    }

    pub fn parse(&self) -> CursorIcon {
        match self {
            MouseCursorIcon::Arrow => CursorIcon::Default,
            MouseCursorIcon::IBeam => CursorIcon::Text,
        }
    }
}

impl GeometryArgs {
    pub fn from_config(
        size: Option<&str>,
        grid: Option<&str>,
        maximized: Option<bool>,
    ) -> Result<Self, String> {
        let maximized = maximized.unwrap_or(false);
        let has_size = size.is_some();
        let has_grid = grid.is_some();
        let conflicting = (has_size && has_grid) || (maximized && (has_size || has_grid));
        if conflicting {
            return Err("size, grid and maximized are mutually exclusive".to_owned());
        }

        Ok(Self {
            grid: grid.map(|grid| grid.parse::<Dimensions>().map(Some)).transpose()?,
            size: size.map(str::parse::<Dimensions>).transpose()?,
            maximized,
        })
    }
}

impl Default for CmdLineSettings {
    fn default() -> Self {
        Self::parse_from(iter::empty::<String>())
    }
}

pub fn handle_command_line_arguments(args: Vec<String>, settings: &Settings) -> Result<()> {
    let mut cmdline = CmdLineSettings::try_parse_from(args)?;

    if cmdline._no_fork {
        cmdline.fork = false;
    }

    if cmdline._no_srgb {
        cmdline.srgb = false;
    }

    if cmdline._no_vsync {
        cmdline.vsync = false;
    }

    settings.set::<CmdLineSettings>(&cmdline);
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn argv_chdir() -> Option<String> {
    let matches = CmdLineSettings::command().try_get_matches_from(std::env::args_os()).ok()?;

    (matches.value_source("chdir") == Some(ValueSource::CommandLine))
        .then(|| matches.get_one::<String>("chdir").cloned())
        .flatten()
}

#[cfg(test)]
#[allow(clippy::bool_assert_comparison)] // useful here since the explicit true/false comparison matters
#[serial_test::serial]
mod tests {
    use scoped_env::ScopedEnv;

    use super::*;

    #[test]
    fn test_grid() {
        let settings = Settings::new();
        let args: Vec<String> =
            ["termvide", "--grid=420x240"].iter().map(|s| s.to_string()).collect();

        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(
            settings.get::<CmdLineSettings>().geometry.grid,
            Some(Some(Dimensions { width: 420, height: 240 })),
        );
    }

    #[test]
    fn test_grid_environment_variable() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide"].iter().map(|s| s.to_string()).collect();

        let _env = ScopedEnv::set("TERMVIDE_GRID", "420x240");
        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(
            settings.get::<CmdLineSettings>().geometry.grid,
            Some(Some(Dimensions { width: 420, height: 240 })),
        );
    }

    #[test]
    fn test_size() {
        let settings = Settings::new();
        let args: Vec<String> =
            ["termvide", "--size=420x240"].iter().map(|s| s.to_string()).collect();

        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(
            settings.get::<CmdLineSettings>().geometry.size,
            Some(Dimensions { width: 420, height: 240 }),
        );
    }

    #[test]
    fn test_size_environment_variable() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide"].iter().map(|s| s.to_string()).collect();

        let _env = ScopedEnv::set("TERMVIDE_SIZE", "420x240");
        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(
            settings.get::<CmdLineSettings>().geometry.size,
            Some(Dimensions { width: 420, height: 240 }),
        );
    }

    #[test]
    fn test_geometry_args_from_config_size() {
        assert_eq!(
            GeometryArgs::from_config(Some("420x240"), None, None).unwrap(),
            GeometryArgs {
                size: Some(Dimensions { width: 420, height: 240 }),
                grid: None,
                maximized: false,
            }
        );
    }

    #[test]
    fn test_geometry_args_from_config_grid() {
        assert_eq!(
            GeometryArgs::from_config(None, Some("80x24"), None).unwrap(),
            GeometryArgs {
                size: None,
                grid: Some(Some(Dimensions { width: 80, height: 24 })),
                maximized: false,
            }
        );
    }

    #[test]
    fn test_geometry_args_from_config_rejects_conflicts() {
        assert_eq!(
            GeometryArgs::from_config(Some("420x240"), Some("80x24"), None).unwrap_err(),
            "size, grid and maximized are mutually exclusive"
        );
    }

    #[test]
    fn test_log_to_file() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide", "--log"].iter().map(|s| s.to_string()).collect();

        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert!(settings.get::<CmdLineSettings>().log_to_file);
    }

    #[test]
    fn test_frameless_flag() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide", "--frame=full"].iter().map(|s| s.to_string()).collect();

        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().frame, Frame::Full);
    }

    #[test]
    fn test_frameless_environment_variable() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide"].iter().map(|s| s.to_string()).collect();

        let _env = ScopedEnv::set("TERMVIDE_FRAME", "none");
        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().frame, Frame::None);
    }

    #[test]
    fn test_srgb_default() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide"].iter().map(|s| s.to_string()).collect();

        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        #[cfg(target_os = "windows")]
        let default_value = true;
        #[cfg(not(target_os = "windows"))]
        let default_value = false;
        assert_eq!(settings.get::<CmdLineSettings>().srgb, default_value);
    }

    #[test]
    fn test_srgb() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide", "--srgb"].iter().map(|s| s.to_string()).collect();

        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().srgb, true);
    }

    #[test]
    fn test_nosrgb() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide", "--no-srgb"].iter().map(|s| s.to_string()).collect();

        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().srgb, false);
    }

    #[test]
    fn test_no_srgb_environment() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide"].iter().map(|s| s.to_string()).collect();

        let _env = ScopedEnv::set("TERMVIDE_SRGB", "0");
        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().srgb, false);
    }

    #[test]
    fn test_override_srgb_environment() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide", "--no-srgb"].iter().map(|s| s.to_string()).collect();

        let _env = ScopedEnv::set("TERMVIDE_SRGB", "1");
        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().srgb, false);
    }

    #[test]
    fn test_override_nosrgb_environment() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide", "--srgb"].iter().map(|s| s.to_string()).collect();

        let _env = ScopedEnv::set("TERMVIDE_SRGB", "0");
        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().srgb, true,);
    }

    #[test]
    fn test_vsync_default() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide"].iter().map(|s| s.to_string()).collect();

        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().vsync, true);
    }

    #[test]
    fn test_vsync() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide", "--vsync"].iter().map(|s| s.to_string()).collect();

        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().vsync, true);
    }

    #[test]
    fn test_novsync() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide", "--no-vsync"].iter().map(|s| s.to_string()).collect();

        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().vsync, false);
    }

    #[test]
    fn test_no_vsync_environment() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide"].iter().map(|s| s.to_string()).collect();

        let _env = ScopedEnv::set("TERMVIDE_VSYNC", "0");
        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().vsync, false);
    }

    #[test]
    fn test_override_vsync_environment() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide", "--no-vsync"].iter().map(|s| s.to_string()).collect();

        let _env = ScopedEnv::set("TERMVIDE_VSYNC", "1");
        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().vsync, false);
    }

    #[test]
    fn test_override_novsync_environment() {
        let settings = Settings::new();
        let args: Vec<String> = ["termvide", "--vsync"].iter().map(|s| s.to_string()).collect();

        let _env = ScopedEnv::set("TERMVIDE_VSYNC", "0");
        handle_command_line_arguments(args, &settings).expect("Could not parse arguments");
        assert_eq!(settings.get::<CmdLineSettings>().vsync, true,);
    }
}
