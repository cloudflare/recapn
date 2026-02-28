mod generated {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}

pub use generated::calculator_capnp::calculator::*;

pub async fn read_value(value: &Value) -> recapn_rpc::Result<f64> {
    Ok(value.read().send()?.await.results()?.get().value())
}

pub fn bad_params_err() -> recapn_rpc::Error {
    recapn_rpc::Error::failed("Wrong number of parameters.")
}

pub fn spawn_server<T, S>(server: S) -> T
where
    T: recapn_rpc::server::FromServer<S>,
    T::Dispatcher: recapn_rpc::server::Dispatch + Send + 'static,
{
    let (client, server) = T::from_server(server);
    tokio::spawn(async move {
        let mut server = server;
        server.run().await
    });
    client
}
