use std::future::poll_fn;
use std::pin::pin;

use std::io::{self, IoSlice};
use std::sync::Arc;

use recapn::io::StreamOptions;
use recapn::io::stream::StreamTable;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::SetOnce;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::JoinSet;

use crate::{Client, Connection, ConnectionOptions, LocalMessage, MessageFactory, MessageOutbound, OutboundMessage};

#[derive(Clone)]
pub struct CloseSignal(Arc<SetOnce<crate::Error>>);

impl CloseSignal {
    pub fn close(self, result: crate::Error) {
        let _ = self.0.set(result);
    }
    pub async fn get(&self) -> &crate::Error {
        self.0.wait().await
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

struct SharedConnection(parking_lot::Mutex<Connection<Outbound>>);

impl SharedConnection {
    async fn poll_tasks(&self) -> crate::Error {
        poll_fn(|cx| self.0.lock().poll_tasks(cx)).await
    }
    async fn poll_events(&self) -> crate::Error {
        poll_fn(|cx| self.0.lock().poll_events(cx)).await
    }
    async fn poll_channels(&self) -> crate::Error {
        poll_fn(|cx| self.0.lock().poll_channels(cx)).await
    }
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
    let mut tasks = JoinSet::new();
    let close_signal = CloseSignal(Arc::new(SetOnce::new()));

    let (out_send, out_recv) = mpsc::unbounded_channel();
    let write_task = tokio::spawn(async move {
        let mut write = pin!(write);
        let mut out_recv = out_recv;
        while let Some(next) = out_recv.recv().await {
            if let Err(err) = write_message_async(&mut write, next).await {
                return Err(crate::Error::disconnected(format!("write error: {err}")))
            }
        }

        Ok(())
    });
    let outbound = Outbound {
        channel: out_send,
    };

    let mut connection = Connection::new(outbound, bootstrap, options);
    let bootstrap = connection.bootstrap();

    let connection = std::sync::Arc::new(SharedConnection(parking_lot::Mutex::new(connection)));
    tasks.spawn({
        let connection = connection.clone();
        async move {
            dbg!(connection.poll_channels().await)
        }
    });
    tasks.spawn({
        let connection = connection.clone();
        async move {
            dbg!(connection.poll_events().await)
        }
    });
    tasks.spawn({
        let connection = connection.clone();
        async move {
            dbg!(connection.poll_tasks().await)
        }
    });

    let read_task = tokio::spawn({
        let connection = connection.clone();
        async move {
            let read = pin!(read);
            let mut message_stream = crate::io::stream::ReadMessageBuf::new(read, stream_options);

            loop {
                match message_stream.read().await {
                    Ok(Some(msg)) => connection.0.lock().handle_message(msg)?,
                    Ok(None) => break Ok(()),
                    Err(err) => break Err(crate::Error::disconnected(format!("read error: {err}"))),
                }
            }
        }
    });

    tokio::spawn({
        let close_signal = close_signal.clone();
        async move {
            tokio::select! {
                err = close_signal.get() => {
                    tasks.abort_all();
                    todo!("closed")
                }
            }
        }
    });

    (bootstrap, close_signal)
}