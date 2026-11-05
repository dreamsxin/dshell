//! iTerm2 inline image protocol (OSC 1337) collector.
//!
//! Remote programs can print images with the escape sequence
//! `ESC ] 1337 ; File=name=<b64>;size=N;inline=1:<base64 data> BEL`.
//! dshell does not render images inside the terminal grid (the VT parser
//! consumes the sequence silently); instead the decoded bytes are saved to
//! the local Downloads folder and the user is notified, preserving the
//! image without ever garbling the terminal.
//!
//! The collector is fed byte by byte from the SSH stream so sequences
//! spanning SSH packet boundaries are handled correctly.

use std::path::PathBuf;

/// Maximum accepted image size (16 MiB) to bound memory use.
pub const MAX_INLINE_IMAGE_BYTES: usize = 16 * 1024 * 1024;

const OSC_PREFIX: &[u8] = b"\x1b]1337;File=";

/// A decoded inline image ready to be written to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineImageFile {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Matching the OSC 1337 File prefix byte by byte.
    Scan,
    /// Inside the payload; accumulating until BEL or ST (ESC \).
    Collect,
    /// Saw ESC inside the payload; the next byte must be `\` to finish.
    CollectEsc,
}

/// Byte-by-byte collector for OSC 1337 inline images.
#[derive(Debug)]
pub struct InlineImageCollector {
    phase: Phase,
    prefix_index: usize,
    buffer: Vec<u8>,
    counter: u64,
}

impl Default for InlineImageCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl InlineImageCollector {
    pub fn new() -> Self {
        Self {
            phase: Phase::Scan,
            prefix_index: 0,
            buffer: Vec::new(),
            counter: 0,
        }
    }

    /// Feed one wire byte. Returns a decoded image when a complete OSC 1337
    /// sequence has been received. The parser auto-resynchronizes after
    /// malformed or oversized payloads.
    pub fn feed(&mut self, byte: u8) -> Option<InlineImageFile> {
        match self.phase {
            Phase::Scan => {
                if byte == OSC_PREFIX[self.prefix_index] {
                    self.prefix_index += 1;
                    if self.prefix_index == OSC_PREFIX.len() {
                        self.phase = Phase::Collect;
                        self.buffer.clear();
                    }
                } else {
                    // Resynchronize: the current byte may start a new prefix.
                    self.prefix_index = if byte == OSC_PREFIX[0] { 1 } else { 0 };
                }
                None
            }
            Phase::Collect => {
                if byte == 0x07 {
                    // BEL terminator.
                    return self.complete();
                }
                if byte == 0x1B {
                    self.phase = Phase::CollectEsc;
                    return None;
                }
                self.buffer.push(byte);
                if self.buffer.len() > MAX_INLINE_IMAGE_BYTES {
                    self.reset();
                }
                None
            }
            Phase::CollectEsc => {
                if byte == b'\\' {
                    return self.complete();
                }
                // Not a terminator; the ESC and this byte are payload.
                self.buffer.push(0x1B);
                self.buffer.push(byte);
                self.phase = Phase::Collect;
                if self.buffer.len() > MAX_INLINE_IMAGE_BYTES {
                    self.reset();
                }
                None
            }
        }
    }

    fn reset(&mut self) {
        self.phase = Phase::Scan;
        self.prefix_index = 0;
        self.buffer.clear();
    }

    fn complete(&mut self) -> Option<InlineImageFile> {
        let payload = String::from_utf8_lossy(&self.buffer).into_owned();
        self.reset();
        let (name, data) = parse_osc1337_payload(&payload)?;
        self.counter = self.counter.wrapping_add(1);
        Some(InlineImageFile { name, data })
    }
}

/// Parse an OSC 1337 `File=...` payload: metadata before the first `:`,
/// base64 image data after it. Returns (file name, decoded bytes).
fn parse_osc1337_payload(payload: &str) -> Option<(String, Vec<u8>)> {
    // The collector's scan prefix already consumed "File="; accept both
    // forms defensively.
    let rest = payload.strip_prefix("File=").unwrap_or(payload);
    let (metadata, encoded) = rest.split_once(':')?;
    let mut name = String::new();
    let mut has_inline = false;
    for field in metadata.split(';') {
        if let Some(value) = field.strip_prefix("name=") {
            name = percent_decode(value);
        } else if field == "inline=1" {
            has_inline = true;
        }
    }
    if !has_inline && !metadata.contains("inline=") {
        // Non-inline payloads (e.g. remote file downloads) are skipped.
        return None;
    }
    let cleaned: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return None;
    }
    use data_encoding::BASE64;
    let data = BASE64.decode(cleaned.as_bytes()).map_err(|_| ()).ok()?;
    if data.is_empty() {
        return None;
    }
    Some((sanitize_inline_image_name(&name), data))
}

