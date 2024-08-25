fn main() {
    println!("cargo::rerun-if-changed=capnp");
    recapnc::CapnpCommand::new()
        .src_prefix("capnp")
        .file("capnp/addressbook.capnp")
        .write_to_out_dir();
}
