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

fn encode_jpeg(frame: &Video) -> Result<Vec<u8>, ff::Error> {
    let codec = ff::encoder::find(ff::codec::Id::MJPEG).ok_or(ff::Error::EncoderNotFound)?;
    let ctx = ff::codec::context::Context::new_with_codec(codec);
    let mut enc = ctx.encoder().video()?;
    enc.set_width(frame.width());
    enc.set_height(frame.height());
    enc.set_format(Pixel::YUVJ420P);
    enc.set_time_base((1, FPS as i32));

    // `-q:v 10` => fixed qscale. Set the QSCALE flag + global_quality, and the
    // per-frame quality, matching ffmpeg's qscale path.
    enc.set_flags(ff::codec::flag::Flags::QSCALE);
    enc.set_global_quality(JPEG_QSCALE * FF_QP2LAMBDA);

    let mut opened = enc.open()?;

    let mut f = frame.clone();
    unsafe {
        (*f.as_mut_ptr()).quality = JPEG_QSCALE * FF_QP2LAMBDA;
    }
    opened.send_frame(&f)?;
    opened.send_eof()?;

    let mut packet = ff::Packet::empty();
    let mut out = Vec::new();
    while opened.receive_packet(&mut packet).is_ok() {
        if let Some(data) = packet.data() {
            out.extend_from_slice(data);
        }
    }
    Ok(out)
}

pub fn decode_video_frames<P: AsRef<Path>>(path: P) -> Result<Vec<Vec<u8>>, ConvertError> {
    ff::init()?;
    let mut ictx = ff::format::input(path.as_ref())?;

    let stream = ictx
        .streams()
        .best(Type::Video)
        .ok_or(ConvertError::NoVideo)?;
    let stream_index = stream.index();
    let time_base = stream.time_base();

    let ctx = ff::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = ctx.decoder().video()?;
    let mut graph = video_filter(&decoder, time_base)?;

    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut decoded = Video::empty();
    let mut filtered = Video::empty();

    // Pull all available frames out of the filtergraph sink and JPEG-encode them.
    fn drain_sink(
        graph: &mut ff::filter::Graph,
        filtered: &mut Video,
        frames: &mut Vec<Vec<u8>>,
    ) -> Result<(), ConvertError> {
        while graph
            .get("out")
            .unwrap()
            .sink()
            .frame(filtered)
            .is_ok()
        {
            frames.push(encode_jpeg(filtered)?);
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
            drain_sink(&mut graph, &mut filtered, &mut frames)?;
        }
    }

    decoder.send_eof()?;
    while decoder.receive_frame(&mut decoded).is_ok() {
        let ts = decoded.timestamp();
        decoded.set_pts(ts);
        graph.get("in").unwrap().source().add(&decoded)?;
        drain_sink(&mut graph, &mut filtered, &mut frames)?;
    }

    graph.get("in").unwrap().source().flush()?;
    drain_sink(&mut graph, &mut filtered, &mut frames)?;

    if frames.is_empty() {
        return Err(ConvertError::Empty);
    }
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

pub fn decode_audio_pcm<P: AsRef<Path>>(path: P) -> Result<Option<Vec<i16>>, ConvertError> {
    ff::init()?;
    let mut ictx = ff::format::input(path.as_ref())?;

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
    let mut filtered = Audio::empty();

    // Pull all available (s16, mono, packed) frames out of the sink.
    fn drain_sink(
        graph: &mut ff::filter::Graph,
        filtered: &mut Audio,
        pcm: &mut Vec<i16>,
    ) {
        while graph
            .get("out")
            .unwrap()
            .sink()
            .frame(filtered)
            .is_ok()
        {
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
            drain_sink(&mut graph, &mut filtered, &mut pcm);
        }
    }

    decoder.send_eof()?;
    while decoder.receive_frame(&mut decoded).is_ok() {
        graph.get("in").unwrap().source().add(&decoded)?;
        drain_sink(&mut graph, &mut filtered, &mut pcm);
    }

    graph.get("in").unwrap().source().flush()?;
    drain_sink(&mut graph, &mut filtered, &mut pcm);

    Ok(Some(pcm))
}

// ---------------------------------------------------------------------------
// End-to-end: decode -> pack -> write collision-safe .fwmv
// ---------------------------------------------------------------------------

fn unique_output_path(dest_dir: &Path, stem: &str) -> PathBuf {
    let first = dest_dir.join(format!("{stem}.fwmv"));
    if !first.exists() {
        return first;
    }
    let mut i = 1;
    loop {
        let p = dest_dir.join(format!("{stem}_{i}.fwmv"));
        if !p.exists() {
            return p;
        }
        i += 1;
    }
}

/// Convert one video to a `.fwmv` in `dest_dir`. `progress(frac)` is called with
/// 0.0..=1.0 as work proceeds. Returns the written path.
pub fn convert_file<P: AsRef<Path>>(
    input: P,
    dest_dir: &Path,
    mut progress: impl FnMut(f32),
) -> Result<PathBuf, ConvertError> {
    let input = input.as_ref();
    progress(0.05);
    let frames = decode_video_frames(input)?; // heavy
    progress(0.7);
    let audio = decode_audio_pcm(input)?; // heavy
    progress(0.9);

    let params = PackParams {
        width: WIDTH as u16,
        height: HEIGHT as u16,
        fps_num: FPS as u16,
        fps_den: 1,
        audio_rate: AUDIO_RATE as u16,
    };
    let bytes = fwmv::pack(&frames, audio.as_deref(), params);

    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let out = unique_output_path(dest_dir, stem);
    std::fs::write(&out, &bytes)?;
    progress(1.0);
    Ok(out)
}
