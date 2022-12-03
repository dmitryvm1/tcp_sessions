use std::collections::HashMap;

use bytes::BytesMut;
use tcp_sessions::{network::{Network, NetworkCommand}, session::{SessionEvent}, dispatcher::ParserDispatcher};

#[derive(Clone, Debug)]
pub enum LineCodecState {
    Data,
    /// New line
    NL
}
#[derive(Clone, Debug)]
pub struct LineCodec {
    state: LineCodecState,
    pub tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    start: usize,
    current: Option<Vec<u8>>,
}

impl LineCodec {
    pub fn new(tx: tokio::sync::mpsc::Sender<Vec<u8>>) -> Self {
        LineCodec { 
            state: LineCodecState::Data,
            tx,
            start: 0,
            current: Some(Vec::new()),
        }
    }

}

impl<'a> ParserDispatcher for LineCodec {
    type Message = Vec<u8>;
    /// # Remarks
    /// Current decoder works smoothly when the new line combination(\n \r) is split
    /// over different buffers.
    fn parse_and_dispatch(self: &mut Self, buf: &mut BytesMut, sz: tokio::io::Result<usize>) {
        self.start = 0;
        let mut i = 0;
        if let Ok(sz) = sz {
            while i < sz {
                match self.state {
                    LineCodecState::Data => {
                        if let Some(nl) = buf[self.start..sz].iter().position(|b| *b == 10) {
                            let mut send = false;
                            if let Some(ref mut buffer) = &mut self.current {
                                buffer.extend_from_slice(&buf[self.start..(nl+self.start)]);
                                i = nl + self.start;
                                send = true;
                            }
                            if send {
                                match self.tx.try_send(self.current.take().unwrap()) {
                                    Ok(_) => {},
                                    Err(err) => {
                                        println!("Error sending: {:?}", err);
                                    }
                                }
                            }
                            self.current = Some(Vec::new());
                            self.state = LineCodecState::NL;
                        }
                    },
                    LineCodecState::NL => {
                        // expect 13
                        if buf[i] == 13 {
                            if i < sz - 1 {
                                self.start = i + 1;
                            }
                        } else {
                            self.start = i;
                        }
                        self.state = LineCodecState::Data;
                    }
                    _ => {}
                }
                i+=1;
            }
        }
    }
}

pub fn dispatcher_factory(tx: tokio::sync::mpsc::Sender<Vec<u8>>) -> Box<dyn ParserDispatcher<Message = Vec<u8>>> {
    Box::new(LineCodec::new(tx))
}

#[tokio::main(worker_threads = 4)]
pub async fn main() {
    let mut sessions = HashMap::new();
    let (cmd, mut network) = Network::<LineCodec>::new(dispatcher_factory);
    cmd.send(NetworkCommand::Connect 
        { 
            id: 1,
            name: "tcp session 1".to_owned(),
            target_addr: "127.0.0.1:80".to_owned(),
            proxy: None,
            encoding: "ascii".to_owned()
        }).await.unwrap();

    loop {
        network.process_commands(512);
        // Poll network events
        if let Some(e) = network.poll_event() {
            match e {
                SessionEvent::SessionStart(s) => {
                    sessions.insert(s.id, s);
                    println!("Session started")
                }
                SessionEvent::SessionEnd(id) => {
                    println!("Session over: {}", id);
                }
                SessionEvent::ConnectionFailed(_id) => {}
            }
        }
        // Read data
        for (_id, session) in &mut sessions {
            match session.data_rx.try_recv() {
                Ok(m) => {
                    println!("{:?}", String::from_utf8(m));
                },
                Err(_err) => {  }
            }
        }
    }
}