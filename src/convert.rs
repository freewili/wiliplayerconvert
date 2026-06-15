// Converter core: drives statically-linked FFmpeg (demux -> decode ->
// libavfilter scale/pad/fps + resample -> MJPEG encode) to produce JPEG frames
// and mono 16 kHz s16 PCM, then calls the pure-Rust fwmv packer.
//
// NOTE: this crate links `ffmpeg-the-third` (v5, FFmpeg 8.1) under the import
// name `ffmpeg_next`. The channel-layout API is the AVChannelLayout era.

use std::path::{Path, PathBuf};

use ffmpeg_next as ff;
use ff::format::Pixel;
use ff::media::Type;
use ff::util::frame::audio::Audio;
use ff::util::frame::video::Video;

use crate::fwmv::{self, PackParams};

pub const WIDTH: u32 = 480;
pub const HEIGHT: u32 = 270;
pub const FPS: u32 = 15;
pub const AUDIO_RATE: u32 = 16000;

// ffmpeg `-q:v 10`. FF_QP2LAMBDA is a C macro (118) not exported by the sys crate.
const JPEG_QSCALE: i32 = 10;
const FF_QP2LAMBDA: i32 = 118;

#[derive(Debug)]
pub enum ConvertError {
    Ffmpeg(ff::Error),
    Io(std::io::Error),
    NoVideo,
    Empty,
}

impl From<ff::Error> for ConvertError {
    fn from(e: ff::Error) -> Self {
        ConvertError::Ffmpeg(e)
    }
}
impl From<std::io::Error> for ConvertError {
    fn from(e: std::io::Error) -> Self {
        ConvertError::Io(e)
    }
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::Ffmpeg(e) => write!(f, "ffmpeg error: {e}"),
            ConvertError::Io(e) => write!(f, "io error: {e}"),
            ConvertError::NoVideo => write!(f, "input has no video stream"),
            ConvertError::Empty => write!(f, "no frames decoded"),
        }
    }
}
impl std::error::Error for ConvertError {}

// ---------------------------------------------------------------------------
// Video: decode -> scale/pad to 480x270 letterboxed -> fps 15 -> MJPEG @ q:v 10
// ---------------------------------------------------------------------------

fn video_filter(
    decoder: &ff::decoder::Video,
    time_base: ff::Rational,
) -> Result<ff::filter::Graph, ff::Error> {
    let mut g = ff::filter::Graph::new();

    let pix_fmt = decoder
        .format()
        .descriptor()
        .map(|d| d.name())
        .unwrap_or("yuv420p");
    let (tb_num, tb_den) = (time_base.numerator(), time_base.denominator());
    let (tb_num, tb_den) = if tb_den == 0 { (1, 1_000_000) } else { (tb_num, tb_den) };
    let args = format!(
        "video_size={}x{}:pix_fmt={}:time_base={}/{}:pixel_aspect=1/1",
        decoder.width(),
        decoder.height(),
        pix_fmt,
        tb_num,
        tb_den,
    );
    g.add(&ff::filter::find("buffer").unwrap(), "in", &args)?;
    g.add(&ff::filter::find("buffersink").unwrap(), "out", "")?;

    let spec = "scale=480:270:force_original_aspect_ratio=decrease,\
                pad=480:270:(ow-iw)/2:(oh-ih)/2,fps=15,format=yuvj420p";
    g.output("in", 0)?.input("out", 0)?.parse(spec)?;
    g.validate()?;
    Ok(g)
}

/// Open one MJPEG encoder for the whole video (480x270 yuvj420p, fixed qscale).
/// Reused across every frame — recreating it per frame was a large overhead.
fn open_mjpeg_encoder() -> Result<ff::encoder::video::Encoder, ff::Error> {
    let codec = ff::encoder::find(ff::codec::Id::MJPEG).ok_or(ff::Error::EncoderNotFound)?;
    let ctx = ff::codec::context::Context::new_with_codec(codec);
    let mut enc = ctx.encoder().video()?;
    enc.set_width(WIDTH);
    enc.set_height(HEIGHT);
    enc.set_format(Pixel::YUVJ420P);
    enc.set_time_base((1, FPS as i32));
    // `-q:v 10` => fixed qscale: QSCALE flag + global_quality (per-frame quality
    // is also set on each frame below).
    enc.set_flags(ff::codec::flag::Flags::QSCALE);
    enc.set_global_quality(JPEG_QSCALE * FF_QP2LAMBDA);
    enc.open()
}

/// Feed one filtered frame to the persistent encoder and collect the JPEG(s)
/// it emits (MJPEG is intra-only: one complete JPEG per frame).
fn encode_into(
    encoder: &mut ff::encoder::video::Encoder,
    frame: &mut Video,
    frames: &mut Vec<Vec<u8>>,
) -> Result<(), ConvertError> {
    unsafe {
        (*frame.as_mut_ptr()).quality = JPEG_QSCALE * FF_QP2LAMBDA;
    }
    encoder.send_frame(frame)?;
    let mut packet = ff::Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        if let Some(data) = packet.data() {
            frames.push(data.to_vec());
        }
    }
    Ok(())
}

