//! SNI (Server Name Indication) peeking and stream prepending.
//!
//! When the rigor layer redirects all outbound :443 connections to the daemon,
//! the daemon needs to know which real host the client wanted to reach. The
//! TLS ClientHello carries this info in the SNI extension. We peek it without
//! completing the handshake, then either MITM the connection (if the host is
//! in MITM_HOSTS) or blind-tunnel to the real upstream.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};

/// Read enough bytes from the stream to capture the ClientHello, then parse SNI.
/// Returns (buffered_bytes, sni_hostname). The buffered bytes can be replayed
/// either through `PrependedStream` (for MITM) or written to upstream (for blind tunnel).
pub async fn peek_client_hello<R: AsyncRead + Unpin>(
    stream: &mut R,
) -> io::Result<(Vec<u8>, Option<String>)> {
    // TLS record header is 5 bytes: type(1) + version(2) + length(2)
    let mut header = [0u8; 5];
    stream.read_exact(&mut header).await?;

    if header[0] != 0x16 {
        // Not a TLS handshake record (0x16 = handshake)
        return Ok((header.to_vec(), None));
    }

    let record_len = u16::from_be_bytes([header[3], header[4]]) as usize;
    if record_len > 16 * 1024 {
        // Suspiciously large — refuse
        return Ok((header.to_vec(), None));
    }

    let mut record = vec![0u8; record_len];
    stream.read_exact(&mut record).await?;

    let mut full = Vec::with_capacity(5 + record_len);
    full.extend_from_slice(&header);
    full.extend_from_slice(&record);

    let sni = parse_sni_from_client_hello(&record);
    Ok((full, sni))
}

/// Parse SNI from a TLS handshake record body (no record header).
/// Returns the first SNI hostname found, or None.
fn parse_sni_from_client_hello(data: &[u8]) -> Option<String> {
    // Handshake header: type(1) + length(3)
    if data.len() < 4 || data[0] != 0x01 {
        return None; // not a ClientHello
    }
    let mut p = 4;

    // ClientHello body: version(2) + random(32)
    p += 34;
    if p + 1 > data.len() {
        return None;
    }

    // Skip session_id (length-prefixed)
    let session_id_len = data[p] as usize;
    p += 1 + session_id_len;
    if p + 2 > data.len() {
        return None;
    }

    // Skip cipher suites (length-prefixed)
    let cs_len = u16::from_be_bytes([data[p], data[p + 1]]) as usize;
    p += 2 + cs_len;
    if p + 1 > data.len() {
        return None;
    }

    // Skip compression methods (length-prefixed)
    let cm_len = data[p] as usize;
    p += 1 + cm_len;
    if p + 2 > data.len() {
        return None;
    }

    // Extensions
    let ext_total_len = u16::from_be_bytes([data[p], data[p + 1]]) as usize;
    p += 2;
    let ext_end = (p + ext_total_len).min(data.len());

    while p + 4 <= ext_end {
        let ext_type = u16::from_be_bytes([data[p], data[p + 1]]);
        let ext_len = u16::from_be_bytes([data[p + 2], data[p + 3]]) as usize;
        p += 4;
        if p + ext_len > ext_end {
            return None;
        }

        if ext_type == 0x0000 {
            // SNI extension: list_len(2) + name_type(1) + name_len(2) + name
            if ext_len < 5 {
                return None;
            }
            let name_type = data[p + 2];
            let name_len = u16::from_be_bytes([data[p + 3], data[p + 4]]) as usize;
            if name_type != 0 || 5 + name_len > ext_len {
                return None;
            }
            return std::str::from_utf8(&data[p + 5..p + 5 + name_len])
                .ok()
                .map(|s| s.to_string());
        }
        p += ext_len;
    }

    None
}

/// A wrapper that yields `prefix` bytes first, then reads from `inner`.
/// Used to replay buffered ClientHello bytes through a TLS acceptor.
pub struct PrependedStream<S> {
    prefix: Vec<u8>,
    pos: usize,
    inner: S,
}

impl<S> PrependedStream<S> {
    pub fn new(prefix: Vec<u8>, inner: S) -> Self {
        Self { prefix, pos: 0, inner }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for PrependedStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.pos < self.prefix.len() {
            let remaining = &self.prefix[self.pos..];
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            self.pos += to_copy;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for PrependedStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sni_minimal() {
        // Hand-crafted minimal ClientHello with SNI = "example.com"
        // type(1=0x01) + length(3) + version(2) + random(32) + session_id(0)
        // + ciphers(2 bytes len + 2 bytes content) + compression(1 byte len + 1 byte)
        // + extensions(2 bytes len + SNI extension)
        let mut data = Vec::new();
        data.push(0x01); // ClientHello
        data.extend_from_slice(&[0, 0, 0]); // length placeholder
        data.extend_from_slice(&[0x03, 0x03]); // version
        data.extend_from_slice(&[0u8; 32]); // random
        data.push(0); // session_id length
        data.extend_from_slice(&[0, 2]); // cipher suites length
        data.extend_from_slice(&[0xc0, 0x2f]); // one cipher
        data.push(1); // compression methods length
        data.push(0); // compression method

        // Build SNI extension
        let host = b"example.com";
        let mut sni_ext = Vec::new();
        sni_ext.extend_from_slice(&[0, 0]); // ext type SNI
        let sni_data_len = 2 + 1 + 2 + host.len(); // list_len + name_type + name_len + name
        sni_ext.extend_from_slice(&(sni_data_len as u16).to_be_bytes());
        sni_ext.extend_from_slice(&((1 + 2 + host.len()) as u16).to_be_bytes()); // server_name_list length
        sni_ext.push(0); // name_type host_name
        sni_ext.extend_from_slice(&(host.len() as u16).to_be_bytes());
        sni_ext.extend_from_slice(host);

        // Extensions container
        data.extend_from_slice(&(sni_ext.len() as u16).to_be_bytes());
        data.extend_from_slice(&sni_ext);

        let sni = parse_sni_from_client_hello(&data);
        assert_eq!(sni.as_deref(), Some("example.com"));
    }

    #[test]
    fn test_parse_sni_returns_none_for_non_clienthello() {
        let data = vec![0x02; 100]; // Not a ClientHello
        assert_eq!(parse_sni_from_client_hello(&data), None);
    }
}
