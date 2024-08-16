use std::sync::Arc;

use recapn::any::AnyPtr;
use recapn::rpc;

use crate::chan::{PipelineBuilder, RpcChannel};
use crate::client::Client;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PipelineOp {
    // We don't include PipelineOp::None because it's pointless and a waste of space.
    PtrField(u16),
}

#[derive(Clone)]
pub struct Pipeline {
    builder: PipelineBuilder,
    ops: Vec<PipelineOp>,
}

impl Pipeline {
    pub(crate) fn new(builder: PipelineBuilder) -> Self {
        Self {
            builder,
            ops: Vec::new(),
        }
    }
}

impl rpc::CapSystem for Pipeline {
    type Cap = Client;
}

impl rpc::Pipelined for Pipeline {
    fn into_cap(self) -> Self::Cap {
        let ops = Arc::from(self.ops);
        Client(self.builder.build(ops, || RpcChannel::Pipeline))
    }
    fn to_cap(&self) -> Self::Cap {
        let ops = Arc::from(self.ops.as_slice());
        Client(self.builder.build(ops, || RpcChannel::Pipeline))
    }
}

impl rpc::PipelineBuilder<AnyPtr> for Pipeline {
    type Operation = PipelineOp;

    fn push(mut self, op: Self::Operation) -> Self {
        self.ops.push(op);
        self
    }
}

pub type PipelineOf<R> = <R as rpc::Pipelinable>::Pipeline<Pipeline>;
