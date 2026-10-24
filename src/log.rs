//! Session log framing.
//!
//! New logs start with [`HEADER`] and contain length-prefixed records:
//! `DSL2` + little-endian u64 timestamp (Unix epoch ms) + little-endian u32
//! payload length + raw terminal bytes. Legacy raw-byte logs remain readable
//! and simply have no timeline.

use std::{
    fs::File,
    io::{Read as _, Seek as _, SeekFrom},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// Current Unix epoch milliseconds, used for log record timestamps.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

pub const HEADER: &[u8; 12] = b"DSHELL-LOG\x02\n";
const SYNC: &[u8; 4] = b"DSL2";
const FRAME_HEADER_BYTES: usize = 16;
const MAX_FRAME_BYTES: u32 = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// Unix epoch milliseconds.
    pub timestamp_ms: u64,
    /// Offset in the concatenated terminal byte stream.
    pub offset: usize,
    pub length: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayLog {
    pub bytes: Vec<u8>,
    pub timeline: Vec<LogEntry>,
    pub truncated: bool,
    pub timestamped: bool,
}

pub fn is_timestamped_header(header: &[u8]) -> bool {
    header == HEADER
}

pub fn frame(timestamp_ms: u64, payload: &[u8]) -> Vec<u8> {
    let length = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    let mut record = Vec::with_capacity(FRAME_HEADER_BYTES + payload.len());
    record.extend_from_slice(SYNC);
    record.extend_from_slice(&timestamp_ms.to_le_bytes());
    record.extend_from_slice(&length.to_le_bytes());
    record.extend_from_slice(payload);
    record
}

/// Reads a log for replay, limiting framed logs to `limit` bytes of tail.
/// Legacy raw logs keep the previous behaviour and start mid-stream when the
/// tail is truncated.
pub fn read_replay(path: &Path, limit: u64) -> Result<ReplayLog, String> {
    let limit = limit.max(1);
    let mut file =
        File::open(path).map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let length = file
        .metadata()
        .map_err(|error| format!("Could not read {} metadata: {error}", path.display()))?
        .len();
    let truncated = length > limit;

    let mut header = [0u8; HEADER.len()];
    let timestamped = if length >= HEADER.len() as u64 {
        file.read_exact(&mut header)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        is_timestamped_header(&header)
    } else {
        false
    };

    if !timestamped {
        let start = length.saturating_sub(limit);
        file.seek(SeekFrom::Start(start))
            .map_err(|error| format!("Could not seek {}: {error}", path.display()))?;
        let mut bytes = Vec::with_capacity(length.min(limit) as usize);
        file.read_to_end(&mut bytes)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        return Ok(ReplayLog {
            bytes,
            timeline: Vec::new(),
            truncated,
            timestamped: false,
        });
    }

    let start = length.saturating_sub(limit).max(HEADER.len() as u64);
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("Could not seek {}: {error}", path.display()))?;
    let mut tail = Vec::with_capacity((length - start) as usize);
    file.read_to_end(&mut tail)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;

    let parsed = if start == HEADER.len() as u64 {
        parse_frames(&tail, 0)
    } else {
        find_frame_boundary(&tail)
    };
    let Some(mut replay) = parsed else {
        return Err(format!(
            "Could not find a complete timestamped record in the tail of {}.",
            path.display()
        ));
    };
    replay.truncated = truncated;
    Ok(replay)
}

fn find_frame_boundary(bytes: &[u8]) -> Option<ReplayLog> {
    let mut search_from = 0;
    while search_from + FRAME_HEADER_BYTES <= bytes.len() {
        let position = bytes[search_from..]
            .windows(SYNC.len())
            .position(|window| window == SYNC)?;
        let candidate = search_from + position;
        if let Some(replay) = parse_frames(bytes, candidate) {
            return Some(replay);
        }
        search_from = candidate + 1;
    }
    None
}

fn parse_frames(bytes: &[u8], start: usize) -> Option<ReplayLog> {
    let mut position = start;
    let mut offset = 0usize;
    let mut payload = Vec::new();
    let mut timeline = Vec::new();
    while position < bytes.len() {
        if bytes.len() - position < FRAME_HEADER_BYTES {
            return None;
        }
        if &bytes[position..position + SYNC.len()] != SYNC {
            return None;
        }
        let timestamp = u64::from_le_bytes(
            bytes[position + 4..position + 12]
                .try_into()
                .expect("frame timestamp"),
        );
        let length = u32::from_le_bytes(
            bytes[position + 12..position + 16]
                .try_into()
                .expect("frame length"),
        );
        if length > MAX_FRAME_BYTES {
            return None;
        }
        let length = length as usize;
        let payload_start = position + FRAME_HEADER_BYTES;
        let payload_end = payload_start.checked_add(length)?;
        if payload_end > bytes.len() {
            return None;
        }
        let chunk = &bytes[payload_start..payload_end];
        if !chunk.is_empty() {
            timeline.push(LogEntry {
                timestamp_ms: timestamp,
                offset,
                length,
            });
            payload.extend_from_slice(chunk);
            offset += length;
        }
        position = payload_end;
    }
    Some(ReplayLog {
        bytes: payload,
        timeline,
        truncated: false,
        timestamped: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framed_logs_round_trip_with_a_timeline() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.log");
        let mut contents = HEADER.to_vec();
        contents.extend(frame(1_000, b"first "));
        contents.extend(frame(1_250, b"second"));
        std::fs::write(&path, &contents).unwrap();

        let replay = read_replay(&path, 1024).unwrap();
        assert!(replay.timestamped);
        assert!(!replay.truncated);
        assert_eq!(replay.bytes, b"first second");
        assert_eq!(
            replay.timeline,
            vec![
                LogEntry {
                    timestamp_ms: 1_000,
                    offset: 0,
                    length: 6
                },
                LogEntry {
                    timestamp_ms: 1_250,
                    offset: 6,
                    length: 6
                }
            ]
        );
    }

    #[test]
    fn legacy_raw_logs_stay_readable_without_a_timeline() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("legacy.log");
        std::fs::write(&path, b"0123456789").unwrap();

        let replay = read_replay(&path, 4).unwrap();
        assert!(!replay.timestamped);
        assert!(replay.truncated);
        assert_eq!(replay.bytes, b"6789");
        assert!(replay.timeline.is_empty());
    }

    #[test]
    fn truncated_timestamped_logs_start_at_a_frame_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.log");
        let mut contents = HEADER.to_vec();
        for (index, payload) in [b"aaaa".as_slice(), b"bbbb", b"cccc", b"dddd"]
            .into_iter()
            .enumerate()
        {
            contents.extend(frame(1_000 + index as u64, payload));
        }
        std::fs::write(&path, &contents).unwrap();

        let replay = read_replay(&path, 40).unwrap();
        assert!(replay.timestamped);
        assert!(replay.truncated);
        assert!(!replay.bytes.is_empty());
        assert!(replay.bytes.len() % 4 == 0);
        assert_eq!(replay.timeline.last().unwrap().length, 4);
        assert_eq!(
            replay.bytes.len(),
            replay
                .timeline
                .iter()
                .map(|entry| entry.length)
                .sum::<usize>()
        );
    }

    #[test]
    fn frame_length_covers_the_payload() {
        let record = frame(42, b"payload");
        assert_eq!(&record[..4], SYNC);
        assert_eq!(u64::from_le_bytes(record[4..12].try_into().unwrap()), 42);
        assert_eq!(u32::from_le_bytes(record[12..16].try_into().unwrap()), 7);
        assert_eq!(&record[16..], b"payload");
    }
}
