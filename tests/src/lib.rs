#![cfg(test)]

#[rustfmt::skip]
pub mod gen;

pub mod build_gen {
    include!(concat!(env!("OUT_DIR"), "/build_rs/mod.rs"));
}

use gen::capnp_test_capnp::{TestAllTypes, TestEnum};
use recapn::message::Message;
use recapn::text;

#[test]
fn make_all_types() {
    let mut message = Message::global();
    let mut builder = message.builder().init_struct_root::<TestAllTypes>();
    builder.bool_field().set(true);
    builder.int8_field().set(7);
    builder.int16_field().set(15);
    builder.int32_field().set(31);
    builder.int64_field().set(63);
    builder.u_int8_field().set(8);
    builder.u_int16_field().set(16);
    builder.u_int32_field().set(32);
    builder.u_int64_field().set(64);
    builder.float32_field().set(-32.32);
    builder.float64_field().set(-64.64);
    builder.text_field().set(text!("Hello world!"));
    builder.data_field().set_slice(b"Hello bytes!");
    let mut inner = builder.struct_field().init();
    inner.float32_field().set(32.32);
    inner.float64_field().set(64.64);
    builder.enum_field().set(TestEnum::Bar);
}

#[test]
fn make_build_gen_type() {
    let mut message = Message::global();
    let mut builder = message.builder();
    let mut foo = builder
        .by_ref()
        .init_struct_root::<build_gen::build_capnp::Foo>();
    let mut bar = foo.bar().init();
    bar.flub().set(1234);

    assert_eq!(builder.segments().first().as_words().len(), 3);
}
