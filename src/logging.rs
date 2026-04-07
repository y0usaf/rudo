#[cfg(not(debug_assertions))]
const INFO_ENV_VARS: [&str; 2] = ["RUDO_LOG_INFO", "TERMVIDE_LOG_INFO"];

#[inline]
pub(crate) fn info_enabled() -> bool {
    #[cfg(debug_assertions)]
    {
        true
    }

    #[cfg(not(debug_assertions))]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| {
            INFO_ENV_VARS.iter().any(|key| match std::env::var(key) {
                Ok(value) => matches!(
                    value.as_str(),
                    "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
                ),
                Err(_) => false,
            })
        })
    }
}

#[macro_export]
macro_rules! info_log {
    ($($arg:tt)*) => {{
        if $crate::logging::info_enabled() {
            eprintln!("[INFO] {}", format_args!($($arg)*));
        }
    }};
}

#[macro_export]
macro_rules! warn_log {
    ($($arg:tt)*) => {{
        eprintln!("[WARN] {}", format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! error_log {
    ($($arg:tt)*) => {{
        eprintln!("[ERROR] {}", format_args!($($arg)*));
    }};
}
