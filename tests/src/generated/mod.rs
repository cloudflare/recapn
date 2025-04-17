#[path = "."]
pub mod capnp_test_capnp {
    #![allow(unused_imports)]
    use super::capnp_test_capnp as __file;
    mod __imports {}
    #[path = "capnp/test.capnp.rs"]
    mod capnp_test_capnp;
    pub use capnp_test_capnp::*;
}
#[path = "."]
pub mod capnp_test_import_capnp {
    #![allow(unused_imports)]
    use super::capnp_test_import_capnp as __file;
    mod __imports {
        pub use super::super::capnp_test_capnp;
    }
    #[path = "capnp/test-import.capnp.rs"]
    mod capnp_test_import_capnp;
    pub use capnp_test_import_capnp::*;
}
#[path = "."]
pub mod capnp_test_import2_capnp {
    #![allow(unused_imports)]
    use super::capnp_test_import2_capnp as __file;
    mod __imports {
        pub use super::super::capnp_test_capnp;
        pub use super::super::capnp_test_import_capnp;
    }
    #[path = "capnp/test-import2.capnp.rs"]
    mod capnp_test_import2_capnp;
    pub use capnp_test_import2_capnp::*;
}
