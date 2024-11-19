use assert_matches2::assert_matches;
use hashbrown::HashMap;
use tokio_test::assert_ready;
use tokio_test::task::spawn;

use crate::request::TryRecvError;
use crate::{mpsc, Chan, IntoResults, PipelineResolver};
use crate::request::{
    request_response, request_response_pipeline, RequestUsage, ResponseReceiverFactory
};

#[derive(Clone, Debug)]
struct Error(#[allow(dead_code)] &'static str);

#[derive(Debug)]
struct TestChannel;

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
struct IntRequest(i32);

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
    type Parameters = IntRequest;
    type Error = Error;
    type Results = Result<IntsResponse, Error>;
    type PipelineKey = IntPipelineKey;
    type Pipeline = IntsResponse;
}

impl PipelineResolver<TestChannel> for IntsResponse {
    fn resolve(
        &self,
        _: ResponseReceiverFactory<'_, TestChannel>,
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
        recv: ResponseReceiverFactory<'_, TestChannel>,
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

#[test]
fn sync_request_response() {
    // Make a request and respond, then receive the result and check it.

    let input = IntRequest(1);
    let (req, resp) = request_response::<TestChannel>(input);
    assert_eq!(req.usage(), RequestUsage::Response);

    let (params, responder) = req.respond();
    assert_eq!(input, params);

    assert!(!responder.is_finished());
    responder.respond(Ok(IntsResponse(HashMap::new())));

    let results = resp.try_recv().unwrap();
    let response = (*results).as_ref().unwrap();

    assert!(response.0.is_empty());
}

#[test]
fn sync_close_request() {
    // Make a request, but drop the request, and make sure the receiver becomes aware of it.

    let input = IntRequest(1);
    let (req, resp) = request_response::<TestChannel>(input);
    drop(req);

    assert_matches!(resp.try_recv(), Err(TryRecvError::Closed));
}

#[test]
fn sync_finished() {
    // Make a request, but drop all receivers of the request. This will mark the request as finished.

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

#[test]
fn finished_after_response() {
    // Make a request with a separate Finished future.

    let (pipeline_channel, recv) = mpsc::channel(TestChannel);

    let input = IntRequest(5);
    let (req, resp, pipeline) = request_response_pipeline::<TestChannel>(input);

    let mut finished = spawn(req.finished());
    assert!(finished.poll().is_pending());

    let channel = pipeline.build(IntPipelineKey(3), || TestChannel);
    let mut resolved = spawn(channel.resolution());
    assert!(resolved.poll().is_pending());

    let (_, responder) = req.respond();
    responder.respond(Ok(IntsResponse(HashMap::from([
        (3, pipeline_channel)
    ]))));

    assert!(resolved.is_woken());

    let resolved = assert_ready!(resolved.poll()).forwarded().unwrap();

    // Even though we've responded at this point, there's still active receivers for the response.
    // Pipelines could still be made at this point, and response receivers could still try to access
    // the response. We won't wake up until those are dropped.

    assert!(!finished.is_woken());
    assert!(finished.poll().is_pending());

    let result = resp.try_recv().unwrap();
    drop(resp);

    assert!(result.is_ok());

    // We still have a pipeline builder, so finished should still be pending.

    assert!(!finished.is_woken());
    assert!(finished.poll().is_pending());

    drop(pipeline);

    // Now, we should be finished.

    assert!(finished.is_woken());
    assert!(finished.poll().is_ready());

    assert!(!resolved.is_resolved());
    drop(recv);

    assert!(resolved.try_resolved().unwrap().is_dropped());
}