fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=csrc/rudo_font.c");
    println!("cargo:rustc-link-lib=dl");

    let mut build = cc::Build::new();
    build.file("csrc/rudo_font.c");
    build.flag_if_supported("-std=c11");
    build.flag_if_supported("-O3");
    build.flag_if_supported("-fstrict-aliasing");
    build.flag_if_supported("-fomit-frame-pointer");
    build.flag_if_supported("-fno-asynchronous-unwind-tables");
    build.flag_if_supported("-fno-unwind-tables");
    build.compile("rudo_font");
}
