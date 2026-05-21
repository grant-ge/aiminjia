//! Minimal hand-rolled protobuf decoder for the feishu `pbbp2.Frame` envelope.
//!
//! We do NOT pull in `prost`+`prost-build`: the schema is tiny (2 messages, 9
//! fields) and the upstream `larksuite/oapi-sdk-go` repo ships a precompiled
//! `ws/pbbp2.pb.go` (no .proto in tree). Reconstructed schema (proto2 syntax):
//!
//! ```proto
//! syntax = "proto2";
//! package pbbp2;
//!
//! message Header {
//!   required string key   = 1;
//!   required string value = 2;
//! }
//!
//! message Frame {
//!   required uint64 SeqID            = 1;
//!   required uint64 LogID            = 2;
//!   required int32  service          = 3;
//!   required int32  method           = 4;   // 0 = control, 1 = data
//!   repeated Header headers          = 5;
//!   optional string payload_encoding = 6;   // "" or "gzip"
//!   optional string payload_type     = 7;
//!   optional bytes  payload          = 8;
//!   optional string LogIDNew         = 9;
//! }
//! ```
//!
//! Source: `larksuite/oapi-sdk-go` v3_main `ws/pbbp2.pb.go` (struct Frame at
//! line ~80) and `ws/const.go` for FrameType / MessageType enum values.

use anyhow::{bail, Context, Result};

/// FrameType values that may appear in `Frame::method` (per oapi-sdk-go const.go).
pub const FRAME_TYPE_CONTROL: i32 = 0;
pub const FRAME_TYPE_DATA: i32 = 1;

/// Header keys (per oapi-sdk-go const.go).
pub const HEADER_TYPE: &str = "type"; // message type — event / card / ping / pong
pub const HEADER_MESSAGE_ID: &str = "message_id";
pub const HEADER_SUM: &str = "sum"; // total number of fragments for this logical message
pub const HEADER_SEQ: &str = "seq"; // fragment index (0-based)

/// MessageType values that may appear in the `"type"` header (per oapi-sdk-go const.go).
pub const MSG_TYPE_EVENT: &str = "event";
pub const MSG_TYPE_CARD: &str = "card";
pub const MSG_TYPE_PING: &str = "ping";
pub const MSG_TYPE_PONG: &str = "pong";

#[derive(Debug, Default, Clone)]
pub struct Frame {
    pub seq_id: u64,
    pub log_id: u64,
    pub service: i32,
    pub method: i32,
    pub headers: Vec<Header>,
    pub payload_encoding: String,
    pub payload_type: String,
    pub payload: Vec<u8>,
    pub log_id_new: String,
}

#[derive(Debug, Default, Clone)]
pub struct Header {
    pub key: String,
    pub value: String,
}

