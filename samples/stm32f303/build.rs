//! embassy-stm32's `memory-x` feature provides `memory.x` for the chip, so this only adds the
//! linker scripts.

fn main() {
    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
}
