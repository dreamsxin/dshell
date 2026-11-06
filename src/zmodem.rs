//! ZMODEM protocol (per the original ZMODEM specification by Chuck
//! Forsberg, matching the lrzsz wire behavior).
//!
//! The receive side covers the common "download via ZMODEM" workflow
//! triggered by running `sz <file>` on the remote host; the send side
//! ([`ZmodemSender`]) covers "upload via ZMODEM" towards a remote `rz`.
//! Frames are parsed byte by byte so the parser can be fed directly from
//! the SSH data stream; responses are built as raw byte frames that the
//! caller writes to the SSH channel.
//!
//! Control characters (below 0x20, plus 0x7F, 0x8D, and ZDLE itself) are
//! ZDLE-escaped on the wire: ZDLE followed by the byte XOR 0x40. CRC-16
//! (CCITT reflected, a.k.a. KERMIT, init 0) covers hex-header payloads and
//! subpackets; binary headers with the ZBIN prefix use CRC-16 and ZBIN32
//! uses CRC-32.
//!
//! Files are capped at [`MAX_ZMODEM_FILE_BYTES`] (1 MiB) to bound memory
//! use; larger transfers abort with an error event.

/// CRC-16/KERMIT (poly 0x1021 reflected as 0x8408, init 0, refin/refout).
/// Matches lrzsz `updcrc`; the low byte is transmitted first on the wire.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x8408
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Standard zlib-compatible CRC-32, used for ZBIN32 frames.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

pub const ZPAD: u8 = b'*';
pub const ZDLE: u8 = 0x18;
pub const ZHEX: u8 = b'B';
pub const ZBIN: u8 = b'A';
pub const ZBIN32: u8 = b'C';

pub const ZRQINIT: u8 = 0;
pub const ZRINIT: u8 = 1;
pub const ZACK: u8 = 3;
pub const ZFILE: u8 = 4;
pub const ZSKIP: u8 = 5;
pub const ZFIN: u8 = 8;
pub const ZRPOS: u8 = 9;
pub const ZDATA: u8 = 10;
pub const ZEOF: u8 = 11;
pub const ZCAN: u8 = 16;

pub const ZCRCE: u8 = 0x68; // 'h' data ends; CRC follows
pub const ZCRCG: u8 = 0x69; // 'i' data continues, no ACK needed
pub const ZCRCQ: u8 = 0x6A; // 'j' data continues, sender wants ZACK
pub const ZCRCW: u8 = 0x6B; // 'k' data ends, sender waits for ZACK

/// Maximum accepted file size (1 MiB).
pub const MAX_ZMODEM_FILE_BYTES: usize = 1024 * 1024;
/// Maximum bytes buffered while parsing a single subpacket.
const MAX_SUBPACKET_BYTES: usize = 2 * 1024;

/// Events produced by the receive parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZmodemEvent {
    /// Remote sender announced itself; caller should reply with ZRINIT.
    ZRqInit,
    /// Receiver capabilities acknowledged (sender flow: start offering files).
    ZRinit,
    /// Receiver requested (re)start at the given offset (sender flow).
    ZRpos(u32),
    /// Receiver acknowledged up to the given offset.
    ZAck(u32),
    /// Receiver refused the offered file.
    ZSkip,
    /// File metadata received: (name, declared size in bytes).
    ZFile { name: String, size: Option<u64> },
    /// File data chunk received.
    ZData(Vec<u8>),
    /// Sender finished a file: declared size for verification.
    ZEof(u64),
    /// Remote ended the session; caller should reply with ZFIN.
    ZFin,
    /// Protocol violation or aborted transfer.
    Error(String),
}

/// Internal parser phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Scanning for a frame start (ZPAD).
    Scan,
    /// Saw ZPAD; waiting for ZDLE (binary/hex) or second ZPAD.
    PadSeen,
    /// Saw ZPAD ZDLE; waiting for header type char.
    TypeSeen,
    /// Reading hex header payload (ASCII hex chars until CR).
    HexHeader,
    /// Reading escaped binary header payload.
    BinHeader,
    /// Reading escaped subpacket data after a ZFILE/ZDATA header.
    Subpacket,
    /// Reading the 2 CRC bytes after a subpacket terminator.
    SubCrc,
    /// Skipping the CR LF XON tail that follows a header frame.
    HeaderTail,
}