/// Percent-decode an iTerm2 file name (the spec encodes reserved chars).
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() + 1 {
            let hex = value.get(index + 1..index + 3);
            if let Some(hex) = hex
                && let Ok(decoded) = u8::from_str_radix(hex, 16)
            {
                out.push(decoded);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Keep only the final path segment of an image name; fall back to a
/// timestamped default for empty or traversal-only names.
fn sanitize_inline_image_name(name: &str) -> String {
    let candidate = name.rsplit(['/', '\\']).next().unwrap_or_default();
    if candidate.is_empty() || candidate == "." || candidate == ".." {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        return format!("inline-image-{stamp}.bin");
    }
    candidate.to_owned()
}

/// Compute the download path for a received inline image.
pub fn inline_image_download_path(name: &str) -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let downloads = home.join("Downloads");
    if downloads.is_dir() {
        downloads.join(name)
    } else {
        home.join(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(collector: &mut InlineImageCollector, bytes: &[u8]) -> Option<InlineImageFile> {
        let mut result = None;
        for &byte in bytes {
            if let Some(file) = collector.feed(byte) {
                result = Some(file);
            }
        }
        result
    }

    fn build_osc1337(name: &str, data: &[u8]) -> Vec<u8> {
        use data_encoding::BASE64;
        let mut out = OSC_PREFIX.to_vec();
        out.extend_from_slice(format!("name={};size={};inline=1:", name, data.len()).as_bytes());
        out.extend_from_slice(BASE64.encode(data).as_bytes());
        out.push(0x07);
        out
    }

    #[test]
    fn collects_image_spanning_chunk_boundaries() {
        let png = b"\x89PNG fake image bytes";
        let frame = build_osc1337("chart.png", png);
        let mut collector = InlineImageCollector::new();
        // Feed in 3-byte chunks to prove cross-chunk handling.
        let mut last = None;
        for chunk in frame.chunks(3) {
            if let Some(file) = feed_all(&mut collector, chunk) {
                last = Some(file);
            }
        }
        let file = last.expect("image completed");
        assert_eq!(file.name, "chart.png");
        assert_eq!(file.data, png.to_vec());
    }

    #[test]
    fn parses_payload_metadata_and_decodes_base64() {
        use data_encoding::BASE64;
        let payload = format!(
            "name=report%2Epdf;size=5;inline=1:{}",
            BASE64.encode(b"hello")
        );
        let (name, data) = parse_osc1337_payload(&payload).unwrap();
        assert_eq!(name, "report.pdf");
        assert_eq!(data, b"hello".to_vec());
    }

    #[test]
    fn rejects_payloads_without_inline_flag_or_data() {
        assert!(parse_osc1337_payload("File=name=x.png;size=1:AAAA").is_none());
        assert!(parse_osc1337_payload("File=name=x.png;inline=1:").is_none());
        assert!(parse_osc1337_payload("Malformed").is_none());
    }

    #[test]
    fn st_terminated_sequences_complete() {
        let png = b"st-terminated";
        use data_encoding::BASE64;
        let mut frame = OSC_PREFIX.to_vec();
        frame.extend_from_slice(b"inline=1:");
        frame.extend_from_slice(BASE64.encode(png).as_bytes());
        frame.extend_from_slice(b"\x1b\\");
        let mut collector = InlineImageCollector::new();
        let file = feed_all(&mut collector, &frame).expect("image completed");
        assert_eq!(file.data, png.to_vec());
    }

    #[test]
    fn names_keep_final_segment_and_fall_back_when_unsafe() {
        assert_eq!(sanitize_inline_image_name("/tmp/a.png"), "a.png");
        assert_eq!(sanitize_inline_image_name("..\\b.jpg"), "b.jpg");
        let fallback = sanitize_inline_image_name("");
        assert!(fallback.starts_with("inline-image-"));
    }
}
