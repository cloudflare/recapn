use crate::client::Client;
use crate::sync::mpsc;
use crate::Error;
use std::future::Future;
use tokio::select;

use super::RpcClient;

async fn wait_and_resolve(
    future: impl Future<Output = Client>,
    mut receiver: mpsc::Receiver<RpcClient>,
) {
    select! {
        // if all the senders and requests drop, just cancel
        _ = receiver.closed() => {},
        client = future => {
            if let Err(r) = receiver.forward_to(&client.sender) {
                // Woops, it resolved into itself. Resolve it to an error instead
                r.close(Error::failed("attempted to resolve client into itself"))
            }
        },
    }
}

/// Spawns an asynchronous task and returns a new client that queues up calls until it resolves,
/// then forwards them to the new client. This client's `resolved()` and `more_resolved()`
/// functions will reflect the redirection with the eventual replacement client.
pub(super) fn spawn<F>(future: F) -> mpsc::Sender<RpcClient>
where
    F: Future<Output = Client> + Send + 'static,
{
    let (client, resolver) = mpsc::channel(RpcClient::Spawned);

    let _ = tokio::task::spawn(wait_and_resolve(future, resolver));

    client
}

/// Spawns a !Send future on the current LocalSet and returns a new client that queues up
/// calls until it resolves, then forwards them to the new client.
pub(super) fn spawn_local<F>(future: F) -> mpsc::Sender<RpcClient>
where
    F: Future<Output = Client> + 'static,
{
    let (client, resolver) = mpsc::channel(RpcClient::Spawned);

    let _ = tokio::task::spawn_local(wait_and_resolve(future, resolver));

    client
}
