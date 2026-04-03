use std::{env, ffi::OsString};

pub fn platform_default_shell_impl() -> OsString {
    env::var_os("COMSPEC")
        .or_else(|| env::var_os("TERMVIDE_SHELL"))
        .unwrap_or_else(|| OsString::from("pwsh.exe"))
}