/// Byte-by-byte ZMODEM receive parser.
#[derive(Debug)]
pub struct ZmodemReceiver {
    phase: Phase,
    /// Header kind seen for the current frame (ZHEX / ZBIN / ZBIN32).
    header_kind: u8,
    hex_buffer: Vec<u8>,
    bin_buffer: Vec<u8>,
    subpacket: Vec<u8>,
    crc_bytes: Vec<u8>,
    subpacket_terminator: u8,
    /// True while the previous byte was ZDLE (next byte is XOR 0x40).
    escaped: bool,
    /// Filename collected from the ZFILE subpacket.
    file_name: String,
    file_size: Option<u64>,
    file_data: Vec<u8>,
    /// True once a ZFILE header has been seen for the current file.
    file_pending: bool,
}

impl Default for ZmodemReceiver {
    fn default() -> Self {
        Self::new()
    }
}

impl ZmodemReceiver {
    pub fn new() -> Self {
        Self {
            phase: Phase::Scan,
            header_kind: 0,
            hex_buffer: Vec::new(),
            bin_buffer: Vec::new(),
            subpacket: Vec::new(),
            crc_bytes: Vec::new(),
            subpacket_terminator: 0,
            escaped: false,
            file_name: String::new(),
            file_size: None,
            file_data: Vec::new(),
            file_pending: false,
        }
    }

    /// Filename accumulated from the current ZFILE header.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Bytes written so far for the receiving file.
    pub fn received_bytes(&self) -> usize {
        self.file_data.len()
    }

