use std::error::Error;
use std::fmt::Debug;
use bytes::BytesMut;
use futures::{select, FutureExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{tcp::OwnedWriteHalf, TcpStream},
};

use crate::dispatcher::{ParserDispatcher};

#[derive(Debug)]
pub enum SessionCommand {
    Close,
}

#[derive(Debug)]
pub enum SessionEvent<T:ParserDispatcher> {
    SessionEnd(usize),
    ConnectionFailed(usize),
    SessionStart(Session<T>),
}

#[derive(Debug)]
pub struct Session<T: ParserDispatcher> {
    /// Profile id
    pub id: usize,
    /// Display name
    pub name: String,
    /// For sending session commands
    pub tx: tokio::sync::mpsc::Sender<SessionCommand>,
    /// For sending data to server
    pub sender: tokio::sync::mpsc::Sender<Vec<u8>>,
    /// For receiving decoded messages from server
    pub data_rx: tokio::sync::mpsc::Receiver<T::Message>,
}

impl<T:ParserDispatcher> Session<T> {}

pub(crate) async fn handle_connect<'a, T>(
    enc: String,
    proxy: Option<String>,
    server: String,
    event_tx: tokio::sync::mpsc::Sender<SessionEvent<T>>,
    id: usize,
    name: String,
    dispatcher: Box<dyn ParserDispatcher<Message = T::Message>>,
    data_rx: tokio::sync::mpsc::Receiver<T::Message>
) -> Result<(), Box<dyn Error>>
where T: ParserDispatcher + Debug + 'static {
    println!("Connecting... Proxy: {:?}, Server: {:?}", proxy, server);
    let connection = {
        let stream = TcpStream::connect(&server).await?;
        stream
    };
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    let (sender, sender_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(32);
    let (read_half, write_half) = connection.into_split();
    spawn_worker(id, sender_rx, write_half, read_half, dispatcher, event_tx.clone(), rx);
    let session = Session {
        tx,
        sender,
        data_rx,
        id,
        name,
    };
    event_tx.send(SessionEvent::SessionStart(session)).await?;
    Ok(())
}

fn spawn_worker<T>(
    id: usize,
    mut sender_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    mut write_half: OwnedWriteHalf,
    mut read_half: OwnedReadHalf,
    mut dispatcher: Box<dyn ParserDispatcher<Message = T::Message>>,
    session_event_tx: tokio::sync::mpsc::Sender<SessionEvent<T>>,
    mut session_cmd_rx: tokio::sync::mpsc::Receiver<SessionCommand>,
) where T: ParserDispatcher + Debug + 'static {
    let mut buf = BytesMut::with_capacity(4096);
    tokio::spawn(async move {
        loop {
            buf.resize(4096, 0);
            select! {
                cmd_to_send = sender_rx.recv().fuse() => {
                    if let Some(cmd_to_send) = cmd_to_send {
                        write_half.write(cmd_to_send.as_slice()).await.unwrap();
                    }
                }
                cmd = session_cmd_rx.recv().fuse() => {
                    if let Some(cmd) = cmd {
                        match cmd {
                            SessionCommand::Close => {
                                // TODO: more stuff, close socket
                                println!("SessionCommand::Close received");
                                break;
                            }
                        }
                    }
                },
                sz = read_half.read(&mut buf).fuse() => {
                    match sz {
                        Ok(0) => {
                            session_event_tx.send(SessionEvent::SessionEnd(id)).await.unwrap();
                            break;
                        },
                        _ => {}
                    }
                    dispatcher.parse_and_dispatch(&mut buf, sz);
                },
            };
        }
    });
}
