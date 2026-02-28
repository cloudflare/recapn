use assert_matches2::assert_matches;
use hashbrown::HashMap;
use tokio_test::{assert_pending, assert_ready};
use tokio_test::task::spawn;

use crate::request::{ResponseReceiver, TryRecvError};
use crate::request::{
    request_response, request_response_pipeline, RequestUsage, ResponseReceiverFactory,
};
use crate::{mpsc, Chan, IntoResults, PipelineResolver};

/// Assert that an instance of Spawn hasn't been woken and is pending.
macro_rules! assert_spawn_pending {
    ($expr:expr) => {
        assert!(!($expr).is_woken());
        assert_pending!(($expr).poll());
    };
}

/// Assert that an instance of Spawn has been woken and is ready, returning the ready value.
macro_rules! assert_spawn_ready {
    ($expr:expr) => {{
        assert!(($expr).is_woken());
        assert_ready!(($expr).poll())
    }};
}

macro_rules! assert_unresolved {
    ($expr:expr) => {
        match ($expr).try_resolved() {
            Some(resolved) =>
                panic!("assertion failed: channel was already resolved with \"{:?}\"", resolved),
            None => {}
        }
    };
}

macro_rules! assert_resolved {
    ($expr:expr) => {{
        match ($expr).try_resolved() {
            Some(resolved) => resolved,
            None => panic!("assertion failed: channel was unresolved")
        }
    }};
}

