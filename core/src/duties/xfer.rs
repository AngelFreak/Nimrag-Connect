//! The table of bodies the watch pulls through `data_transfer` (Smart service
//! 7): a reply that is too big for one protobuf message carries a transfer id
//! and size, and the watch fetches the bytes in pieces with
//! `DataDownloadRequest { id, offset, max_chunk_size }`.
//!
//! Ported from Gadgetbridge commit 7257742b (AGPL-3.0-or-later):
//! `app/src/main/java/nodomain/freeyourgadget/gadgetbridge/service/devices/garmin/http/DataTransferHandler.java`
//! (Copyright (C) 2024-2026 Daniele Gobbetti, José Rebelo, kuhy, Thomas Kuehne
//! and the other Gadgetbridge contributors): `registerData`,
//! `handleDataDownloadRequest`, `onDataChunkSuccessfullyReceived` and the
//! `Data` class with its "no gap from 0 to the size" completeness rule.
//! *inferred (Gadgetbridge)*: no capture of ours shows a watch pulling data.
//! What we do differently is marked [ours]: a bounded table (the oldest entry
//! is dropped), ids that carry the connection epoch, and a default chunk size
//! of 4096 when the watch gives none (the schema's default; Gadgetbridge sends
//! everything that is left).

use std::collections::BTreeMap;

/// `max_chunk_size` default in `gdi_data_transfer.proto`.
pub const DEFAULT_CHUNK: u32 = 4096;

/// Transfers kept at once. [ours]
const MAX_ITEMS: usize = 2;

/// What `DataDownloadResponse.Status` says.
pub const STATUS_SUCCESS: u64 = 1;
pub const STATUS_INVALID_ID: u64 = 2;
pub const STATUS_INVALID_OFFSET: u64 = 3;

#[derive(Debug)]
struct Item {
    id: u32,
    /// The caller's number for what this body is for (a GPS-assist request).
    tag: u32,
    data: Vec<u8>,
    /// Chunks the watch has taken: start to end, the longest per start.
    received: BTreeMap<u32, u32>,
}

impl Item {
    /// No gap between the chunks taken, from 0 up to the size.
    fn complete(&self) -> bool {
        let mut end: Option<u32> = None;
        for (&start, &chunk_end) in &self.received {
            match end {
                None if start != 0 => return false,
                Some(e) if start > e => return false,
                _ => {}
            }
            end = Some(chunk_end);
        }
        end.is_some_and(|e| e as usize == self.data.len())
    }
}

/// The result of one `DataDownloadRequest`.
#[derive(Debug, PartialEq, Eq)]
pub enum Download<'a> {
    Chunk(&'a [u8]),
    InvalidId,
    InvalidOffset,
}

#[derive(Debug)]
pub struct Transfers {
    next_id: u32,
    items: Vec<Item>,
}

impl Transfers {
    /// `epoch` is the connection epoch: ids of different connections differ.
    pub fn new(epoch: u32) -> Self {
        Self {
            next_id: (epoch & 0x7FFF) * 0x1_0000 + 1,
            items: Vec::new(),
        }
    }

    /// A transfer id that names nothing (for an empty body). [ours]
    pub fn fresh_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// Keep `data` for the watch to pull. Returns its id and, when the table
    /// was full, the tag of the oldest transfer that was dropped.
    pub fn register(&mut self, tag: u32, data: Vec<u8>) -> (u32, Option<u32>) {
        let id = self.fresh_id();
        let dropped = if self.items.len() >= MAX_ITEMS {
            Some(self.items.remove(0).tag)
        } else {
            None
        };
        self.items.push(Item {
            id,
            tag,
            data,
            received: BTreeMap::new(),
        });
        (id, dropped)
    }