    /// Take ownership of the accumulated file data (used when ZEOF arrives).
    pub fn take_file_data(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.file_data)
    }

    /// Reset per-file state while keeping the parser ready for the next file.
    pub fn reset_for_next_file(&mut self) {
        self.file_name.clear();
        self.file_size = None;
        self.file_data.clear();
        self.file_pending = false;
        self.phase = Phase::Scan;
    }

    /// Feed one raw byte from the SSH channel. Returns an event when a
    /// complete protocol step has been recognized.
    pub fn feed(&mut self, byte: u8) -> Option<ZmodemEvent> {
        match self.phase {
            Phase::Scan => self.feed_scan(byte),
            Phase::PadSeen => self.feed_pad_seen(byte),
            Phase::TypeSeen => self.feed_type_seen(byte),
            Phase::HexHeader => self.feed_hex_header(byte),
            Phase::BinHeader => self.feed_bin_header(byte),
            Phase::Subpacket => self.feed_subpacket(byte),
            Phase::SubCrc => self.feed_sub_crc(byte),
            Phase::HeaderTail => self.feed_header_tail(byte),
        }
    }

    /// Discard the CR LF XON tail that terminates a header frame; the first
    /// non-tail byte begins the subpacket that follows a ZFILE/ZDATA header.
    fn feed_header_tail(&mut self, byte: u8) -> Option<ZmodemEvent> {
        if matches!(byte, 0x0D | 0x0A | 0x11) {
            return None;
        }
        self.phase = Phase::Subpacket;
        self.feed_subpacket(byte)
    }

    fn feed_scan(&mut self, byte: u8) -> Option<ZmodemEvent> {
        if byte == ZPAD {
            self.phase = Phase::PadSeen;
        }
        None
    }

    fn feed_pad_seen(&mut self, byte: u8) -> Option<ZmodemEvent> {
        match byte {
            ZPAD => None, // stay in PadSeen waiting for ZDLE
            ZDLE => {
                self.phase = Phase::TypeSeen;
                None
            }
            _ => {
                self.phase = Phase::Scan;
                None
            }
        }
    }

    fn feed_type_seen(&mut self, byte: u8) -> Option<ZmodemEvent> {
        match byte {
            ZHEX => {
                self.header_kind = ZHEX;
                self.hex_buffer.clear();
                self.phase = Phase::HexHeader;
            }
            ZBIN | ZBIN32 => {
                self.header_kind = byte;
                self.bin_buffer.clear();
                self.phase = Phase::BinHeader;
            }
            _ => {
                self.phase = Phase::Scan;
            }
        }
        None
    }

    fn feed_hex_header(&mut self, byte: u8) -> Option<ZmodemEvent> {
        // Hex header payload: 2 (type) + 8 (pos) + 4 (crc) hex chars, then CR.
        if byte == b'\r' || byte == b'\n' {
            return self.complete_hex_header();
        }
        if !byte.is_ascii_hexdigit() {
            self.phase = Phase::Scan;
            return None;
        }
        self.hex_buffer.push(byte);
        if self.hex_buffer.len() == 14 {
            return self.complete_hex_header();
        }
        None
    }

    fn complete_hex_header(&mut self) -> Option<ZmodemEvent> {
        let hex = String::from_utf8_lossy(&self.hex_buffer).into_owned();
        self.phase = Phase::Scan;
        let type_byte = u8::from_str_radix(hex.get(0..2)?, 16).ok()?;
        let mut payload = Vec::with_capacity(4);
        for index in 0..4 {
            let start = 2 + index * 2;
            payload.push(u8::from_str_radix(hex.get(start..start + 2)?, 16).ok()?);
        }
        self.dispatch_header(type_byte, &payload)
    }

    fn feed_bin_header(&mut self, byte: u8) -> Option<ZmodemEvent> {
        let Some(decoded) = self.decode(byte) else {
            return None;
        };
        self.bin_buffer.push(decoded);
        // Binary header: type(1) + pos(4) + crc(2 for 16, 4 for 32).
        let expected = if self.header_kind == ZBIN32 { 9 } else { 7 };
        if self.bin_buffer.len() == expected {
            let buffer = std::mem::take(&mut self.bin_buffer);
            let type_byte = buffer[0];
            let payload = buffer[1..5].to_vec();
            self.phase = Phase::Scan;
            return self.dispatch_header(type_byte, &payload);
        }
        None
    }

    fn dispatch_header(&mut self, type_byte: u8, payload: &[u8]) -> Option<ZmodemEvent> {
        let position = u32::from_be_bytes([
            payload.first().copied().unwrap_or(0),
            payload.get(1).copied().unwrap_or(0),
            payload.get(2).copied().unwrap_or(0),
            payload.get(3).copied().unwrap_or(0),
        ]);
        match type_byte {
            ZRQINIT => Some(ZmodemEvent::ZRqInit),
            ZRINIT => Some(ZmodemEvent::ZRinit),
            ZACK => Some(ZmodemEvent::ZAck(position)),
            ZRPOS => Some(ZmodemEvent::ZRpos(position)),
            ZSKIP => Some(ZmodemEvent::ZSkip),
            ZFIN => Some(ZmodemEvent::ZFin),
            ZFILE => {
                self.file_name.clear();
                self.file_size = None;
                self.file_data.clear();
                self.file_pending = true;
                self.subpacket.clear();
                self.phase = Phase::HeaderTail;
                None
            }
            ZDATA => {
                self.subpacket.clear();
                self.phase = Phase::HeaderTail;
                None
            }
            ZEOF => Some(ZmodemEvent::ZEof(u64::from(position))),
            ZCAN => Some(ZmodemEvent::Error("Transfer cancelled by remote".into())),
            _ => None,
        }
    }

    fn feed_subpacket(&mut self, byte: u8) -> Option<ZmodemEvent> {
        // Only a ZDLE-escaped byte in the terminator range ends the subpacket;
        // raw printable bytes (including 0x68 'h') are subpacket data.
        if byte == ZDLE {
            self.escaped = true;
            return None;
        }
        if self.escaped {
            self.escaped = false;
            let decoded = byte ^ 0x40;
            if (ZCRCE..=ZCRCW).contains(&decoded) {
                self.subpacket_terminator = decoded;
                self.crc_bytes.clear();
                self.phase = Phase::SubCrc;
                return None;
            }
            self.subpacket.push(decoded);
        } else {
            self.subpacket.push(byte);
        }
        if self.subpacket.len() > MAX_SUBPACKET_BYTES {
            self.phase = Phase::Scan;
            return Some(ZmodemEvent::Error("Subpacket too large".into()));
        }
        None
    }

    fn feed_sub_crc(&mut self, byte: u8) -> Option<ZmodemEvent> {
        let Some(decoded) = self.decode(byte) else {
            return None;
        };
        self.crc_bytes.push(decoded);
        if self.crc_bytes.len() < 2 {
            return None;
        }
        // Subpacket CRC-16 arrives least significant byte first.
        let wire_crc = (u16::from(self.crc_bytes[1]) << 8) | u16::from(self.crc_bytes[0]);
        let mut crc_input = std::mem::take(&mut self.subpacket);
        crc_input.push(self.subpacket_terminator);
        // ZCRCG/ZCRCQ announce more subpackets in the same ZDATA frame; any
        // other terminator ends the frame and the next byte starts a header.
        self.phase = if matches!(self.subpacket_terminator, ZCRCG | ZCRCQ) {
            Phase::Subpacket
        } else {
            Phase::Scan
        };
        if crc16(&crc_input) != wire_crc {
            return Some(ZmodemEvent::Error("CRC-16 mismatch".into()));
        }
        Some(self.finish_chunk(&crc_input[..crc_input.len() - 1]))
    }

    fn finish_chunk(&mut self, data: &[u8]) -> ZmodemEvent {
        if self.file_pending {
            // First subpacket after ZFILE: "name size date mode ..." text.
            self.file_pending = false;
            let text = String::from_utf8_lossy(data).into_owned();
            let mut fields = text.split_whitespace();
            let name = fields.next().unwrap_or_default().to_owned();
            let size = fields.next().and_then(|value| value.parse::<u64>().ok());
            self.file_name = name;
            self.file_size = size;
            return ZmodemEvent::ZFile {
                name: self.file_name.clone(),
                size,
            };
        }
        if self.file_data.len() + data.len() > MAX_ZMODEM_FILE_BYTES {
            return ZmodemEvent::Error(format!(
                "File exceeds the {} byte ZMODEM receive limit",
                MAX_ZMODEM_FILE_BYTES
            ));
        }
        self.file_data.extend_from_slice(data);
        ZmodemEvent::ZData(data.to_vec())
    }

    /// Decode one wire byte honoring ZDLE escape state. Returns None while
    /// the escape sequence is incomplete (byte was ZDLE itself).
    fn decode(&mut self, byte: u8) -> Option<u8> {
        if self.escaped {
            self.escaped = false;
            return Some(byte ^ 0x40);
        }
        if byte == ZDLE {
            self.escaped = true;
            return None;
        }
        Some(byte)
    }
}