pub fn decode_video_frames<P: AsRef<Path>>(
    path: P,
    threads: usize,
    mut on_progress: impl FnMut(f32),
) -> Result<Vec<Vec<u8>>, ConvertError> {
    ff::init()?;
    let mut ictx = ff::format::input(path.as_ref())?;

    // Expected output frame count, for progress reporting (0 = duration unknown).
    let expected_frames = if ictx.duration() > 0 {
        (ictx.duration() as f64 / 1_000_000.0 * FPS as f64).max(1.0) as f32
    } else {
        0.0
    };

    let stream = ictx
        .streams()
        .best(Type::Video)
        .ok_or(ConvertError::NoVideo)?;
    let stream_index = stream.index();
    let time_base = stream.time_base();

    let mut ctx = ff::codec::context::Context::from_parameters(stream.parameters())?;
    // Multithreaded (frame-parallel) decode — the source decode dominates the
    // cost. `threads` is the per-file budget (the GUI splits cores across the
    // worker pool); 0 = let FFmpeg auto-detect.
    if threads != 1 {
        ctx.set_threading(ff::codec::threading::Config {
            kind: ff::codec::threading::Type::Frame,
            count: threads,
        });
    }
    let mut decoder = ctx.decoder().video()?;
    let mut graph = video_filter(&decoder, time_base)?;
    let mut encoder = open_mjpeg_encoder()?;

    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut decoded = Video::empty();

    // Pull all available frames out of the filtergraph sink and JPEG-encode them
    // with the persistent encoder. NOTE: av_buffersink_get_frame does NOT unref
    // the target frame first, so we use a FRESH frame each pull — reusing one
    // frame leaks the previous frame's buffer on every call.
    fn drain_sink(
        graph: &mut ff::filter::Graph,
        frames: &mut Vec<Vec<u8>>,
        encoder: &mut ff::encoder::video::Encoder,
    ) -> Result<(), ConvertError> {
        loop {
            let mut filtered = Video::empty();
            if graph.get("out").unwrap().sink().frame(&mut filtered).is_err() {
                break;
            }
            encode_into(encoder, &mut filtered, frames)?;
        }
        Ok(())
    }

    // The `fps` filter needs monotonic pts in the buffer's declared time_base;
    // carry the decoded frame's best-effort timestamp into pts.
    for item in ictx.packets() {
        let (s, packet) = item?; // surface a corrupt read instead of truncating
        if s.index() != stream_index {
            continue;
        }
        decoder.send_packet(&packet)?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            let ts = decoded.timestamp();
            decoded.set_pts(ts);
            graph.get("in").unwrap().source().add(&decoded)?;
            drain_sink(&mut graph, &mut frames, &mut encoder)?;
            if expected_frames > 0.0 {
                on_progress((frames.len() as f32 / expected_frames).min(0.999));
            }
        }
    }

    decoder.send_eof()?;
    while decoder.receive_frame(&mut decoded).is_ok() {
        let ts = decoded.timestamp();
        decoded.set_pts(ts);
        graph.get("in").unwrap().source().add(&decoded)?;
        drain_sink(&mut graph, &mut frames, &mut encoder)?;
    }

    graph.get("in").unwrap().source().flush()?;
    drain_sink(&mut graph, &mut frames, &mut encoder)?;

    // Flush the encoder for any frame it was still holding.
    encoder.send_eof()?;
    let mut packet = ff::Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        if let Some(data) = packet.data() {
            frames.push(data.to_vec());
        }
    }

    if frames.is_empty() {
        return Err(ConvertError::Empty);
    }
    on_progress(1.0);
    Ok(frames)
}

// ---------------------------------------------------------------------------
// Audio: decode -> aresample 16000 -> mono s16. Returns None if no audio.
// ---------------------------------------------------------------------------

fn audio_filter(decoder: &ff::decoder::Audio) -> Result<ff::filter::Graph, ff::Error> {
    let mut g = ff::filter::Graph::new();

    let args = format!(
        "time_base=1/{rate}:sample_rate={rate}:sample_fmt={fmt}:channel_layout={layout}",
        rate = decoder.rate(),
        fmt = decoder.format().name(),
        layout = decoder.ch_layout().description(),
    );
    g.add(&ff::filter::find("abuffer").unwrap(), "in", &args)?;
    g.add(&ff::filter::find("abuffersink").unwrap(), "out", "")?;

    let spec = "aresample=16000,aformat=sample_fmts=s16:channel_layouts=mono";
    g.output("in", 0)?.input("out", 0)?.parse(spec)?;
    g.validate()?;
    Ok(g)
}

