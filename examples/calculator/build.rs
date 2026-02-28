fn main() {
    println!("cargo::rerun-if-changed=capnp");
    recapnc::CapnpCommand::new()
        .src_prefix("schema")
        .file("schema/calculator.capnp")
        .write_to_out_dir();
}
