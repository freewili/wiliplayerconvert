// FWMV v2 container primitives. Mirror of tools/fwmv_format.py.

pub const MAGIC: &[u8; 4] = b"FWMV";
pub const HEADER_SIZE: usize = 64;
pub const VERSION_V2: u16 = 2;
pub const CODEC_MJPEG: u16 = 1;
pub const FLAG_AUDIO: u16 = 1;
pub const AUDIO_IMA_ADPCM: u16 = 1;

pub const REC_VIDEO: u16 = 1;
pub const REC_AUDIO: u16 = 2;
pub const REC_END: u16 = 0xFFFF;
pub const RECORD_HDR_SIZE: usize = 8;

pub const INDEX_MAGIC: &[u8; 4] = b"FWIX";
pub const INDEX_ENTRY_SIZE: usize = 16;
pub const INDEX_MAX_ENTRIES: usize = 600;

pub fn record_padding(size: usize) -> usize {
    (4 - size % 4) % 4
}

pub fn pack_record(rtype: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(RECORD_HDR_SIZE + payload.len() + 3);
    out.extend_from_slice(&rtype.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());          // reserved
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out.extend(std::iter::repeat(0u8).take(record_padding(payload.len())));
    out
}

#[derive(Clone, Copy)]
pub struct Header {
    pub version: u16, pub flags: u16, pub codec: u16,
    pub width: u16, pub height: u16, pub fps_num: u16, pub fps_den: u16,
    pub frame_count: u32, pub index_offset: u32, pub data_offset: u32,
    pub audio_offset: u32, pub audio_codec: u16, pub audio_rate: u16,
    pub audio_size: u32, pub audio_samples: u32,
}

impl Header {
    pub fn pack(&self) -> [u8; HEADER_SIZE] {
        let mut b = [0u8; HEADER_SIZE];
        b[0..4].copy_from_slice(MAGIC);
        let mut o = 4usize; // cursor starts just past the 4-byte magic
        macro_rules! put { ($v:expr) => {{ let s = $v.to_le_bytes(); b[o..o+s.len()].copy_from_slice(&s); o += s.len(); }}; }
        put!(self.version); put!(self.flags); put!(self.codec);
        put!(self.width); put!(self.height); put!(self.fps_num); put!(self.fps_den);
        put!(self.frame_count); put!(self.index_offset); put!(self.data_offset);
        put!(self.audio_offset); put!(self.audio_codec); put!(self.audio_rate);
        put!(self.audio_size); put!(self.audio_samples);
        // remaining bytes already zero (4*u32 + 2 pad)
        let _ = o;
        b
    }

    pub fn parse(buf: &[u8]) -> Header {
        let u16a = |o: usize| u16::from_le_bytes(buf[o..o+2].try_into().unwrap());
        let u32a = |o: usize| u32::from_le_bytes(buf[o..o+4].try_into().unwrap());
        Header {
            version: u16a(4), flags: u16a(6), codec: u16a(8),
            width: u16a(10), height: u16a(12), fps_num: u16a(14), fps_den: u16a(16),
            frame_count: u32a(18), index_offset: u32a(22), data_offset: u32a(26),
            audio_offset: u32a(30), audio_codec: u16a(34), audio_rate: u16a(36),
            audio_size: u32a(38), audio_samples: u32a(42),
        }
    }
}

#[derive(Clone, Copy)]
pub struct IndexEntry {
    pub frame_no: u32, pub file_offset: u32, pub audio_bytes_before: u32,
    pub adpcm_predictor: i16, pub adpcm_step_index: u8,
}

pub fn index_size(n_entries: usize) -> usize { 8 + n_entries * INDEX_ENTRY_SIZE }

pub fn pack_index(entries: &[IndexEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(index_size(entries.len()));
    out.extend_from_slice(INDEX_MAGIC);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        out.extend_from_slice(&e.frame_no.to_le_bytes());
        out.extend_from_slice(&e.file_offset.to_le_bytes());
        out.extend_from_slice(&e.audio_bytes_before.to_le_bytes());
        out.extend_from_slice(&e.adpcm_predictor.to_le_bytes());
        out.push(e.adpcm_step_index);
        out.push(0); // pad
    }
    out
}