#[derive(Clone, Debug)]
struct Error(#[allow(dead_code)] &'static str);

#[derive(Debug)]
struct TestChannel;

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
struct IntRequest(i32);

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
struct IntEvent(i32);

#[derive(Clone, Debug)]
struct IntsResponse(HashMap<i32, mpsc::Sender<TestChannel>>);

#[derive(PartialEq, Eq, Hash, Debug)]
struct IntPipelineKey(i32);

impl IntoResults<TestChannel> for Error {
    #[inline]
    fn into_results(self) -> <TestChannel as Chan>::Results {
        Err(self)
    }
}

impl Chan for TestChannel {
    type Event = IntEvent;
    type Parameters = IntRequest;
    type Error = Error;
    type Results = Result<IntsResponse, Error>;
    type PipelineKey = IntPipelineKey;
    type Pipeline = IntsResponse;
}

impl PipelineResolver<TestChannel> for IntsResponse {
    fn resolve(
        &self,
        _: ResponseReceiver<TestChannel>,
        key: IntPipelineKey,
        channel: mpsc::Receiver<TestChannel>,
    ) {
        let Some(results) = self.0.get(&key.0) else {
            channel.close(Error("unknown pipeline"));
            return;
        };

        if let Err(c) = channel.forward_to(results) {
            // If it's an infinite loop, close it with an error
            c.close(Error("infinite loop"));
        }
    }

    fn pipeline(
        &self,
        _: ResponseReceiverFactory<'_, TestChannel>,
        key: IntPipelineKey,
    ) -> mpsc::Sender<TestChannel> {
        match self.0.get(&key.0) {
            Some(c) => c.clone(),
            None => mpsc::broken(TestChannel, Error("unknown pipeline")),
        }
    }
}

impl PipelineResolver<TestChannel> for Result<IntsResponse, Error> {
    fn resolve(
        &self,
        recv: ResponseReceiver<TestChannel>,
        key: IntPipelineKey,
        channel: mpsc::Receiver<TestChannel>,
    ) {
        match self {
            Ok(r) => r.resolve(recv, key, channel),
            Err(err) => channel.close(err.clone()),
        }
    }

    fn pipeline(
        &self,
        _: ResponseReceiverFactory<'_, TestChannel>,
        key: IntPipelineKey,
    ) -> mpsc::Sender<TestChannel> {
        match self {
            Ok(r) => match r.0.get(&key.0) {
                Some(c) => c.clone(),
                None => mpsc::broken(TestChannel, Error("unknown pipeline")),
            },
            Err(err) => mpsc::broken(TestChannel, err.clone()),
        }
    }
}

/// Make a request and respond, then receive the result and check it.
#[test]
fn sync_request_response() {
    let input = IntRequest(1);
    let (req, resp) = request_response::<TestChannel>(input);
    assert_eq!(req.usage(), RequestUsage::Response);

    let (params, responder) = req.respond();
    assert_eq!(input, params);

    assert!(!responder.is_finished());
    let responder_results = responder.respond(Ok(IntsResponse(HashMap::new())));
    let responder_response = responder_results.get().as_ref().unwrap();

    let results = resp.try_recv().unwrap();
    let response = results.get().as_ref().unwrap();

    assert!(response.0.is_empty());
    assert!(std::ptr::eq(response, responder_response));
}

/// Make a request, but drop the request, and make sure the receiver becomes aware of it.
#[test]
fn sync_close_request() {
    let input = IntRequest(1);
    let (req, resp) = request_response::<TestChannel>(input);
    drop(req);

    assert_matches!(resp.try_recv(), Err(TryRecvError::Closed));
}

/// Make a request, but drop all receivers of the request. This will mark the request as finished.
#[test]
fn sync_finished() {
    let input = IntRequest(1);
    let (req, resp, pipeline) = request_response_pipeline::<TestChannel>(input);

    assert!(!req.is_finished());
    drop(resp);

    assert!(!req.is_finished());
    let pipeline_clone = pipeline.clone();

    assert!(!req.is_finished());
    drop(pipeline);

    assert!(!req.is_finished());
    drop(pipeline_clone);

    assert!(req.is_finished());
}

/// Make a request with a separate Finished future.
#[test]
fn finished_after_response() {
    let (pipeline_channel, recv) = mpsc::channel(TestChannel);

    let input = IntRequest(5);
    let (req, resp, pipeline) = request_response_pipeline::<TestChannel>(input);
    let mut resp_spawn = spawn(resp.clone().recv());
    assert_pending!(resp_spawn.poll());

    let mut finished = spawn(req.finished());
    assert_pending!(finished.poll());

    let channel = pipeline.build(IntPipelineKey(3), || TestChannel);
    let mut resolved = spawn(channel.resolution());
    assert_pending!(resolved.poll());

    let (_, responder) = req.respond();
    responder.respond(Ok(IntsResponse(HashMap::from([(3, pipeline_channel)]))));

    let resolved = assert_spawn_ready!(resolved).forwarded().unwrap();

    // Even though we've responded at this point, there's still active receivers for the response.
    // Pipelines could still be made at this point, and response receivers could still try to access
    // the response. We won't wake up until those are dropped.
    assert_spawn_pending!(finished);

    let result = resp.try_recv().unwrap();
    drop(resp);

    assert!(result.is_ok());

    // Recv futures are considered receivers and have to be droppped as well.
    assert_spawn_pending!(finished);

    let task_result = assert_spawn_ready!(resp_spawn).unwrap();
    drop(resp_spawn);

    assert!(task_result.is_ok());

    // We still have a pipeline builder, so finished should still be pending.
    assert_spawn_pending!(finished);

    drop(pipeline);

    // Now we should be finished.
    assert_spawn_ready!(finished);

    // After everything is finished, our pipeline channel should still be resolved to the
    // destination channel which is currently unresolved.
    assert_unresolved!(resolved);
    drop(recv);

    // When we drop it, it should resolve into "dropped".
    let resolution = assert_resolved!(resolved);
    assert!(resolution.is_dropped());
}

/// Make a request with a separate Finished future and make sure it's declared finished when all
/// receivers are dropped.
#[test]
fn finished_after_drop() {
    let input = IntRequest(5);
    let (req, resp, pipeline) = request_response_pipeline::<TestChannel>(input);
    let mut resp_spawn = spawn(resp.clone().recv());
    assert_pending!(resp_spawn.poll());

    let mut finished = spawn(req.finished());
    assert_pending!(finished.poll());

    let channel = pipeline.build(IntPipelineKey(3), || TestChannel);
    let mut resolved = spawn(channel.resolution());
    assert_pending!(resolved.poll());

    drop(req);
    // At this point, we will never receive a response, so all pipelines are dropped, and all
    // response receivers receive the result "closed".

    // Our pipeline channel resolution should've resolved as the channel gets dropped.
    let resolution = assert_spawn_ready!(resolved);
    assert!(resolution.is_dropped());

    // Like with the response case, there's still active receivers for the response.
    // Pipelines could still be made at this point (though they'd be immediately dropped) and 
    // response receivers could still try to access the response. We won't wake up until those are
    // dropped.
    assert_spawn_pending!(finished);

    let result = resp.try_recv();
    drop(resp);

    assert_matches!(result, Err(TryRecvError::Closed));

    // Recv futures are considered receivers and have to be droppped as well.
    assert_spawn_pending!(finished);

    let task_result = assert_spawn_ready!(resp_spawn);
    drop(resp_spawn);

    assert!(task_result.is_none());

    // We still have a pipeline builder, so finished should still be pending.
    assert_spawn_pending!(finished);

    drop(pipeline);

    // Now, we should be finished.
    assert_spawn_ready!(finished);
}
