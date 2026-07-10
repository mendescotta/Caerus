use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");

    // Locate libxbps the same way meson's dependency('libxbps', ...) did
    // in the original C project: via pkg-config. Requires xbps-devel /
    // libxbps-devel to be installed on the build machine.
    let lib = pkg_config::Config::new()
        .atleast_version("0.59")
        .probe("libxbps")
        .expect(
            "libxbps not found via pkg-config. On Void Linux: \
             xbps-install -S libxbps-devel",
        );

    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg("-D_GNU_SOURCE")
        // Keep the generated surface small and intentional: only the
        // xbps_*/prop_*/XBPS_* symbols actually used by src/backend, not
        // the entire libxbps + libprop transitive header surface.
        .allowlist_function("xbps_.*")
        .allowlist_type("xbps_.*")
        .allowlist_type("prop_.*")
        .allowlist_var("XBPS_.*")
        .derive_default(true)
        .layout_tests(false)
        .generate_comments(true)
        .default_enum_style(bindgen::EnumVariation::Rust {
            non_exhaustive: false,
        });

    for inc in &lib.include_paths {
        builder = builder.clang_arg(format!("-I{}", inc.display()));
    }

    let bindings = builder
        .generate()
        .expect("failed to generate xbps-sys bindings from <xbps.h>");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("failed to write xbps-sys bindings.rs");

    for path in &lib.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    for l in &lib.libs {
        println!("cargo:rustc-link-lib={}", l);
    }
}