/// Escape a byte for ZMODEM transmission: control characters, 0x7F, 0x8D,
/// and ZDLE itself are sent as ZDLE followed by the byte XOR 0x40.
fn escape_byte(byte: u8) -> Vec<u8> {
    let needs_escape = byte < 0x20 || byte == 0x7F || byte == ZDLE || byte == 0x8D;
    if needs_escape {
        vec![ZDLE, byte ^ 0x40]
    } else {
        vec![byte]
    }
}

fn escape_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 8);
    for byte in data {
        out.extend(escape_byte(*byte));
    }
    out
}

fn build_binary_header(frame_type: u8, position: u32) -> Vec<u8> {
    let mut payload = vec![frame_type];
    payload.extend_from_slice(&position.to_be_bytes());
    let crc = crc16(&payload);
    payload.push((crc >> 8) as u8);
    payload.push((crc & 0xFF) as u8);
    let mut out = vec![ZPAD, ZDLE, ZBIN];
    out.extend(escape_bytes(&payload));
    out
}

fn build_subpacket(data: &[u8], terminator: u8) -> Vec<u8> {
    let mut crc_input = data.to_vec();
    crc_input.push(terminator);
    let crc = crc16(&crc_input);
    let mut out = escape_bytes(data);
    // The subpacket terminator is ALWAYS ZDLE-escaped (ZDLE + term XOR 0x40)
    // so raw data bytes in the terminator range stay unambiguous.
    out.push(ZDLE);
    out.push(terminator ^ 0x40);
    // Subpacket CRC-16 is transmitted least significant byte first.
    out.extend(escape_byte((crc & 0xFF) as u8));
    out.extend(escape_byte((crc >> 8) as u8));
    out
}

