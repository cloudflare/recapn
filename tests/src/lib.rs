#![cfg(test)]

#[rustfmt::skip]
pub mod generated;

#[rustfmt::skip]
pub mod ports;

pub mod build_gen {
    include!(concat!(env!("OUT_DIR"), "/build_rs/mod.rs"));
}

use std::time::Instant;

use generated::capnp_test_capnp::{TestAllTypes, TestEnum};
use recapn::message::Message;
use recapn::{text, ty};
use recapn_port::{DeserializeAnyPtrExt, DeserializePtr, SerializeAnyPtrExt, SerializePtr};
use recapn_rpc::client::{Client, Request};
use recapn_rpc::server::{
    CallContext, CallResult, Dispatch, DispatchRequest, DispatchResponse, Dispatcher, FromServer,
};
use recapn_rpc::Error;

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
fn make_port() {
    let src_value = ports::capnp_test_capnp::TestAllTypes {
        bool_field: true,
        int8_field: 7,
        int16_field: 15,
        int32_field: 31,
        int64_field: 63,
        u_int8_field: 8,
        u_int16_field: 16,
        u_int32_field: 32,
        u_int64_field: 64,
        float32_field: -32.32,
        float64_field: -64.64,
        text_field: Some(recapn_port::text::Text::new("Hello world!")),
        data_field: Some(
            recapn_port::data::Data::new(Box::from(b"Hello bytes!".as_slice())).unwrap()
        ),
        struct_field: Some(Box::new(ports::capnp_test_capnp::TestAllTypes {
            float32_field: 32.32,
            float64_field: 64.64,
            ..Default::default()
        })),
        enum_field: recapn_port::Enum::from_type(ports::capnp_test_capnp::TestEnum::Bar),
        ..Default::default()
    };
    let mut message = Message::global();
    message.builder().into_root().serialize::<_, Box<dyn core::error::Error>>(&src_value).unwrap();
    let dst_value: ports::capnp_test_capnp::TestAllTypes =
        message.reader().root().deserialize::<_, Box<dyn core::error::Error>>().unwrap();

    assert_eq!(src_value.bool_field, dst_value.bool_field);
    assert_eq!(src_value.int8_field, dst_value.int8_field);
    assert_eq!(src_value.int16_field, dst_value.int16_field);
    assert_eq!(src_value.int32_field, dst_value.int32_field);
    assert_eq!(src_value.int64_field, dst_value.int64_field);
    assert_eq!(src_value.u_int32_field, dst_value.u_int32_field);
    assert_eq!(src_value.float64_field, dst_value.float64_field);
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

pub trait FooServer {
    fn bar(&mut self, ctx: CallContext<TestAllTypes, TestAllTypes>) -> CallResult {
        ctx.response.error(Error::unimplemented(
            "'Foo.bar' is not implemented for this type",
        ))
    }
}

pub struct FooDispatcher<T>(pub T);

impl<T> FooDispatcher<T> {
    pub const NAME: &str = "Foo";
}

impl<T> Dispatch for FooDispatcher<T>
where
    T: FooServer,
{
    fn dispatch(&mut self, request: DispatchRequest) -> DispatchResponse {
        match (request.interface(), request.method()) {
            (0, 0) => self.0.bar(request.into_call_context()).into(),
            (0, _) => request.unimplemented_method(Self::NAME),
            (_, _) => request.unimplemented_interface(Self::NAME),
        }
    }
}

pub struct FooClient(Client);

impl ty::Capability for FooClient {
    type Client = Client;

    fn from_client(c: Self::Client) -> Self {
        Self(c)
    }
    fn into_inner(self) -> Self::Client {
        self.0
    }
}

impl<T: FooServer> FromServer<T> for FooClient {
    type Dispatcher = FooDispatcher<T>;

    #[inline]
    fn from_server(server: T) -> (Self, Dispatcher<Self::Dispatcher>) {
        let (client, dispatcher) = recapn_rpc::server::new_server(FooDispatcher(server));
        (Self(client), dispatcher)
    }
}

impl FooClient {
    pub fn bar(&self) -> Request<TestAllTypes, TestAllTypes> {
        self.0.call(0, 0)
    }
}

pub struct FooImpl;
impl FooServer for FooImpl {
    fn bar(&mut self, ctx: CallContext<TestAllTypes, TestAllTypes>) -> CallResult {
        ctx.response.respond_with(|r| {
            let params = ctx.params.reader();
            let mut results = r.results();
            results.float32_field().set(params.get().float32_field());
            Ok(())
        })
    }
}

#[tokio::test]
async fn make_client() -> recapn_rpc::Result<()> {
    let (client, mut dispatcher) = FooClient::from_server(FooImpl);
    tokio::spawn(async move {
        dispatcher.run().await.unwrap();
    });
    let before = Instant::now();
    let mut request = client.bar();
    request.params().float32_field().set(0.123);
    let response = request.send()?.await;
    dbg!(Instant::now().duration_since(before));
    let results = response.results()?;
    assert_eq!(results.get().float32_field(), 0.123);

    Ok(())
}
