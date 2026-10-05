use std::env;
use std::path::PathBuf;

fn compiler_include_dirs() -> Vec<String> {
    let mut dirs = Vec::new();
    for cc in ["gcc", "cc"] {
        if let Ok(out) = std::process::Command::new(cc)
            .arg("-print-file-name=include")
            .output()
        {
            let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if dir.starts_with('/') && std::path::Path::new(&dir).join("stddef.h").exists() {
                dirs.push(dir);
            }
        }
    }
    dirs
}

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");

    let lib = pkg_config::Config::new()
        .atleast_version("0.59")
        .probe("libxbps")
        .expect(
            "libxbps not found via pkg-config. On Void Linux: \
             xbps-install -S libxbps-devel",
        );

    let make = |extra_include: Option<&str>| {
        let mut builder = bindgen::Builder::default()
            .header("wrapper.h")
            .clang_arg("-D_GNU_SOURCE")
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
        if let Some(dir) = extra_include {
            builder = builder.clang_arg(format!("-isystem{dir}"));
        }
        builder.generate()
    };

    let bindings = make(None)
        .or_else(|first_err| {
            // libclang sometimes cannot find its own resource headers
            // (stddef.h) when several LLVM versions are installed.
            compiler_include_dirs()
                .iter()
                .find_map(|dir| make(Some(dir)).ok())
                .ok_or(first_err)
        })
        .expect("failed to generate xbps-sys bindings from <xbps.h>; if stddef.h is missing, set BINDGEN_EXTRA_CLANG_ARGS=-I<clang resource dir>/include");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("failed to write xbps-sys bindings.rs");

    for path in &lib.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    for l in &lib.libs {
        println!("cargo:rustc-link-lib={l}");
    }
}
