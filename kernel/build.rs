//! Passes the linker script to the kernel binary (and only to it).

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let script = format!("{manifest_dir}/linker-{arch}.ld");
    println!("cargo:rustc-link-arg-bins=-T{script}");
    println!("cargo:rustc-link-arg-bins=-pie");
    println!("cargo:rustc-link-arg-bins=--no-dynamic-linker");
    println!("cargo:rerun-if-changed={script}");
}
