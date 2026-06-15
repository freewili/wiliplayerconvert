use wiliplayerconvert::fwmv::{pack, PackParams};

fn sample_frames() -> Vec<Vec<u8>> {
    (0..40u32).map(|i| {
        let mut f = vec![0xFF, 0xD8];
        f.extend(std::iter::repeat((i & 0xFF) as u8).take(50 + i as usize));
        f.extend_from_slice(&[0xFF, 0xD9]);
        f
    }).collect()
}

fn sample_audio() -> Vec<i16> {
    (0..(40 * 16000 / 15 + 16000)).map(|i| ((i as f64 * 0.05).sin() * 6000.0) as i16).collect()
}

fn params() -> PackParams {
    PackParams { width: 480, height: 270, fps_num: 15, fps_den: 1, audio_rate: 16000 }
}

#[test]
fn pack_av_matches_python_golden() {
    let frames = sample_frames();
    let audio = sample_audio();
    let got = pack(&frames, Some(&audio), params());
    let want = std::fs::read("tests/fixtures/pack_av.fwmv").unwrap();
    assert_eq!(got, want, "av packing differs from Python golden");
}

#[test]
fn pack_video_only_matches_python_golden() {
    let frames = sample_frames();
    let got = pack(&frames, None, params());
    let want = std::fs::read("tests/fixtures/pack_video_only.fwmv").unwrap();
    assert_eq!(got, want, "video-only packing differs from Python golden");
}
