use wiliplayerconvert::fwmv::format::{self, Header, IndexEntry};

#[test]
fn record_padding_aligns_to_four() {
    assert_eq!(format::record_padding(0), 0);
    assert_eq!(format::record_padding(3), 1);
    assert_eq!(format::record_padding(4), 0);
    assert_eq!(format::record_padding(5), 3);
}

#[test]
fn pack_record_end_is_eight_bytes() {
    assert_eq!(format::pack_record(format::REC_END, &[]),
               vec![0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn pack_record_video_pads_payload() {
    // type=1, reserved=0, size=3, "abc", + 1 pad byte.
    assert_eq!(format::pack_record(format::REC_VIDEO, b"abc"),
               vec![0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, b'a', b'b', b'c', 0x00]);
}

#[test]
fn header_is_64_bytes_with_magic() {
    let h = Header {
        version: 2, flags: 0, codec: 1, width: 480, height: 270,
        fps_num: 15, fps_den: 1, frame_count: 40, index_offset: 1234,
        data_offset: 64, audio_offset: 0, audio_codec: 1, audio_rate: 16000,
        audio_size: 555, audio_samples: 999,
    };
    let bytes = h.pack();
    assert_eq!(bytes.len(), 64);
    assert_eq!(&bytes[0..4], b"FWMV");
    // round-trip fields
    let p = Header::parse(&bytes);
    assert_eq!(p.width, 480);
    assert_eq!(p.frame_count, 40);
    assert_eq!(p.audio_rate, 16000);
}

#[test]
fn pack_index_has_magic_count_and_16_byte_entries() {
    let entries = vec![IndexEntry {
        frame_no: 0, file_offset: 64, audio_bytes_before: 0,
        adpcm_predictor: 0, adpcm_step_index: 0,
    }];
    let blob = format::pack_index(&entries);
    assert_eq!(&blob[0..4], b"FWIX");
    assert_eq!(u32::from_le_bytes(blob[4..8].try_into().unwrap()), 1);
    assert_eq!(blob.len(), 8 + 16);
}
