use bytes::BytesMut;
use std::fmt::Debug;

pub trait ParserDispatcher: Send {
    type Message: Debug + Send;
    fn parse_and_dispatch(self: &mut Self, buf: &mut BytesMut, sz: tokio::io::Result<usize>);
}