pub fn decode_audio_pcm<P: AsRef<Path>>(
    path: P,
    mut on_progress: impl FnMut(f32),
) -> Result<Option<Vec<i16>>, ConvertError> {
    ff::init()?;
    let mut ictx = ff::format::input(path.as_ref())?;

    // Expected mono-16k sample count, for progress (0 = duration unknown).
    let expected_samples = if ictx.duration() > 0 {
        (ictx.duration() as f64 / 1_000_000.0 * AUDIO_RATE as f64).max(1.0) as f32
    } else {
        0.0
    };

    let stream = match ictx.streams().best(Type::Audio) {
        Some(s) => s,
        None => return Ok(None),
    };
    let stream_index = stream.index();

    let ctx = ff::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = ctx.decoder().audio()?;
    let mut graph = audio_filter(&decoder)?;

    let mut pcm: Vec<i16> = Vec::new();
    let mut decoded = Audio::empty();

    // Pull all available (s16, mono, packed) frames out of the sink. As with the
    // video sink, use a FRESH frame each pull — av_buffersink_get_frame doesn't
    // unref the target, so reusing one frame leaks the previous frame each call.
    fn drain_sink(graph: &mut ff::filter::Graph, pcm: &mut Vec<i16>) {
        loop {
            let mut filtered = Audio::empty();
            if graph.get("out").unwrap().sink().frame(&mut filtered).is_err() {
                break;
            }
            // plane(0) is exactly nb_samples long for packed mono s16.
            pcm.extend_from_slice(filtered.plane::<i16>(0));
        }
    }

    for item in ictx.packets() {
        let (s, packet) = item?; // surface a corrupt read instead of truncating
        if s.index() != stream_index {
            continue;
        }
        decoder.send_packet(&packet)?;
        while decoder.receive_frame(&mut decoded).is_ok() {
            graph.get("in").unwrap().source().add(&decoded)?;
            drain_sink(&mut graph, &mut pcm);
            if expected_samples > 0.0 {
                on_progress((pcm.len() as f32 / expected_samples).min(0.999));
            }
        }
    }

    decoder.send_eof()?;
    while decoder.receive_frame(&mut decoded).is_ok() {
        graph.get("in").unwrap().source().add(&decoded)?;
        drain_sink(&mut graph, &mut pcm);
    }

    graph.get("in").unwrap().source().flush()?;
    drain_sink(&mut graph, &mut pcm);

    on_progress(1.0);
    Ok(Some(pcm))
}

// ---------------------------------------------------------------------------
// End-to-end: decode -> pack -> write collision-safe .fwmv
// ---------------------------------------------------------------------------

/// Pick the `.fwmv` path for `input` in `dest_dir`, avoiding both existing files
/// on disk and any path already in `reserved`. The `reserved` set lets a caller
/// assign collision-safe names for a whole batch up front, so parallel workers
/// never race two same-named inputs onto the same output file.
pub fn output_path(
    dest_dir: &Path,
    input: &Path,
    reserved: &std::collections::HashSet<PathBuf>,
) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let taken = |p: &PathBuf| p.exists() || reserved.contains(p);
    let first = dest_dir.join(format!("{stem}.fwmv"));
    if !taken(&first) {
        return first;
    }
    let mut i = 1;
    loop {
        let p = dest_dir.join(format!("{stem}_{i}.fwmv"));
        if !taken(&p) {
            return p;
        }
        i += 1;
    }
}

/// Decode `input` and write the packed `.fwmv` to the explicit `out_path`.
/// `progress(frac)` is called with 0.0..=1.0 as work proceeds.
pub fn convert_to<P: AsRef<Path>>(
    input: P,
    out_path: &Path,
    decode_threads: usize,
    mut progress: impl FnMut(f32),
) -> Result<(), ConvertError> {
    let input = input.as_ref();
    progress(0.0);
    // Video decode is the bulk of the work (~0..0.9 of the bar); audio (~0.9..0.98)
    // and packing/writing (the final jump to 1.0) are comparatively quick. Throttle
    // forwarding to ~1% steps so a long video doesn't flood the UI channel.
    let mut vlast = -1.0f32;
    let frames = decode_video_frames(input, decode_threads, |p| {
        let v = p * 0.9;
        if v - vlast >= 0.01 || p >= 1.0 {
            vlast = v;
            progress(v);
        }
    })?;
    let mut alast = -1.0f32;
    let audio = decode_audio_pcm(input, |p| {
        let v = 0.9 + p * 0.08;
        if v - alast >= 0.01 || p >= 1.0 {
            alast = v;
            progress(v);
        }
    })?;

    let params = PackParams {
        width: WIDTH as u16,
        height: HEIGHT as u16,
        fps_num: FPS as u16,
        fps_den: 1,
        audio_rate: AUDIO_RATE as u16,
    };
    let bytes = fwmv::pack(&frames, audio.as_deref(), params);
    std::fs::write(out_path, &bytes)?;
    progress(1.0);
    Ok(())
}

/// Convert one video to a collision-safe `.fwmv` in `dest_dir`. Returns the
/// written path. (Sequential convenience wrapper over [`output_path`] +
/// [`convert_to`]; the GUI reserves names for the whole batch itself.)
pub fn convert_file<P: AsRef<Path>>(
    input: P,
    dest_dir: &Path,
    progress: impl FnMut(f32),
) -> Result<PathBuf, ConvertError> {
    let input = input.as_ref();
    let out = output_path(dest_dir, input, &std::collections::HashSet::new());
    // Sequential convenience path: let one file use all cores.
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    convert_to(input, &out, threads, progress)?;
    Ok(out)
}
