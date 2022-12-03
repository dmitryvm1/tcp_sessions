use std::{cell::Cell};
use std::fmt::Debug;

use crate::{session::{SessionEvent, handle_connect}, dispatcher::ParserDispatcher};

#[derive(Debug)]
pub enum NetworkCommand {
    Connect {
        id: usize,
        /// Session name
        name: String,
        target_addr: String,
        /// Optional socks5 proxy
        proxy: Option<String>,
        /// Encoding to be used when sending data
        encoding: String
    },
}

pub struct Network<T: ParserDispatcher + Debug + 'static> {
    net_cmd_rx: Cell<Option<tokio::sync::mpsc::Receiver<NetworkCommand>>>,
    events_rx: tokio::sync::mpsc::Receiver<SessionEvent<T>>,
    events_tx: tokio::sync::mpsc::Sender<SessionEvent<T>>,
    dispatcher_factory: Box<dyn Fn(tokio::sync::mpsc::Sender<T::Message>) -> Box<dyn ParserDispatcher<Message = T::Message>> + 'static>
}

impl<T: ParserDispatcher + Debug> Network<T> {
    pub fn new<F>(dispatcher_factory: F) -> (tokio::sync::mpsc::Sender<NetworkCommand>, Network<T>)
        where F: Fn(tokio::sync::mpsc::Sender<T::Message>) -> Box<dyn ParserDispatcher<Message = T::Message>> + 'static {
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let (events_tx, events_rx) = tokio::sync::mpsc::channel::<SessionEvent<T>>(32);
        (tx, Network {
            net_cmd_rx: Cell::new(Some(rx)),
            events_rx,
            events_tx,
            dispatcher_factory: Box::new(dispatcher_factory)
        })
    }

    pub fn poll_event(&mut self) -> Option<SessionEvent<T>> {
        self.events_rx.try_recv().ok()
    }

    pub fn process_commands(&mut self, channel_buffer_sz: usize) {
        let rx = self.net_cmd_rx.get_mut().as_mut().unwrap();
        match rx.try_recv() {
            Ok(NetworkCommand::Connect { id, target_addr, proxy, encoding, name }) => {
               let tx = self.events_tx.clone();
               let (data_tx, data_rx) = tokio::sync::mpsc::channel(channel_buffer_sz);
               let dispatcher = (self.dispatcher_factory)(data_tx);
               tokio::spawn(async move {
                    let result = handle_connect(encoding, proxy, target_addr, tx.clone(), id, name, dispatcher, data_rx).await;
                    match result {
                        Ok(_) => {},
                        Err(err) => {
                            tx.try_send(SessionEvent::ConnectionFailed(id)).unwrap();
                            println!("Error connecting: {:?}", err);
                        }
                    }
               });
            },
            _ => {}
        }
    }
  
}