fn header_frame(frame_type: u8, position: u32) -> Vec<u8> {
    let mut frame = build_binary_header(frame_type, position);
    frame.push(b'\r');
    frame.push(b'\n');
    frame
}

/// Build the ZRINIT response (can full-duplex, CRC-32 capable).
pub fn build_zrinit() -> Vec<u8> {
    // Flags: F3=0x02 (CANFC32), F0=0x40 (CANFDX full duplex).
    header_frame(ZRINIT, 0x0000_0042)
}

/// Build a ZRPOS response requesting transfer from the given offset.
pub fn build_zrpos(offset: u32) -> Vec<u8> {
    header_frame(ZRPOS, offset)
}

/// Build a ZACK response for the given offset.
pub fn build_zack(position: u32) -> Vec<u8> {
    header_frame(ZACK, position)
}

/// Build a ZFIN response (session close).
pub fn build_zfin() -> Vec<u8> {
    header_frame(ZFIN, 0)
}

/// Build a ZSKIP response to reject a file.
pub fn build_zskip() -> Vec<u8> {
    header_frame(ZSKIP, 0)
}

/// Build a ZRQINIT-shaped frame (test and probe helper).
pub fn build_zrqinit() -> Vec<u8> {
    header_frame(ZRQINIT, 0)
}

/// Build a hex-encoded header (matches the sender's initial ZRQINIT shape).
pub fn build_hex_header(frame_type: u8, position: u32) -> Vec<u8> {
    let mut payload = vec![frame_type];
    payload.extend_from_slice(&position.to_be_bytes());
    let crc = crc16(&payload);
    let mut hex_bytes = payload;
    hex_bytes.push((crc >> 8) as u8);
    hex_bytes.push((crc & 0xFF) as u8);
    let mut out = vec![ZPAD, ZDLE, ZHEX];
    for byte in hex_bytes {
        out.extend(format!("{byte:02x}").into_bytes());
    }
    out.push(b'\r');
    out.push(b'\n');
    out
}

/// Build a ZFILE/ZDATA subpacket ending in ZCRCW (wait-for-ACK).
pub fn build_subpacket_zcrcw(data: &[u8]) -> Vec<u8> {
    build_subpacket(data, ZCRCW)
}

/// Bytes per data subpacket when sending files (1 KiB of payload; up to
/// 4 KiB on the wire after ZDLE escaping).
const SEND_CHUNK_BYTES: usize = 1024;

/// One action the transfer driver should perform after feeding bytes to the
/// sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZmodemSendStep {
    /// Write these protocol bytes to the SSH channel.
    Write(Vec<u8>),
    /// The file was delivered and the session closed cleanly.
    Done,
    /// The transfer was aborted with a human-readable reason.
    Aborted(String),
}

