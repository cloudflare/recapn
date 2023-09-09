//! Implements an RPC connection between two parties. This API has been heavily simplified for
//! ease of use and should "just work™".

use std::marker::PhantomData;

use crate::system::{Provision, VatNetwork, VatConnection, VatSystem, OutgoingChannel};
use crate::{Result, Error};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Server,
    Client,
}

pub struct TwoPartyRpcSystem<T: OutgoingChannel> {
    system: VatSystem<Network<T>>,
    connection: VatConnection<Network<T>>,
}

impl<T: OutgoingChannel> TwoPartyRpcSystem<T> {
    #[inline]
    pub fn new(out: T, their_side: Side) -> Self {
        let system = VatSystem::new(Network { t: PhantomData });
        let connection = system.new_connection(Connection { their_side, conn: out });
        Self { system, connection }
    }

    pub fn recv<M>(&mut self, msg: M) -> Result<()> {
        todo!()
    }

    #[inline]
    pub fn their_side(&self) -> Side {
        self.connection.conn().their_side
    }

    #[inline]
    pub fn our_side(&self) -> Side {
        match self.their_side() {
            Side::Client => Side::Server,
            Side::Server => Side::Client,
        }
    }
}

struct Connection<T> {
    their_side: Side,
    conn: T,
}

impl<T: OutgoingChannel> OutgoingChannel for Connection<T> {
    type Message = T::Message;

    #[inline]
    fn new_message(&self) -> Self::Message {
        self.conn.new_message()
    }

    #[inline]
    fn is_closed(&self) -> bool {
        self.conn.is_closed()
    }
    #[inline]
    fn close(self, err: Option<Error>) {
        self.conn.close(err)
    }
}

struct Network<T> {
    t: PhantomData<T>,
}

fn third_party_handoff_unimplemented() -> Error {
    Error::unimplemented("Third-party handoffs are not supported in a two-party network")
}

impl<T: OutgoingChannel> VatNetwork for Network<T> {
    type Connection = Connection<T>;

    type ProvisionId = ();
    type RecipientId = ();
    type ThirdPartyCapId = ();

    fn new_provision(&mut self, _: &VatConnection<Self>, _: &VatConnection<Self>)
            -> Result<(Self::RecipientId, Self::ThirdPartyCapId)> {
        Err(third_party_handoff_unimplemented())
    }
    fn resolve_third_party_cap(&mut self, _: &Self::ThirdPartyCapId)
            -> Result<(VatConnection<Self>, Self::ProvisionId)> {
        Err(third_party_handoff_unimplemented())
    }
    fn receive_provision(&mut self, _: &Self::RecipientId) -> Result<Provision> {
        Err(third_party_handoff_unimplemented())
    }
    fn resolve_provision(&mut self, _: &Self::ProvisionId) -> Result<Provision> {
        Err(third_party_handoff_unimplemented())
    }
    fn cancel_provision(&mut self, _: &Provision) {}
}