    /// The piece at `offset`, at most `max` bytes (0 means the default).
    pub fn download(&self, id: u32, offset: u32, max: u32) -> Download<'_> {
        let Some(item) = self.items.iter().find(|i| i.id == id) else {
            return Download::InvalidId;
        };
        let max = if max == 0 { DEFAULT_CHUNK } else { max } as usize;
        let start = offset as usize;
        if start >= item.data.len() {
            return Download::InvalidOffset;
        }
        let end = start.saturating_add(max).min(item.data.len());
        Download::Chunk(&item.data[start..end])
    }

    /// The watch took `start..end` of transfer `id`. Returns the tag and the
    /// size when that completes the transfer (it is then forgotten).
    pub fn received(&mut self, id: u32, start: u32, end: u32) -> Option<(u32, usize)> {
        let at = self.items.iter().position(|i| i.id == id)?;
        let item = &mut self.items[at];
        let longer = match item.received.get(&start) {
            Some(&known) => known < end,
            None => true,
        };
        if longer {
            item.received.insert(start, end);
        }
        if item.complete() {
            let item = self.items.remove(at);
            return Some((item.tag, item.data.len()));
        }
        None
    }

    /// Forget transfer `id`; its tag when it existed.
    pub fn remove(&mut self, id: u32) -> Option<u32> {
        let at = self.items.iter().position(|i| i.id == id)?;
        Some(self.items.remove(at).tag)
    }

    pub fn contains(&self, id: u32) -> bool {
        self.items.iter().any(|i| i.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_follow_the_offset_and_the_limit() {
        let mut t = Transfers::new(1);
        let (id, dropped) = t.register(7, (0..10u8).collect());
        assert_eq!(dropped, None);
        assert_eq!(t.download(id, 0, 4), Download::Chunk(&[0, 1, 2, 3]));
        assert_eq!(t.download(id, 8, 4), Download::Chunk(&[8, 9]));
        assert_eq!(t.download(id, 10, 4), Download::InvalidOffset);
        assert_eq!(t.download(id + 1, 0, 4), Download::InvalidId);
        // 0 means the default chunk size, which covers all ten bytes.
        assert_eq!(
            t.download(id, 2, 0),
            Download::Chunk(&[2, 3, 4, 5, 6, 7, 8, 9])
        );
    }

    #[test]
    fn a_transfer_completes_only_without_gaps() {
        let mut t = Transfers::new(1);
        let (id, _) = t.register(5, vec![0; 10]);
        // The head is missing: not complete however far the tail goes.
        assert_eq!(t.received(id, 4, 10), None);
        // A gap between 0..2 and 4..10.
        assert_eq!(t.received(id, 0, 2), None);
        // The gap is closed (an overlapping chunk is fine).
        assert_eq!(t.received(id, 2, 6), Some((5, 10)));
        // Complete means forgotten.
        assert!(!t.contains(id));
        assert_eq!(t.download(id, 0, 4), Download::InvalidId);
    }

    #[test]
    fn a_repeated_request_does_not_shorten_a_chunk() {
        let mut t = Transfers::new(1);
        let (id, _) = t.register(1, vec![0; 8]);
        assert_eq!(t.received(id, 0, 6), None);
        // The watch asked for 0..3 again: the longer chunk stays.
        assert_eq!(t.received(id, 0, 3), None);
        assert_eq!(t.received(id, 6, 8), Some((1, 8)));
    }

    #[test]
    fn the_table_is_bounded() {
        let mut t = Transfers::new(1);
        let (a, _) = t.register(10, vec![1]);
        let (b, _) = t.register(11, vec![2]);
        let (c, dropped) = t.register(12, vec![3]);
        assert_eq!(dropped, Some(10));
        assert!(!t.contains(a) && t.contains(b) && t.contains(c));
        assert_eq!(t.remove(b), Some(11));
        assert_eq!(t.remove(b), None);
    }

    #[test]
    fn ids_differ_between_connections() {
        let mut a = Transfers::new(1);
        let mut b = Transfers::new(2);
        assert_ne!(a.fresh_id(), b.fresh_id());
    }
}