/// Sender-side transfer state. The remote host runs `rz`; this side offers
/// one file per transfer, streams its bytes in subpackets and closes the
/// session. Incoming protocol bytes are parsed with the same frame parser
/// the receive path uses.
#[derive(Debug)]
pub struct ZmodemSender {
    receiver: ZmodemReceiver,
    file_name: String,
    data: Vec<u8>,
    phase: SendPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendPhase {
    /// ZRQINIT sent; waiting for the receiver's ZRINIT.
    Announced,
    /// ZFILE offered; waiting for ZRPOS or ZSKIP.
    FileOffered,
    /// ZDATA and ZEOF sent; waiting for the final ZACK.
    DataSent,
    /// ZFIN sent; waiting for the receiver's ZFIN.
    FinSent,
}

impl ZmodemSender {
    /// Prepare a transfer of `data` under `path`'s final path segment.
    /// Enforce the size limit here so both call sites and tests share it.
    pub fn new(path: &str, data: Vec<u8>) -> Result<Self, String> {
        if data.len() > MAX_ZMODEM_FILE_BYTES {
            return Err(format!(
                "File exceeds the {} byte ZMODEM send limit",
                MAX_ZMODEM_FILE_BYTES
            ));
        }
        let file_name = path
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty() && *name != "." && *name != "..")
            .unwrap_or("zmodem-file")
            .to_owned();
        Ok(Self {
            receiver: ZmodemReceiver::new(),
            file_name,
            data,
            phase: SendPhase::Announced,
        })
    }

