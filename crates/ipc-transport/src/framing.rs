//! Accepted G1 wire framing: `u32` big-endian payload length + payload.
//!
//! Transports bytes/frames only. No Protobuf interpretation happens here:
//! `HandshakeRequest`/`PingRequest`/`ShutdownRequest` semantics belong to
//! TASK-0019. One request in flight per connection is the G1 model, but
//! sequencing is not implemented here.

use std::future::poll_fn;
use std::io;
use std::pin::Pin;

use bytes::{Bytes, BytesMut};
use futures_core::Stream;
use futures_sink::Sink;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time;
use tokio_util::codec::{
    Decoder, Encoder, Framed, LengthDelimitedCodec, LengthDelimitedCodecError,
};

use crate::{TransportError, FRAME_IO_TIMEOUT, MAX_FRAME_BYTES};

/// Transport-facing framing codec for G1 control messages.
///
/// Thin wrapper around [`LengthDelimitedCodec`] with every
/// security-relevant value configured explicitly (never relying on
/// undocumented defaults): 4-byte `u32` big-endian length prefix,
/// [`MAX_FRAME_BYTES`](crate::MAX_FRAME_BYTES) cap, zero-length payloads
/// rejected on both encode and decode.
#[derive(Debug)]
pub struct ControlFrameCodec {
    inner: LengthDelimitedCodec,
}

impl ControlFrameCodec {
    /// Builds the codec with the accepted G1 wire parameters.
    pub fn new() -> Self {
        let mut builder = LengthDelimitedCodec::builder();
        builder
            .length_field_length(4)
            .big_endian()
            .max_frame_length(MAX_FRAME_BYTES);
        Self {
            inner: builder.new_codec(),
        }
    }

    /// The enforced maximum frame payload in bytes.
    #[must_use]
    pub fn max_frame_bytes(&self) -> usize {
        self.inner.max_frame_length()
    }
}

impl Default for ControlFrameCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Encoder<Bytes> for ControlFrameCodec {
    type Error = TransportError;

    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
        if item.is_empty() {
            return Err(TransportError::EmptyFrame);
        }
        if item.len() > MAX_FRAME_BYTES {
            return Err(TransportError::FrameTooLarge {
                declared: u32::try_from(item.len()).ok(),
                limit: MAX_FRAME_BYTES,
            });
        }
        self.inner.encode(item, dst).map_err(map_codec_error)
    }
}

impl Decoder for ControlFrameCodec {
    type Item = BytesMut;
    type Error = TransportError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self
            .inner
            .decode(src)
            .map_err(|error| map_decode_error(src, error))?
        {
            Some(frame) if frame.is_empty() => Err(TransportError::EmptyFrame),
            frame => Ok(frame),
        }
    }
}

/// A framed byte-stream with the accepted G1 frame-I/O deadline applied.
///
/// `TASK-0019` performs actual request reads/writes through
/// [`read_frame`](Self::read_frame)/[`write_frame`](Self::write_frame);
/// malformed framing terminates use of the connection (no
/// resynchronization).
pub struct FramedTransport<T> {
    inner: Framed<T, ControlFrameCodec>,
}

impl<T> FramedTransport<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    /// Wraps a connected byte stream with G1 framing.
    pub fn new(io: T) -> Self {
        Self {
            inner: Framed::new(io, ControlFrameCodec::new()),
        }
    }

    /// Reads exactly one frame, bounded by
    /// [`FRAME_IO_TIMEOUT`](crate::FRAME_IO_TIMEOUT).
    pub async fn read_frame(&mut self) -> Result<BytesMut, TransportError> {
        let item = time::timeout(
            FRAME_IO_TIMEOUT,
            poll_fn(|cx| Pin::new(&mut self.inner).poll_next(cx)),
        )
        .await
        .map_err(|_| TransportError::timeout())?;
        match item {
            Some(frame) => frame,
            None => Err(TransportError::MalformedFrame(
                "connection closed before a frame completed".to_owned(),
            )),
        }
    }

    /// Writes exactly one frame, bounded by
    /// [`FRAME_IO_TIMEOUT`](crate::FRAME_IO_TIMEOUT).
    pub async fn write_frame(&mut self, payload: Bytes) -> Result<(), TransportError> {
        if payload.is_empty() {
            return Err(TransportError::EmptyFrame);
        }
        if payload.len() > MAX_FRAME_BYTES {
            return Err(TransportError::FrameTooLarge {
                declared: u32::try_from(payload.len()).ok(),
                limit: MAX_FRAME_BYTES,
            });
        }
        time::timeout(FRAME_IO_TIMEOUT, async {
            poll_fn(|cx| Pin::new(&mut self.inner).poll_ready(cx)).await?;
            Pin::new(&mut self.inner).start_send(payload)?;
            poll_fn(|cx| Pin::new(&mut self.inner).poll_flush(cx)).await
        })
        .await
        .map_err(|_| TransportError::timeout())?
    }

    /// Borrows the underlying stream.
    #[must_use]
    pub fn get_ref(&self) -> &T {
        self.inner.get_ref()
    }

    /// Unwraps into the raw `Framed` stream for future layers.
    #[must_use]
    pub fn into_inner(self) -> Framed<T, ControlFrameCodec> {
        self.inner
    }
}

