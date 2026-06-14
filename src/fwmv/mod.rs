pub mod adpcm;
pub mod format;

use format as F;

#[derive(Clone, Copy)]
pub struct PackParams {
    pub width: u16, pub height: u16,
    pub fps_num: u16, pub fps_den: u16,
    pub audio_rate: u16,
}

fn audio_samples_for(n_frames: usize, p: &PackParams) -> usize {
    n_frames * p.fps_den as usize * p.audio_rate as usize / p.fps_num as usize
}

fn lead_frames(p: &PackParams) -> usize {
    // ceil(fps_num / fps_den)
    let (num, den) = (p.fps_num as usize, p.fps_den as usize);
    (num + den - 1) / den
}

fn index_entry_frames(n: usize, p: &PackParams) -> Vec<usize> {
    let (num, den) = (p.fps_num as usize, p.fps_den as usize);
    let dur_s = n * den / num;
    let interval_s = if dur_s == 0 { 1 } else { ((dur_s + F::INDEX_MAX_ENTRIES - 1) / F::INDEX_MAX_ENTRIES).max(1) };
    let mut out = Vec::new();
    let mut k = 0usize;
    loop {
        let fk = (k * num + den - 1) / den; // ceil(k*num/den)
        if fk >= n { break; }
        out.push(fk);
        k += interval_s;
    }
    out
}

/// Cut the ADPCM blob at per-frame byte boundaries; last chunk takes the rest.
fn audio_chunks<'a>(blob: &'a [u8], n: usize, p: &PackParams) -> Vec<&'a [u8]> {
    let mut bounds: Vec<usize> = (0..n).map(|i| audio_samples_for(i, p) / 2).collect();
    bounds.push(blob.len());
    (0..n).map(|i| &blob[bounds[i]..bounds[i + 1]]).collect()
}

/// Write a full FWMV v2 file (header + interleaved record stream + index) to a
/// byte vector. No size trimming (v1 thumb-drive path). Mirrors pack_fwmv.pack.
pub fn pack(frames: &[Vec<u8>], audio_pcm: Option<&[i16]>, p: PackParams) -> Vec<u8> {
    let n = frames.len();
    let mut flags = 0u16;

    // --- audio prep ---
    let mut audio_blob: Vec<u8> = Vec::new();
    let mut audio_samples = 0usize;
    let mut chunks: Vec<&[u8]> = Vec::new();
    let has_audio = audio_pcm.is_some();
    if let Some(src) = audio_pcm {
        flags |= F::FLAG_AUDIO;
        audio_samples = audio_samples_for(n, &p);
        let mut pcm: Vec<i16> = src.iter().copied().take(audio_samples).collect();
        pcm.resize(audio_samples, 0); // pad if source audio is short
        audio_blob = adpcm::encode(&pcm);
        chunks = audio_chunks(&audio_blob, n, &p);
    }

    let lead = if has_audio { lead_frames(&p).min(n) } else { 0 };
    let entry_frames = index_entry_frames(n, &p);

    // --- record stream ---
    let mut stream: Vec<u8> = Vec::new();
    let mut entries: Vec<F::IndexEntry> = Vec::new();
    let mut audio_bytes_emitted: u32 = 0;
    let mut ei = 0usize;

    for i in 0..lead {
        stream.extend_from_slice(&F::pack_record(F::REC_AUDIO, chunks[i]));
        audio_bytes_emitted += chunks[i].len() as u32;
    }
    for (i, fr) in frames.iter().enumerate() {
        if ei < entry_frames.len() && i == entry_frames[ei] {
            entries.push(F::IndexEntry {
                frame_no: i as u32,
                file_offset: (F::HEADER_SIZE + stream.len()) as u32,
                audio_bytes_before: audio_bytes_emitted,
                adpcm_predictor: 0, adpcm_step_index: 0,
            });
            ei += 1;
        }
        stream.extend_from_slice(&F::pack_record(F::REC_VIDEO, fr));
        let j = i + lead;
        if has_audio && j < n {
            stream.extend_from_slice(&F::pack_record(F::REC_AUDIO, chunks[j]));
            audio_bytes_emitted += chunks[j].len() as u32;
        }
    }
    stream.extend_from_slice(&F::pack_record(F::REC_END, &[]));
    let index_offset = (F::HEADER_SIZE + stream.len()) as u32;

    // fill ADPCM state for each index entry
    if has_audio && !entries.is_empty() {
        let offsets: Vec<usize> = entries.iter().map(|e| e.audio_bytes_before as usize).collect();
        let states = adpcm::scan_states(&audio_blob, &offsets);
        for (e, (pred, si)) in entries.iter_mut().zip(states) {
            e.adpcm_predictor = pred;
            e.adpcm_step_index = si;
        }
    }
    stream.extend_from_slice(&F::pack_index(&entries));

    let header = F::Header {
        version: F::VERSION_V2, flags, codec: F::CODEC_MJPEG,
        width: p.width, height: p.height, fps_num: p.fps_num, fps_den: p.fps_den,
        frame_count: n as u32, index_offset, data_offset: F::HEADER_SIZE as u32,
        audio_offset: 0,
        audio_codec: if has_audio { F::AUDIO_IMA_ADPCM } else { 0 },
        audio_rate: if has_audio { p.audio_rate } else { 0 },
        audio_size: audio_blob.len() as u32,
        audio_samples: audio_samples as u32,
    };

    let mut out = Vec::with_capacity(F::HEADER_SIZE + stream.len());
    out.extend_from_slice(&header.pack());
    out.extend_from_slice(&stream);
    out
}