    /// Frames to send when starting the transfer: the ZRQINIT announcement.
    pub fn start(&self) -> Vec<u8> {
        build_hex_header(ZRQINIT, 0)
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Feed one incoming byte from the SSH channel and return the actions
    /// the driver should perform.
    pub fn feed(&mut self, byte: u8) -> Vec<ZmodemSendStep> {
        let mut steps = Vec::new();
        let Some(event) = self.receiver.feed(byte) else {
            return steps;
        };
        match (self.phase, event) {
            (SendPhase::Announced, ZmodemEvent::ZRinit) => {
                // Offer the file: ZFILE header (flags 0) plus a
                // "name size" subpacket that waits for the receiver.
                let metadata = format!("{} {}", self.file_name, self.data.len());
                let mut frames = header_frame(ZFILE, 0);
                frames.extend(build_subpacket(metadata.as_bytes(), ZCRCW));
                self.phase = SendPhase::FileOffered;
                steps.push(ZmodemSendStep::Write(frames));
            }
            (SendPhase::FileOffered, ZmodemEvent::ZRpos(offset)) => {
                // Stream the payload: ZDATA header, 1 KiB subpackets —
                // ZCRCG for continuations, ZCRCE for the last — then ZEOF
                // with the total size.
                let mut frames = header_frame(ZDATA, offset);
                let chunk_count = self.data.len().div_ceil(SEND_CHUNK_BYTES);
                for (index, chunk) in self.data.chunks(SEND_CHUNK_BYTES).enumerate() {
                    let terminator = if index + 1 == chunk_count {
                        ZCRCE
                    } else {
                        ZCRCG
                    };
                    frames.extend(build_subpacket(chunk, terminator));
                }
                frames.extend(header_frame(
                    ZEOF,
                    u32::try_from(self.data.len()).unwrap_or(u32::MAX),
                ));
                self.phase = SendPhase::DataSent;
                steps.push(ZmodemSendStep::Write(frames));
            }
            (SendPhase::FileOffered, ZmodemEvent::ZSkip) => {
                steps.push(ZmodemSendStep::Aborted(format!(
                    "The remote host skipped {}",
                    self.file_name
                )));
            }
            (SendPhase::DataSent, ZmodemEvent::ZAck(_)) => {
                // The receiver confirmed the whole file; close the session.
                self.phase = SendPhase::FinSent;
                steps.push(ZmodemSendStep::Write(build_zfin()));
            }
            (SendPhase::FinSent, ZmodemEvent::ZFin) => {
                // Session close: "OO" ends the exchange.
                steps.push(ZmodemSendStep::Write(b"OO".to_vec()));
                steps.push(ZmodemSendStep::Done);
            }
            (
                _,
                ZmodemEvent::ZRqInit
                | ZmodemEvent::ZRinit
                | ZmodemEvent::ZRpos(_)
                | ZmodemEvent::ZAck(_)
                | ZmodemEvent::ZSkip
                | ZmodemEvent::ZFile { .. }
                | ZmodemEvent::ZData(_)
                | ZmodemEvent::ZEof(_)
                | ZmodemEvent::ZFin,
            ) => {
                // Frames that do not advance the sender's current phase are
                // ignored; the parser resynchronizes on the next frame.
            }
            (_, ZmodemEvent::Error(message)) => {
                steps.push(ZmodemSendStep::Aborted(message));
            }
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_matches_lrzsz_updcrc_vector() {
        // Catalog check values are for the 9-byte string "123456789".
        assert_eq!(crc16(b"123456789"), 0x2189);
        assert_eq!(crc16(b"1234567890"), 0x286B);
        assert_eq!(crc16(b""), 0);
    }

    #[test]
    fn crc32_matches_zlib_vector() {
        // zlib.crc32(b"1234567890") == 0x261D_AEE5.
        assert_eq!(crc32(b"1234567890"), 0x261D_AEE5);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn zrqinit_frame_round_trips_through_parser() {
        for frame in [build_zrqinit(), build_hex_header(ZRQINIT, 0)] {
            let mut receiver = ZmodemReceiver::new();
            let mut events = Vec::new();
            for byte in frame {
                if let Some(event) = receiver.feed(byte) {
                    events.push(event);
                }
            }
            assert_eq!(events, vec![ZmodemEvent::ZRqInit]);
        }
    }

    #[test]
    fn zfile_data_and_eof_flow_produces_file_bytes() {
        let file_body = b"hello zmodem\n";
        let mut stream = Vec::new();
        stream.extend(build_hex_header(ZFILE, 0));
        stream.extend(build_subpacket(b"hello.txt 13", ZCRCE));
        stream.extend(build_hex_header(ZDATA, 0));
        stream.extend(build_subpacket(file_body, ZCRCW));
        stream.extend(build_hex_header(ZEOF, 13));

        let mut receiver = ZmodemReceiver::new();
        let mut events = Vec::new();
        for byte in stream {
            if let Some(event) = receiver.feed(byte) {
                events.push(event);
            }
        }
        assert!(events.contains(&ZmodemEvent::ZFile {
            name: "hello.txt".to_owned(),
            size: Some(13),
        }));
        assert!(events.contains(&ZmodemEvent::ZData(file_body.to_vec())));
        assert!(events.contains(&ZmodemEvent::ZEof(13)));
        assert_eq!(receiver.file_name(), "hello.txt");
        assert_eq!(receiver.received_bytes(), 13);
        assert_eq!(receiver.take_file_data(), file_body.to_vec());
    }

    #[test]
    fn corrupted_subpacket_reports_crc_error() {
        let mut stream = Vec::new();
        stream.extend(build_hex_header(ZFILE, 0));
        stream.extend(build_subpacket(b"hello.txt 5", ZCRCE));
        stream.extend(build_hex_header(ZDATA, 0));
        let mut corrupted = build_subpacket(b"data", ZCRCW);
        if let Some(last) = corrupted.last_mut() {
            *last = last.wrapping_add(1);
        }
        stream.extend(corrupted);

        let mut receiver = ZmodemReceiver::new();
        let mut events = Vec::new();
        for byte in stream {
            if let Some(event) = receiver.feed(byte) {
                events.push(event);
            }
        }
        assert!(
            events.iter().any(
                |event| matches!(event, ZmodemEvent::Error(message) if message.contains("CRC"))
            )
        );
    }

    #[test]
    fn control_characters_are_zdle_escaped_in_built_frames() {
        // ZRPOS offset 0 contains NUL bytes which must be escaped on the wire.
        let frame = build_zrpos(0);
        let escaped_pairs = frame.windows(2).filter(|pair| pair[0] == ZDLE).count();
        assert!(escaped_pairs >= 5);
        // No raw NUL bytes may appear on the wire.
        assert!(!frame.contains(&0x00));
    }

    /// Run one side of a transfer against the other by feeding the steps
    /// each produces. Returns the events the parser recognized.
    fn drive_into_receiver(sender: &mut ZmodemSender, bytes: &[u8]) -> Vec<ZmodemEvent> {
        let mut steps = Vec::new();
        for &byte in bytes {
            steps.extend(sender.feed(byte));
        }
        let mut events = Vec::new();
        let mut receiver = ZmodemReceiver::new();
        for step in steps {
            if let ZmodemSendStep::Write(frames) = step {
                for byte in frames {
                    if let Some(event) = receiver.feed(byte) {
                        events.push(event);
                    }
                }
            }
        }
        events
    }

    #[test]
    fn sender_completes_full_transfer_against_receive_parser() {
        let body: Vec<u8> = (0..2500u32).map(|index| (index % 251) as u8).collect();
        let mut sender = ZmodemSender::new("D:\\tmp\\report.bin", body.clone()).unwrap();
        assert_eq!(sender.file_name(), "report.bin");
        // The sender opens with a hex ZRQINIT the receiver understands.
        let mut receiver = ZmodemReceiver::new();
        for byte in sender.start() {
            if let Some(event) = receiver.feed(byte) {
                assert_eq!(event, ZmodemEvent::ZRqInit);
            }
        }

        // Receiver capabilities arrive; the sender offers the file.
        let events = drive_into_receiver(&mut sender, &build_zrinit());
        assert_eq!(
            events,
            vec![ZmodemEvent::ZFile {
                name: "report.bin".to_owned(),
                size: Some(body.len() as u64),
            }]
        );

        // ZRPOS triggers the payload: ZDATA chunks plus ZEOF.
        let events = drive_into_receiver(&mut sender, &build_zrpos(0));
        assert!(events.contains(&ZmodemEvent::ZEof(body.len() as u64)));
        let data_bytes: Vec<u8> = events
            .into_iter()
            .flat_map(|event| match event {
                ZmodemEvent::ZData(chunk) => chunk,
                _ => Vec::new(),
            })
            .collect();
        assert_eq!(data_bytes, body);

        // ZACK closes: the sender sends ZFIN, then answers the receiver's
        // ZFIN with "OO" and reports completion.
        let mut steps = Vec::new();
        for &byte in build_zack(0).iter() {
            steps.extend(sender.feed(byte));
        }
        assert!(matches!(
            steps.first(),
            Some(ZmodemSendStep::Write(frames)) if frames.first() == Some(&ZPAD)
        ));
        let mut steps = Vec::new();
        for &byte in build_zfin().iter() {
            steps.extend(sender.feed(byte));
        }
        assert_eq!(
            steps,
            vec![ZmodemSendStep::Write(b"OO".to_vec()), ZmodemSendStep::Done,]
        );
    }

    /// Feed bytes to the sender and collect every step without a parser
    /// round trip.
    fn collect_steps(sender: &mut ZmodemSender, bytes: &[u8]) -> Vec<ZmodemSendStep> {
        let mut steps = Vec::new();
        for &byte in bytes {
            steps.extend(sender.feed(byte));
        }
        steps
    }

    #[test]
    fn sender_aborts_when_receiver_skips_the_file() {
        let mut sender = ZmodemSender::new("a.txt", b"hi".to_vec()).unwrap();
        let steps = collect_steps(&mut sender, &build_zrinit());
        // ZRINIT advances to the file offer: exactly one Write step.
        assert_eq!(steps.len(), 1);
        assert!(matches!(steps[0], ZmodemSendStep::Write(_)));
        let steps = collect_steps(&mut sender, &header_frame(ZSKIP, 0));
        assert!(matches!(
            steps.as_slice(),
            [ZmodemSendStep::Aborted(message)] if message.contains("a.txt")
        ));
    }

    #[test]
    fn sender_aborts_on_receiver_cancel_and_rejects_oversize_files() {
        let mut sender = ZmodemSender::new("a.txt", b"hi".to_vec()).unwrap();
        for &byte in build_zrinit().iter() {
            sender.feed(byte);
        }
        let mut steps = Vec::new();
        for &byte in header_frame(ZCAN, 0).iter() {
            steps.extend(sender.feed(byte));
        }
        assert!(matches!(
            steps.as_slice(),
            [ZmodemSendStep::Aborted(message)] if message.contains("cancelled")
        ));

        let oversize = vec![0u8; MAX_ZMODEM_FILE_BYTES + 1];
        assert!(ZmodemSender::new("big.bin", oversize).is_err());
    }
}