/// Detects the opaque "frame size too big" signal from the inner codec.
fn is_frame_too_big(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<LengthDelimitedCodecError>())
}

fn map_codec_error(error: io::Error) -> TransportError {
    if is_frame_too_big(&error) {
        TransportError::FrameTooLarge {
            declared: None,
            limit: MAX_FRAME_BYTES,
        }
    } else {
        TransportError::Io(error)
    }
}

fn map_decode_error(prefix: &[u8], error: io::Error) -> TransportError {
    if is_frame_too_big(&error) {
        let declared = prefix
            .get(..4)
            .and_then(|head| head.try_into().ok())
            .map(u32::from_be_bytes);
        TransportError::FrameTooLarge {
            declared,
            limit: MAX_FRAME_BYTES,
        }
    } else {
        TransportError::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codec() -> ControlFrameCodec {
        ControlFrameCodec::new()
    }

    #[test]
    fn codec_is_configured_to_accepted_g1_parameters() {
        assert_eq!(codec().max_frame_bytes(), MAX_FRAME_BYTES);
        assert_eq!(MAX_FRAME_BYTES, 1_048_576);
    }

    #[test]
    fn length_prefix_is_exact_u32_big_endian() {
        let mut codec = codec();
        let mut dst = BytesMut::new();
        codec
            .encode(Bytes::from_static(b"hi"), &mut dst)
            .expect("small payload must encode");
        assert_eq!(&dst[..], b"\x00\x00\x00\x02hi");
    }

    #[test]
    fn small_payload_round_trip() {
        let mut codec = codec();
        let mut buf = BytesMut::new();
        let payload = Bytes::from_static(b"control-bytes");
        codec.encode(payload.clone(), &mut buf).expect("encode");
        let decoded = codec.decode(&mut buf).expect("decode").expect("frame");
        assert_eq!(decoded.freeze(), payload);
        assert!(buf.is_empty());
    }

    #[test]
    fn one_mebibyte_payload_is_accepted() {
        let mut codec = codec();
        let mut buf = BytesMut::new();
        let payload = Bytes::from(vec![0xABu8; MAX_FRAME_BYTES]);
        codec.encode(payload.clone(), &mut buf).expect("encode");
        // 4-byte prefix + payload, nothing more.
        assert_eq!(buf.len(), MAX_FRAME_BYTES + 4);
        let decoded = codec.decode(&mut buf).expect("decode").expect("frame");
        assert_eq!(decoded.len(), MAX_FRAME_BYTES);
        assert_eq!(decoded.freeze(), payload);
    }

    #[test]
    fn oversized_payload_encode_is_rejected() {
        let mut codec = codec();
        let mut dst = BytesMut::new();
        let payload = Bytes::from(vec![0u8; MAX_FRAME_BYTES + 1]);
        match codec.encode(payload, &mut dst) {
            Err(TransportError::FrameTooLarge { declared, limit }) => {
                assert_eq!(declared, Some((MAX_FRAME_BYTES + 1) as u32));
                assert_eq!(limit, MAX_FRAME_BYTES);
            }
            other => panic!("expected FrameTooLarge, got {other:?}"),
        }
        assert!(dst.is_empty());
    }

    #[test]
    fn zero_length_payload_encode_is_rejected() {
        let mut codec = codec();
        let mut dst = BytesMut::new();
        match codec.encode(Bytes::new(), &mut dst) {
            Err(TransportError::EmptyFrame) => {}
            other => panic!("expected EmptyFrame, got {other:?}"),
        }
        assert!(dst.is_empty());
    }

    #[test]
    fn zero_length_payload_decode_is_rejected() {
        let mut codec = codec();
        let mut buf = BytesMut::from(&b"\x00\x00\x00\x00"[..]);
        match codec.decode(&mut buf) {
            Err(TransportError::EmptyFrame) => {}
            other => panic!("expected EmptyFrame, got {other:?}"),
        }
    }

    #[test]
    fn declared_oversized_incoming_frame_is_rejected_without_payload() {
        let mut codec = codec();
        // Declares 2 MiB but carries no payload bytes at all.
        let declared: u32 = 2 * 1_048_576;
        let mut buf = BytesMut::from(&declared.to_be_bytes()[..]);
        match codec.decode(&mut buf) {
            Err(TransportError::FrameTooLarge {
                declared: got,
                limit,
            }) => {
                assert_eq!(got, Some(declared));
                assert_eq!(limit, MAX_FRAME_BYTES);
            }
            other => panic!("expected FrameTooLarge, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn framed_payload_crosses_in_memory_stream() {
        let (left, right) = tokio::io::duplex(64);
        let (mut writer, mut reader) = (FramedTransport::new(left), FramedTransport::new(right));
        writer
            .write_frame(Bytes::from_static(b"ping-bytes"))
            .await
            .expect("write");
        let frame = reader.read_frame().await.expect("read");
        assert_eq!(&frame[..], b"ping-bytes");
    }

    #[tokio::test]
    async fn closed_stream_read_is_malformed_not_silent() {
        let (left, right) = tokio::io::duplex(64);
        drop(left);
        let mut reader = FramedTransport::new(right);
        match reader.read_frame().await {
            Err(TransportError::MalformedFrame(_)) => {}
            other => panic!("expected MalformedFrame, got {other:?}"),
        }
    }
}
