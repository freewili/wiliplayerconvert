# Run from the movieplayer repo (so `tools` resolves):
#   python <wiliplayerconvert>/scripts/gen_fixtures.py <wiliplayerconvert>/tests/fixtures
import sys, math, struct, pathlib
from tools import adpcm, pack_fwmv as P, fwmv_format as F

out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)

# 1) ADPCM golden: 1000-sample sine.
pcm = [int(math.sin(i * 0.2) * 8000) for i in range(1000)]
(out / "adpcm_sine.bin").write_bytes(adpcm.encode(pcm))

# 2) Full-file goldens. Tiny fake JPEG payloads (the packer is codec-agnostic
#    about frame *content*; it only stores bytes), plus real ADPCM audio.
frames = [bytes([0xFF, 0xD8]) + bytes([i & 0xFF]) * (50 + i) + bytes([0xFF, 0xD9])
          for i in range(40)]
audio = [int(math.sin(i * 0.05) * 6000) for i in range(40 * 16000 // 15 + 16000)]
import array
apcm = array.array("h", audio)
P.pack(frames, str(out / "pack_av.fwmv"), codec=F.CODEC_MJPEG, width=480, height=270,
       fps_num=15, fps_den=1, audio_pcm=apcm, audio_rate=16000, max_bytes=None)
P.pack(frames, str(out / "pack_video_only.fwmv"), codec=F.CODEC_MJPEG, width=480,
       height=270, fps_num=15, fps_den=1, audio_pcm=None, max_bytes=None)
print("wrote fixtures to", out)
