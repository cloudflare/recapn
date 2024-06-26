mod gen;

use recapn::message::Message;
use gen::capnp_test_capnp::TestAllTypes;

#[test]
fn make_all_types() {
    let mut message = Message::global();
    let mut builder = message.builder().init_struct_root::<TestAllTypes>();
    builder.bool_field().set(true);
    builder.data_field().set_slice(&[0, 1, 2, 3]);
}