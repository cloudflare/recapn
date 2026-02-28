struct ChannelOutbound {
    outgoing: UnboundedSender<recapn_rpc::OutboundMessage>,
}

impl recapn_rpc::MessageFactory for ChannelOutbound {
    fn new_message(&mut self) -> recapn_rpc::LocalMessage {
        Box::new(recapn::message::Message::global())
    }
}

impl recapn_rpc::MessageOutbound for ChannelOutbound {
    fn send(&mut self, msg: recapn_rpc::OutboundMessage) {
        let _ = self.outgoing.send(msg);
    }
}

struct ChannelConnection {
    incoming: UnboundedReceiver<recapn_rpc::OutboundMessage>,
    connection: Connection<ChannelOutbound>,
}

impl ChannelConnection {
    pub fn new(
        incoming: UnboundedReceiver<recapn_rpc::OutboundMessage>,
        outgoing: UnboundedSender<recapn_rpc::OutboundMessage>,
        bootstrap: Client,
    ) -> Self {
        Self {
            incoming,
            connection: Connection::new(
                ChannelOutbound { outgoing },
                bootstrap,
                ConnectionOptions::default(),
            ),
        }
    }

    pub fn bootstrap(&mut self) -> Client {
        self.connection.bootstrap()
    }

    pub async fn run(mut self) -> recapn_rpc::Result<()> {
        loop {
            tokio::select! {
                next = self.incoming.recv() => {
                    let Some(next) = next else {
                        return Ok(())
                    };

                    self.connection.handle_message(next)?;
                }
                Err(err) = self.connection.handle_event() => {
                    return Err(err)
                }
            }
        }
    }
}

struct ClientServerConnections {
    client: ChannelConnection,
    server: ChannelConnection,
}

impl ClientServerConnections {
    pub fn new(client_bootstrap: Client, server_bootstrap: Client) -> Self {
        let (outgoing_server, incoming_server) = tokio::sync::mpsc::unbounded_channel();
        let (outgoing_client, incoming_client) = tokio::sync::mpsc::unbounded_channel();

        Self {
            client: ChannelConnection::new(incoming_client, outgoing_server, client_bootstrap),
            server: ChannelConnection::new(incoming_server, outgoing_client, server_bootstrap),
        }
    }
}