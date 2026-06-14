#[test]
fn ffmpeg_initializes() {
    ffmpeg_next::init().expect("ffmpeg init");
}
