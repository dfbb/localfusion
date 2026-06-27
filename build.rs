// build.rs — re-export CARGO_PKG_VERSION as APP_VERSION so main.rs can print it at startup.
// Cargo sets CARGO_PKG_VERSION automatically from Cargo.toml [package] version.
fn main() {
    println!(
        "cargo:rustc-env=APP_VERSION={}",
        env!("CARGO_PKG_VERSION")
    );
    println!("cargo:rerun-if-changed=Cargo.toml");
}
