pub mod gen {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}

pub use gen::fuzz_capnp::*;