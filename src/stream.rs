use std::{
    cell::RefCell,
    cmp,
    future::Future,
    io::{self},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use self::Side::{Left, Right};
use log::{debug, trace};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
    time::{sleep, Instant, Sleep},
};
macro_rules! try_poll {
    ($expr:expr) => {
        match $expr {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Ready(Ok(ok)) => ok,
        }
    };
}

const SHARED_BUF_SIZE: usize = 1024 * 64;
const PRIVATE_BUF_SIZE: usize = 1024 * 8;
const HALF_CLOSE_TIMEOUT: Duration = Duration::from_secs(60);
thread_local! {
    static SHARED_BUFFER:RefCell<[u8;SHARED_BUF_SIZE]> = RefCell::new([0u8;SHARED_BUF_SIZE]);
}

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
    pub fn is_empty(&self) -> bool {
        self.pos == self.cap
    }

    // Read from self.stream, put the data into buffer
    pub fn poll_read_to_buffer(&mut self, cx: &mut Context) -> Poll<io::Result<usize>> {
        let stream = Pin::new(&mut self.stream);

        let n = try_poll!(if let Some(ref mut buf) = self.buf {
            let mut buf = ReadBuf::new(buf);
            stream
                .poll_read(cx, &mut buf)
                .map_ok(|_| buf.filled().len())
        } else {
            SHARED_BUFFER.with(|buf| {
                let shared_buf = &mut buf.borrow_mut()[..];
                let mut buf = ReadBuf::new(shared_buf);
                stream
                    .poll_read(cx, &mut buf)
                    .map_ok(|_| buf.filled().len())
            })
        });

        if n == 0 {
            self.read_eof = true;
        } else {
            self.pos = 0;
            self.cap = 0;
        }

        Poll::Ready(Ok(n))
    }

    pub fn poll_write_buffer_to(
        &mut self,
        ctx: &mut Context,
        write_stream: &mut TcpStream,
    ) -> Poll<io::Result<usize>> {
        let writer = Pin::new(write_stream);
        let result = if let Some(ref buf) = self.buf {
            writer.poll_write(ctx, &buf[self.pos..self.cap])
        } else {
            SHARED_BUFFER.with(|cell| {
                let buf = cell.borrow_mut();
                writer.poll_write(ctx, &buf[self.pos..self.cap])
            })
        };
        match result {
            Poll::Ready(Ok(0)) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "write zero bytes into writer",
            ))),
            Poll::Ready(Ok(n)) => {
                self.pos += n;
                trace!("{} bytes written to writer", n);
                Poll::Ready(Ok(n))
            }
            Poll::Pending if self.buf.is_none() => {
                let available = self.cap - self.pos;
                SHARED_BUFFER.with(|shared_buf| {
                    let shared_buf = shared_buf.borrow();
                    let mut buf = vec![0; cmp::max(PRIVATE_BUF_SIZE, available)];
                    buf[..available].copy_from_slice(&shared_buf[self.pos..self.cap]);
                    self.buf = Some(buf.into_boxed_slice());
                    Poll::Pending
                })
            }
            _ => result,
        }
    }
}

#[derive(Debug, Clone)]
enum Side {
    Left,
    Right,
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

impl BiPipe {
    fn poll_one_side(&mut self, ctx: &mut Context, side: Side) -> Poll<io::Result<()>> {
        let Self {
            ref mut left,
            ref mut right,
            ..
        } = *self;
        let (reader, writer) = match side {
            Side::Left => (left, right),
            Side::Right => (right, left),
        };
        loop {
            if reader.is_empty() && !reader.read_eof {
                try_poll!(reader.poll_read_to_buffer(ctx));
            }

            while !reader.is_empty() {
                try_poll!(reader.poll_write_buffer_to(ctx, &mut writer.stream));
            }
            if reader.read_eof {
                match Pin::new(&mut writer.stream).poll_shutdown(ctx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(())) => (),
                    Poll::Ready(Err(err)) => {
                        debug!("failed to shutdown, maybe connect error. Error=[{}]", err)
                    }
                }
                // writer 已经关闭
                // reader 将在另一个 poll_one_side 作为 writer 被关闭
                reader.done = true;
                return Poll::Ready(Ok(()));
            }
        }
    }
}

impl Future for BiPipe {
    type Output = io::Result<()>;
    // https://stackoverflow.com/questions/28587698/whats-the-difference-between-placing-mut-before-a-variable-name-and-after-the
    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.left.done {
            if let Poll::Ready(Err(err)) = self.poll_one_side(ctx, Left) {
                return Poll::Ready(Err(err));
            }
        }

        if !self.right.done {
            if let Poll::Ready(Err(err)) = self.poll_one_side(ctx, Right) {
                return Poll::Ready(Err(err));
            }
        }

        match (self.left.done, self.right.done) {
            (true, true) => return Poll::Ready(Ok(())),
            (false, false) => Poll::Pending,
            _ => match &mut self.half_close_deadline {
                None => {
                    // 首次进入
                    let mut ddl = Box::pin(sleep(HALF_CLOSE_TIMEOUT));
                    let _ = ddl.as_mut().poll(ctx);
                    self.half_close_deadline = Some(ddl);
                    Poll::Pending
                }
                Some(ddl) if !ddl.is_elapsed() => {
                    // 设置超时时间
                    ddl.as_mut().reset(Instant::now() + HALF_CLOSE_TIMEOUT);
                    Poll::Pending
                }
                Some(_) => {
                    // 超时后提前返回
                    debug!("BiPipe half-close conn timeout");
                    Poll::Ready(Ok(()))
                }
            },
        }
    }
}
