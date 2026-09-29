// The CLI runs on an 8 MiB main-thread stack, and wasm-ld reserves 1 MiB by
// default. A deeply nested document that the CLI parses would overflow the
// module's stack, so the module reserves what the CLI has.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        println!("cargo::rustc-link-arg-cdylib=-zstack-size=8388608");
    }
}
