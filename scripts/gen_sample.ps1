param([string]$Out = "tests/fixtures/sample.mp4")
ffmpeg -y -f lavfi -i "testsrc=size=320x240:rate=30:duration=2" `
       -f lavfi -i "sine=frequency=440:duration=2" `
       -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest $Out
