use crate::gen::capnp_test_capnp::{TestAllTypes, TestEnum};
use recapn::message::Message;
use recapn::ty::AsReader;

#[test]
fn test_lender() {
    // Lender primarily exists for copying from native types to Cap'n Proto types. Lists are
    // especially painful to deal with because it requires using enumerator() and that sucks.
    // Lender gives a nicer API that, while it doesn't support `for` syntax, does support
    // `while let` which is the next best thing.

    struct Data {
        value: i8,
        big_value: u64,
        enum_value: TestEnum,
    }

    let some_data = vec![
        Data { value: 5, big_value: 0, enum_value: TestEnum::Qux },
        Data { value: -1, big_value: 0xFFFFFFFFFFFFF, enum_value: TestEnum::Bar },
    ];

    let mut message = Message::global();
    let mut builder = message.builder().init_struct_root::<TestAllTypes>();
    let mut list = builder.struct_list()
        .init(some_data.len() as u32)
        .into_lender()
        .zip(&some_data);
    while let Some((builder, data)) = list.next() {
        let mut e = builder.get();
        e.int8_field().set(data.value);
        e.u_int64_field().set(data.big_value);
        e.enum_field().set(data.enum_value);
    }

    let list = builder.as_reader().struct_list().get();
    assert_eq!(list.len(), some_data.len() as u32);

    for (e, data) in list.into_iter().zip(&some_data) {
        assert_eq!(e.int8_field(), data.value);
        assert_eq!(e.u_int64_field(), data.big_value);
        assert_eq!(e.enum_field().unwrap(), data.enum_value);
    }

    // Check to make sure other types of ranges work.
    let mut list = builder.bool_list().init(6).into_lender_range(2..=3);
    while let Some(mut b) = list.next() {
        b.set(true);
    }

    let list = builder.as_reader().bool_list().get();
    assert_eq!(list.len(), 6);
    assert!(list.into_iter().eq([false, false, true, true, false, false].into_iter()));

    // Going into ranges out of bounds yields no elements.
    let mut list = builder.u_int8_list().init(3).into_lender_range(4..);
    assert!(list.next().is_none());
}