use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");

    let target = env::var("TARGET").expect("Cargo build scripts always have TARGET");

    let mut bindings = bindgen::Builder::default()
        .trust_clang_mangling(false)
        .clang_arg("-target")
        .clang_arg(target);

    if let Ok(sysroot) = env::var("SYSROOT") {
        bindings = bindings.clang_arg("--sysroot").clang_arg(sysroot);
    }

    let bindings = bindings
        .header("build/wrapper.h")
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
