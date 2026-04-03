use std::{env, ffi::OsString, fs};

pub fn platform_default_shell_impl() -> OsString {
    env::var_os("TERMVIDE_SHELL")
        .or_else(shell_from_passwd)
        .or_else(|| env::var_os("SHELL"))
        .unwrap_or_else(|| OsString::from("/bin/sh"))
}

fn shell_from_passwd() -> Option<OsString> {
    let uid = rustix::process::getuid().as_raw();
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    parse_shell_from_passwd(&passwd, uid)
}

fn parse_shell_from_passwd(passwd: &str, uid: u32) -> Option<OsString> {
    passwd.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).find_map(|line| {
        let mut parts = line.split(':');
        let _name = parts.next()?;
        let _password = parts.next()?;
        let uid_field = parts.next()?;
        let _gid = parts.next()?;
        let _gecos = parts.next()?;
        let _home = parts.next()?;
        let shell = parts.next()?;

        (uid_field.parse::<u32>().ok()? == uid && !shell.trim().is_empty())
            .then(|| OsString::from(shell.trim()))
    })
}

#[cfg(test)]
mod tests {
    use super::parse_shell_from_passwd;
    use std::ffi::OsString;

    #[test]
    fn parses_shell_for_matching_uid() {
        let passwd = "root:x:0:0:root:/root:/bin/sh\nuser:x:1000:1000:User:/home/user:/run/current-system/sw/bin/nu\n";
        assert_eq!(
            parse_shell_from_passwd(passwd, 1000),
            Some(OsString::from("/run/current-system/sw/bin/nu"))
        );
    }

    #[test]
    fn returns_none_when_uid_missing() {
        let passwd = "root:x:0:0:root:/root:/bin/sh\n";
        assert_eq!(parse_shell_from_passwd(passwd, 1000), None);
    }
}
