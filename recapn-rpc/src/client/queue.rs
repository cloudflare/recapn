use crate::sync::{sharedshot, mpsc, TryRecvError};
use crate::{Error, PipelineOp};
use crate::client::{Client, Pipeline, RequestSender, RequestReceiver};
use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Weak};
use tokio::select;
use parking_lot::RwLock;

use super::{Broken, Unbroken, Queue, Local, ResolvedPipeline, RpcResponseReceiver};

pub(super) struct QueueClient {
    // TODO(someday): Perhaps inline these allocs into the client itself?
    pub redirect: sharedshot::Receiver<Client>,
    pub call_forwarding: RequestSender,
}

struct ClientResolver {
    redirect: sharedshot::Sender<Client>,
    call_forwarding: RequestReceiver,
}

impl ClientResolver {
    /// Resolves the queue and forwards all messages.
    pub async fn resolve(self, client: Client) {
        match client.kind {
            Unbroken(Queue(queue)) => {
                // Another queue! This is pretty simple, but we want to handle the case where we
                // can resolve the queue into itself. It would cause an easily preventable
                // infinite loop, so we'll provide a nice error message in that case instead.

                // First, try and forward calls.
                if let Err(calls) = self.call_forwarding.forward_to(&queue.call_forwarding) {
                    // Looks like the request points back at us.
                    // Resolve the redirect into an error.

                    let err = Error::failed("queued client resolved into itself");

                    let _ = self.redirect.send(Client::broken(err.clone()));
                    Self::break_all(calls, &err).await;

                    return;
                }

                // Calls forwarded successfully! Now let's resolve the redirect.
                let _ = self.redirect.send(Unbroken(Queue(queue)).into());
            }
            Broken(_) => {
                todo!()
            }
            Unbroken(Local(local)) => {
                todo!()
            }
        }
    }

    /// Resolves all requests received by the receiver into the given error
    async fn break_all(mut calls: RequestReceiver, error: &Error) {
        while let Some(req) = calls.recv().await {
            let _ = req.respond(Err(error.clone()));
        }
    }
}

fn client() -> (QueueClient, ClientResolver) {
    let (redirect_sender, redirect_receiver) = sharedshot::channel();
    let (forwarding_sender, forwarding_receiver) = mpsc::channel();

    let client = QueueClient {
        redirect: redirect_receiver,
        call_forwarding: forwarding_sender,
    };
    let resolver = ClientResolver {
        redirect: redirect_sender,
        call_forwarding: forwarding_receiver,
    };

    (client, resolver)
}

pub(super) fn pipeline() -> (QueuePipeline, PipelineResolver) {
    todo!()
}

async fn wait_and_resolve<F>(future: F, mut resolver: ClientResolver)
where
    F: Future<Output = Client>,
{
    select! {
        // if the redirect channel gets closed, we just exit early
        _ = resolver.redirect.closed() => {},
        client = future => resolver.resolve(client).await,
    }
}

/// Spawns an asynchronous task and returns a new client that queues up calls until it resolves,
/// then forwards them to the new client. This client's `resolved()` and `more_resolved()`
/// functions will reflect the redirection with the eventual replacement client.
pub(super) fn spawn<F>(future: F) -> Arc<QueueClient>
where
    F: Future<Output = Client> + Send + 'static,
{
    let (client, resolver) = client();

    let _ = tokio::task::spawn(wait_and_resolve(future, resolver));

    Arc::new(client)
}

/// Spawns a !Send future on the current LocalSet and returns a new client that queues up
/// calls until it resolves, then forwards them to the new client.
pub(super) fn spawn_local<F>(future: F) -> Arc<QueueClient>
where
    F: Future<Output = Client> + 'static,
{
    let (client, resolver) = client();

    let _ = tokio::task::spawn_local(wait_and_resolve(future, resolver));

    Arc::new(client)
}

pub(super) struct PipelineResolver {
    redirect: sharedshot::Sender<Pipeline>,
    pipeline: Weak<QueuePipeline>,
}

impl PipelineResolver {
    pub fn resolve(&self, pipeline: ResolvedPipeline) {
        todo!()
    }
}

pub struct QueuePipeline {
    /// A response receiver, used to keep the original request alive. If all response receivers are dropped,
    /// the request is canceled, and while a pipeline doesn't receive the response, it does have interest
    /// in it, but indirectly. This instance we hold signals that interest.
    pub(super) response: Option<RpcResponseReceiver>,
    redirect: sharedshot::Receiver<Pipeline>,
    client_map: RwLock<HashMap<Box<[PipelineOp]>, Arc<QueueClient>>>,
}

#[cold]
fn broken_pipeline() -> Client {
    Client::broken(Error::failed("pipeline failed to resolve"))
}

impl QueuePipeline {
    pub fn apply(&self, ops: Cow<[PipelineOp]>) -> Client {
        match self.redirect.try_recv() {
            // The redirect resolved, do the pipelining there directly.
            Ok(redirect) => redirect.apply(ops),
            // The redirect hasn't resolved yet. Queue up a task with a queue client.
            Err(TryRecvError::Empty) => {
                let read_map = self.client_map.read();
                if let Some(client) = read_map.get(ops.as_ref()) {
                    // We already have that client in the map, add a reference to it.
                    return Queue(client.clone()).into()
                }

                // We couldn't find it in the map, let's reacquire the lock.
                drop(read_map);

                let mut write_map = self.client_map.write();

                // Check if the redirect is done. If the redirect finished while we were waiting for
                // a write lock, the map will be empty, so we wouldn't want to add anything there.
                match self.redirect.try_recv() {
                    Ok(redirect) => return redirect.apply(ops),
                    Err(TryRecvError::Closed) => return broken_pipeline(),
                    Err(TryRecvError::Empty) => {},
                }

                let (_, client) = write_map.raw_entry_mut().from_key(ops.as_ref()).or_insert_with(|| {
                    let pipeline_receiver = self.redirect.clone();
                    let ops_clone = ops.clone().into_owned();
                    let client = spawn(async move {
                        let ops = Cow::Owned(ops_clone);
                        pipeline_receiver.recv()
                            .await
                            .map(|p| p.apply(ops))
                            .unwrap_or_else(|| Client::broken(Error::failed(
                                "pipeline failed to resolve")))
                    });
                    (Box::from(ops), client)
                });

                let client = client.clone();
                drop(write_map);

                Queue(client).into()
            },
            // The channel closed without resolving into a pipeline. This likely means
            // the task that was meant to return the future failed. Let's just return
            // an error client here.
            Err(TryRecvError::Closed) => broken_pipeline()
        }
    }
}