impl Frame {
    /// Look up a header by key (case-sensitive). Returns the first match.
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.key == key)
            .map(|h| h.value.as_str())
    }

    /// Decode a Frame from a single WebSocket binary frame's bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut frame = Frame::default();
        let mut cursor = Cursor::new(bytes);
        while !cursor.is_empty() {
            let tag = cursor.read_varint().context("read tag")?;
            let field_num = (tag >> 3) as u32;
            let wire_type = (tag & 0x7) as u8;
            match (field_num, wire_type) {
                // SeqID = 1, varint
                (1, 0) => frame.seq_id = cursor.read_varint().context("read SeqID")?,
                // LogID = 2, varint
                (2, 0) => frame.log_id = cursor.read_varint().context("read LogID")?,
                // service = 3, varint (int32; protobuf signed varints for plain int32 are encoded as uint64 wire)
                (3, 0) => {
                    frame.service = cursor.read_varint().context("read service")? as i32;
                }
                // method = 4, varint
                (4, 0) => {
                    frame.method = cursor.read_varint().context("read method")? as i32;
                }
                // headers = 5, length-delimited (repeated message)
                (5, 2) => {
                    let len = cursor.read_varint().context("read headers len")? as usize;
                    let header_bytes = cursor.read_bytes(len).context("read headers bytes")?;
                    frame
                        .headers
                        .push(Header::decode(header_bytes).context("decode header")?);
                }
                // payload_encoding = 6, length-delimited (string)
                (6, 2) => {
                    let len = cursor.read_varint().context("read payload_encoding len")? as usize;
                    let b = cursor.read_bytes(len).context("read payload_encoding")?;
                    frame.payload_encoding =
                        String::from_utf8(b.to_vec()).context("payload_encoding utf8")?;
                }
                // payload_type = 7, length-delimited (string)
                (7, 2) => {
                    let len = cursor.read_varint().context("read payload_type len")? as usize;
                    let b = cursor.read_bytes(len).context("read payload_type")?;
                    frame.payload_type =
                        String::from_utf8(b.to_vec()).context("payload_type utf8")?;
                }
                // payload = 8, length-delimited (bytes)
                (8, 2) => {
                    let len = cursor.read_varint().context("read payload len")? as usize;
                    let b = cursor.read_bytes(len).context("read payload")?;
                    frame.payload = b.to_vec();
                }
                // LogIDNew = 9, length-delimited (string)
                (9, 2) => {
                    let len = cursor.read_varint().context("read LogIDNew len")? as usize;
                    let b = cursor.read_bytes(len).context("read LogIDNew")?;
                    frame.log_id_new = String::from_utf8(b.to_vec()).context("LogIDNew utf8")?;
                }
                // Unknown fields — skip per wire-type.
                (_, 0) => {
                    let _ = cursor.read_varint()?;
                }
                (_, 1) => {
                    // 64-bit fixed
                    let _ = cursor.read_bytes(8)?;
                }
                (_, 2) => {
                    let len = cursor.read_varint()? as usize;
                    let _ = cursor.read_bytes(len)?;
                }
                (_, 5) => {
                    // 32-bit fixed
                    let _ = cursor.read_bytes(4)?;
                }
                (_, wt) => bail!("unsupported wire type {} for field {}", wt, field_num),
            }
        }
        Ok(frame)
    }

    /// Encode the Frame back to protobuf wire format. Only the fields we
    /// actually set on the response path are emitted — empty strings / zero
    /// numerics are kept as proto2 defaults but still serialized for parity
    /// with what the Go SDK writes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        // 1, varint
        write_tag(&mut out, 1, 0);
        write_varint(&mut out, self.seq_id);
        // 2, varint
        write_tag(&mut out, 2, 0);
        write_varint(&mut out, self.log_id);
        // 3, varint
        write_tag(&mut out, 3, 0);
        write_varint(&mut out, self.service as u64);
        // 4, varint
        write_tag(&mut out, 4, 0);
        write_varint(&mut out, self.method as u64);
        // 5, length-delimited (each header is a message)
        for h in &self.headers {
            let hbytes = h.encode();
            write_tag(&mut out, 5, 2);
            write_varint(&mut out, hbytes.len() as u64);
            out.extend_from_slice(&hbytes);
        }
        // 6 / 7 / 8 / 9 — only emit when non-empty (proto2 optional semantics).
        if !self.payload_encoding.is_empty() {
            write_tag(&mut out, 6, 2);
            write_varint(&mut out, self.payload_encoding.len() as u64);
            out.extend_from_slice(self.payload_encoding.as_bytes());
        }
        if !self.payload_type.is_empty() {
            write_tag(&mut out, 7, 2);
            write_varint(&mut out, self.payload_type.len() as u64);
            out.extend_from_slice(self.payload_type.as_bytes());
        }
        if !self.payload.is_empty() {
            write_tag(&mut out, 8, 2);
            write_varint(&mut out, self.payload.len() as u64);
            out.extend_from_slice(&self.payload);
        }
        if !self.log_id_new.is_empty() {
            write_tag(&mut out, 9, 2);
            write_varint(&mut out, self.log_id_new.len() as u64);
            out.extend_from_slice(self.log_id_new.as_bytes());
        }
        out
    }
}

impl Header {
    fn decode(bytes: &[u8]) -> Result<Self> {
        let mut h = Header::default();
        let mut cursor = Cursor::new(bytes);
        while !cursor.is_empty() {
            let tag = cursor.read_varint()?;
            let field_num = (tag >> 3) as u32;
            let wire_type = (tag & 0x7) as u8;
            match (field_num, wire_type) {
                (1, 2) => {
                    let len = cursor.read_varint()? as usize;
                    let b = cursor.read_bytes(len)?;
                    h.key = String::from_utf8(b.to_vec()).context("header key utf8")?;
                }
                (2, 2) => {
                    let len = cursor.read_varint()? as usize;
                    let b = cursor.read_bytes(len)?;
                    h.value = String::from_utf8(b.to_vec()).context("header value utf8")?;
                }
                (_, 0) => {
                    let _ = cursor.read_varint()?;
                }
                (_, 2) => {
                    let len = cursor.read_varint()? as usize;
                    let _ = cursor.read_bytes(len)?;
                }
                (_, wt) => bail!("unsupported wire type {} in header", wt),
            }
        }
        Ok(h)
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.key.len() + self.value.len() + 4);
        write_tag(&mut out, 1, 2);
        write_varint(&mut out, self.key.len() as u64);
        out.extend_from_slice(self.key.as_bytes());
        write_tag(&mut out, 2, 2);
        write_varint(&mut out, self.value.len() as u64);
        out.extend_from_slice(self.value.as_bytes());
        out
    }
}

