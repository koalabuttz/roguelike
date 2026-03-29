use russh::{ChannelId, CryptoVec};
use std::io::{self, Write};
use tokio::runtime::Handle;

/// A `Write` impl that buffers crossterm output and sends it as SSH data.
///
/// All `queue!()` calls write to an in-memory buffer. On `flush()`, the
/// entire buffer is sent as a single SSH data message — efficient batched
/// writes (one TCP segment per frame instead of per escape sequence).
pub struct ChannelWriter {
    buf: Vec<u8>,
    handle: russh::server::Handle,
    channel_id: ChannelId,
    rt_handle: Handle,
}

impl ChannelWriter {
    pub fn new(handle: russh::server::Handle, channel_id: ChannelId, rt_handle: Handle) -> Self {
        Self {
            buf: Vec::with_capacity(8192),
            handle,
            channel_id,
            rt_handle,
        }
    }
}

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let data = CryptoVec::from_slice(&self.buf);
        self.buf.clear();
        let handle = self.handle.clone();
        let channel_id = self.channel_id;
        self.rt_handle
            .block_on(async { handle.data(channel_id, data).await })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "SSH channel closed"))?;
        Ok(())
    }
}

