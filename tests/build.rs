fn main() {
    println!("cargo::rerun-if-changed=schemas");

    recapnc::CapnpCommand::new()
        .src_prefix("schemas")
        .file("schemas/build.capnp")
        .file("schemas/bar.capnp")
        .write_to_out_dir();
}