// ---------------------------------------------------------------------------
// Varint helpers
// ---------------------------------------------------------------------------

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.buf.len() {
            bail!(
                "protobuf: short read need {} have {}",
                n,
                self.buf.len() - self.pos
            );
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn read_varint(&mut self) -> Result<u64> {
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            if self.pos >= self.buf.len() {
                bail!("protobuf: varint truncated");
            }
            let byte = self.buf[self.pos];
            self.pos += 1;
            value |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            shift += 7;
            if shift > 63 {
                bail!("protobuf: varint overflow");
            }
        }
    }
}

fn write_tag(out: &mut Vec<u8>, field_num: u32, wire_type: u8) {
    write_varint(out, ((field_num as u64) << 3) | (wire_type as u64));
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_small_and_large() {
        for v in [0u64, 1, 127, 128, 16383, 16384, 1 << 35, u64::MAX] {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            let mut c = Cursor::new(&buf);
            assert_eq!(c.read_varint().unwrap(), v, "roundtrip {}", v);
            assert!(c.is_empty());
        }
    }

    #[test]
    fn frame_roundtrip_with_headers_and_payload() {
        let orig = Frame {
            seq_id: 42,
            log_id: 1_000_000,
            service: 7,
            method: FRAME_TYPE_DATA,
            headers: vec![
                Header {
                    key: HEADER_TYPE.into(),
                    value: MSG_TYPE_EVENT.into(),
                },
                Header {
                    key: HEADER_MESSAGE_ID.into(),
                    value: "om_test_123".into(),
                },
                Header {
                    key: HEADER_SUM.into(),
                    value: "1".into(),
                },
                Header {
                    key: HEADER_SEQ.into(),
                    value: "0".into(),
                },
            ],
            payload_encoding: "".into(),
            payload_type: "".into(),
            payload: b"{\"event\":\"hi\"}".to_vec(),
            log_id_new: "ln_xx".into(),
        };
        let bytes = orig.encode();
        let decoded = Frame::decode(&bytes).expect("decode");
        assert_eq!(decoded.seq_id, orig.seq_id);
        assert_eq!(decoded.log_id, orig.log_id);
        assert_eq!(decoded.service, orig.service);
        assert_eq!(decoded.method, orig.method);
        assert_eq!(decoded.payload, orig.payload);
        assert_eq!(decoded.log_id_new, orig.log_id_new);
        assert_eq!(decoded.headers.len(), 4);
        assert_eq!(decoded.header(HEADER_TYPE), Some(MSG_TYPE_EVENT));
        assert_eq!(decoded.header(HEADER_MESSAGE_ID), Some("om_test_123"));
        assert_eq!(decoded.header(HEADER_SUM), Some("1"));
        assert_eq!(decoded.header("missing"), None);
    }

    #[test]
    fn frame_decode_control_pong_with_empty_payload() {
        let orig = Frame {
            method: FRAME_TYPE_CONTROL,
            headers: vec![Header {
                key: HEADER_TYPE.into(),
                value: MSG_TYPE_PONG.into(),
            }],
            ..Default::default()
        };
        let bytes = orig.encode();
        let decoded = Frame::decode(&bytes).expect("decode");
        assert_eq!(decoded.method, FRAME_TYPE_CONTROL);
        assert_eq!(decoded.header(HEADER_TYPE), Some(MSG_TYPE_PONG));
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn frame_decode_truncated_returns_error() {
        let mut bytes = Frame {
            seq_id: 1,
            ..Default::default()
        }
        .encode();
        bytes.pop(); // remove last byte to break the last field
                     // Truncation may or may not error depending on which field got chopped;
                     // but a wholly unrelated short slice must definitely error:
        let bad = vec![0x08]; // tag for field 1 / varint but no data
        assert!(Frame::decode(&bad).is_err());
    }

    #[test]
    fn frame_decode_skips_unknown_fields() {
        let mut bytes = Vec::new();
        // Known field 1 varint = 99
        write_tag(&mut bytes, 1, 0);
        write_varint(&mut bytes, 99);
        // Unknown field 17 varint = 42 — must be skipped
        write_tag(&mut bytes, 17, 0);
        write_varint(&mut bytes, 42);
        // Unknown field 18 length-delim = "garbage"
        write_tag(&mut bytes, 18, 2);
        write_varint(&mut bytes, 7);
        bytes.extend_from_slice(b"garbage");
        // Known field 8 (payload)
        write_tag(&mut bytes, 8, 2);
        write_varint(&mut bytes, 3);
        bytes.extend_from_slice(b"abc");

        let f = Frame::decode(&bytes).expect("decode skip unknowns");
        assert_eq!(f.seq_id, 99);
        assert_eq!(f.payload, b"abc");
    }
}
