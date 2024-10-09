use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=schemas");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    macro_rules! run {
        ($base:literal: $($path:literal),*) => {
            recapnc::CapnpCommand::new()
                .src_prefix(concat!("schemas/", $base))
                $(.file(concat!("schemas/", $base, "/", $path)))*
                .write_to(out_dir.join($base))
        };
    }

    // Simple test with imports
    run!("build_rs": "build.capnp", "bar.capnp");
}
