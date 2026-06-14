use fileconvert::convert;
use fileconvert::fwmv::format::Header;

#[test]
fn ffmpeg_initializes() {
    ffmpeg_next::init().expect("ffmpeg init");
}

#[test]
fn decodes_letterboxed_jpeg_frames() {
    let frames = convert::decode_video_frames("tests/fixtures/sample.mp4").unwrap();
    // 2 s at 15 fps target -> ~30 frames.
    assert!(
        frames.len() >= 25 && frames.len() <= 35,
        "got {} frames",
        frames.len()
    );
    // Each frame is a complete JPEG.
    for f in &frames {
        assert_eq!(&f[0..2], &[0xFF, 0xD8], "JPEG SOI");
        assert_eq!(&f[f.len() - 2..], &[0xFF, 0xD9], "JPEG EOI");
    }
}

#[test]
fn decodes_mono_16k_pcm() {
    let pcm = convert::decode_audio_pcm("tests/fixtures/sample.mp4").unwrap();
    // ~2 s of audio at 16 kHz mono.
    let pcm = pcm.expect("sample has audio");
    assert!(
        pcm.len() >= 28000 && pcm.len() <= 36000,
        "got {} samples",
        pcm.len()
    );
}

#[test]
fn returns_none_for_no_audio() {
    // sample.mp4 HAS audio, so this is a compile/contract guard on the API shape.
    let _f: fn(&str) -> Result<Option<Vec<i16>>, convert::ConvertError> =
        |p| convert::decode_audio_pcm(p);
}

#[test]
fn convert_file_writes_valid_fwmv() {
    let dest = std::env::temp_dir().join("fwmv_test_out");
    std::fs::create_dir_all(&dest).unwrap();
    let _ = std::fs::remove_file(dest.join("sample.fwmv"));
    let out = convert::convert_file(
        std::path::Path::new("tests/fixtures/sample.mp4"),
        &dest,
        |_| {},
    )
    .unwrap();
    assert_eq!(out.extension().unwrap(), "fwmv");
    assert_eq!(out.file_stem().unwrap(), "sample");

    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[0..4], b"FWMV");
    let h = Header::parse(&bytes);
    assert_eq!(h.version, 2);
    assert_eq!(h.width, 480);
    assert_eq!(h.height, 270);
    assert_eq!(h.fps_num, 15);
    assert!(h.frame_count >= 25 && h.frame_count <= 35);
    assert_eq!(h.audio_rate, 16000); // sample has audio
                                     // index present at index_offset, magic FWIX
    let io = h.index_offset as usize;
    assert_eq!(&bytes[io..io + 4], b"FWIX");
}

#[test]
fn convert_file_avoids_collisions() {
    let dest = std::env::temp_dir().join("fwmv_test_collide");
    std::fs::create_dir_all(&dest).unwrap();
    let _ = std::fs::remove_file(dest.join("sample.fwmv"));
    let _ = std::fs::remove_file(dest.join("sample_1.fwmv"));
    let a = convert::convert_file(
        std::path::Path::new("tests/fixtures/sample.mp4"),
        &dest,
        |_| {},
    )
    .unwrap();
    let b = convert::convert_file(
        std::path::Path::new("tests/fixtures/sample.mp4"),
        &dest,
        |_| {},
    )
    .unwrap();
    assert_ne!(a, b);
    assert_eq!(b.file_stem().unwrap(), "sample_1");
}
