use std::pin::Pin;
use tokio::{
    net::TcpStream,
    time::Sleep,
};

pub struct StreamWithBuffer {
    pub stream: TcpStream,
    buf: Option<Box<[u8]>>,
    pos: usize,
    // writeIndex
    cap: usize,
    // readIndex
    pub read_eof: bool,
    pub done: bool,
}

impl StreamWithBuffer {
    pub fn new(stream: TcpStream) -> Self {
        StreamWithBuffer {
            stream,
            buf: None,
            pos: 0,
            cap: 0,
            read_eof: false,
            done: false,
        }
    }
}

pub struct BiPipe {
    left: StreamWithBuffer,
    right: StreamWithBuffer,
    half_close_deadline: Option<Pin<Box<Sleep>>>,
}

pub fn pipe(left: TcpStream, right: TcpStream) -> BiPipe {
    BiPipe {
        left: StreamWithBuffer::new(left),
        right: StreamWithBuffer::new(right),
        half_close_deadline: Default::default(),
    }
}