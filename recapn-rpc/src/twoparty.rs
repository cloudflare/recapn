use std::pin::pin;

use std::io::{self, IoSlice};

use recapn::io::StreamOptions;
use recapn::io::stream::StreamTable;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc::{self, Sender, UnboundedSender};

use crate::{Client, Connection, ConnectionOptions, LocalMessage, MessageFactory, MessageOutbound, OutboundMessage};

#[derive(Clone)]
pub struct CloseSignal(Sender<crate::Result<()>>);

impl CloseSignal {
    pub fn close(self, result: crate::Result<()>) {
        let _ = self.0.send(result);
    }
}

async fn write_message_async<W: AsyncWrite + Unpin>(
    mut w: W,
    outbound: OutboundMessage,
) -> Result<(), io::Error> {
    let stream_table;
    let mut io_slice_box = {
        if let Some(segments) = outbound.message.segments() {
            stream_table = recapn::io::stream::StreamTable::from_segments(&segments);
            let message_segment_bytes = segments.clone().into_iter().map(|s| s.as_bytes());
            std::iter::once(stream_table.as_bytes())
                .chain(message_segment_bytes)
                .map(IoSlice::new)
                .collect()
        } else {
            stream_table = StreamTable::new();
            Box::new([IoSlice::new(&stream_table.as_bytes())]) as Box::<[_]>
        }
    };

    // TODO(someday): This is literally a copy of write_all_vectored.
    // Use it when it becomes stable.
    let mut bufs = &mut *io_slice_box;

    IoSlice::advance_slices(&mut bufs, 0);
    while !bufs.is_empty() {
        match w.write_vectored(bufs).await {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write whole buffer",
                ));
            }
            Ok(n) => IoSlice::advance_slices(&mut bufs, n),
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Creates a simple client-server connection.
/// 
/// This spawns 2 tasks:
///  * A read task dedicated to driving the connection and reading incoming messages.
///  * A write task dedicated to writing outgoing messages.
pub fn connect<R, W>(
    read: R,
    write: W,
    bootstrap: Client,
    options: ConnectionOptions,
    stream_options: StreamOptions,
) -> (Client, CloseSignal)
where
    R: AsyncRead + Send + 'static,
    W: AsyncWrite + Send + 'static,
{
    struct Outbound {
        channel: UnboundedSender<OutboundMessage>,
    }

    impl MessageFactory for Outbound {
        fn new_message(&mut self) -> LocalMessage {
            Box::new(recapn::message::Message::global())
        }
        fn new_estimated(&mut self, len: recapn::alloc::AllocLen) -> LocalMessage {
            Box::new(recapn::message::Message::new(
                recapn::alloc::Growing::new(len, recapn::alloc::Global),
            ))
        }
    }

    impl MessageOutbound for Outbound {
        fn send(&mut self, msg: OutboundMessage) {
            let _ = self.channel.send(msg);
        }
    }

    let (close_send, close_recv) = mpsc::channel(1);
    let close_signal = CloseSignal(close_send);

    let (out_send, out_recv) = mpsc::unbounded_channel();
    let write_close_signal = close_signal.clone();
    tokio::spawn(async move {
        let close_signal = write_close_signal;
        let mut write = pin!(write);
        let mut out_recv = out_recv;
        while let Some(next) = out_recv.recv().await {
            if let Err(err) = write_message_async(&mut write, next).await {
                close_signal.close(Err(crate::Error::disconnected(format!("write error: {err}"))));
                return
            }
        }
    });
    let outbound = Outbound {
        channel: out_send,
    };
    let mut connection = Connection::new(outbound, bootstrap, options);
    let bootstrap = connection.bootstrap();

    tokio::spawn(async move {
        let read = pin!(read);
        let mut message_stream = crate::io::stream::ReadMessageBuf::new(read, stream_options);

        let mut connection = connection;
        let mut close_recv = close_recv;

        loop {
            tokio::select! {
                Some(res) = close_recv.recv() => {
                    if let Err(err) = res {
                        let _ = connection.close(err);
                    }
                }
                next = message_stream.read() => {
                    match next {
                        Ok(Some(msg)) => {
                            if connection.handle_message(msg).is_ok() {
                                continue
                            }
                        }
                        Ok(None) => {},
                        Err(err) => {
                            let _ = connection.close(
                                crate::Error::disconnected(format!("read error: {err}"))
                            );
                        }
                    }
                }
                res = connection.handle_event() => {
                    if res.is_ok() {
                        continue
                    }
                }
            }
            break
        }
    });

    (bootstrap, close_signal)
}