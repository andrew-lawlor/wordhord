//! On a Kobo build, compiles FBInk (vendor/FBInk) and our shim over it.
//! On the desktop there's nothing to build: the reader only runs on the
//! device, and its logic is tested without the screen.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/fb.c");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("arm") {
        return;
    }
    let fbink = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/FBInk");
    if !fbink.join("fbink.c").exists() {
        panic!("vendor/FBInk is missing: git submodule update --init --recursive");
    }
    cc::Build::new()
        .file(fbink.join("fbink.c"))
        .file(fbink.join("i2c-tools/lib/smbus.c"))
        .file(fbink.join("qimagescale/qimagescale.c"))
        .file("src/fb.c")
        .include(&fbink)
        .include(fbink.join("i2c-tools/include"))
        .define("_GNU_SOURCE", None)
        .define("FBINK_FOR_KOBO", None)
        .define("FBINK_MINIMAL", None)
        .define("FBINK_WITH_DRAW", None)
        .define("FBINK_WITH_IMAGE", None)
        .define("FBINK_WITH_INPUT", None)
        .flag("-std=gnu11")
        .warnings(false)
        .compile("fbink");
}
