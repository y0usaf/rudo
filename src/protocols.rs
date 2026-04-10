//! Wayland protocol bindings generated directly from XML via `wayland-scanner`.
//!
//! Only the protocols rudo actually uses are included here, avoiding the
//! compile-time cost of building all 53+ protocols from `wayland-protocols`.
//!
//! To regenerate after updating an XML file, just rebuild — the proc macros
//! expand at compile time from the files in `protocols/`.

#[allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#[allow(non_upper_case_globals, non_snake_case, unused_imports)]
#[allow(missing_docs, clippy::all)]
pub mod xdg_shell {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/xdg-shell.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./protocols/xdg-shell.xml");
}

#[allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#[allow(non_upper_case_globals, non_snake_case, unused_imports)]
#[allow(missing_docs, clippy::all)]
pub mod xdg_decoration {
    use wayland_client;
    use wayland_client::protocol::*;
    use super::xdg_shell::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        use super::super::xdg_shell::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/xdg-decoration-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./protocols/xdg-decoration-unstable-v1.xml");
}

#[allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#[allow(non_upper_case_globals, non_snake_case, unused_imports)]
#[allow(missing_docs, clippy::all)]
pub mod viewporter {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/viewporter.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./protocols/viewporter.xml");
}

#[allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#[allow(non_upper_case_globals, non_snake_case, unused_imports)]
#[allow(missing_docs, clippy::all)]
pub mod fractional_scale {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("./protocols/fractional-scale-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("./protocols/fractional-scale-v1.xml");
}
