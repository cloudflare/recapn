fn main() {
    recapnc::CapnpCommand::new()
        .src_prefix("schemas")
        .file("schemas/rpc.capnp")
        .write_to_out_dir();
}
