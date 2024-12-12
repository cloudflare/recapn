use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=schemas");

    let capnp_version = recapnc::CapnpCommand::version();
    if !capnp_version.starts_with("1.") && capnp_version != "(unknown)" {
        panic!("capnp tool 1.x is required, got: {capnp_version}");
    }

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

    // Test that we ignore missing nodes when generating
    run!("missing_annotation": "foo.capnp");

    recapnc::CapnpCommand::new()
        .src_prefix("schemas")
        .import_path("schemas")
        .file("schemas/test.capnp")
        .file("schemas/test-import.capnp")
        .file("schemas/test-import2.capnp")
        .write_to_out_dir();
}
