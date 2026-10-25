# supervox + DJI camera

Researched 24.10.2026 from `~/startups/active/dji` (PocketRemote) and this Mac. Device: **DJI Osmo
Pocket 3**. The dji repo never mentions an Osmo Action or a DJI Mic receiver.

## What exists in the dji repo

| Path | Transport | Audio? | State |
|---|---|---|---|
| iPhone PocketRemote | BLE wake → camera SoftAP → DUML on UDP 9004 | **no**: the Pocket sends no audio over Wi-Fi | works on hardware (remote-test 7/7): preview, gimbal, record, clips |
| iPhone on USB + `iproxy 8787` → `GET /frame` | phone relays a JPEG of the live picture | no | built, not yet run against a live camera |
| Mac ↔ Pocket over USB-C, **Webcam mode** (UVC) | USB | **yes, likely**: interfaces 2–4 are USB audio | video + pan/tilt/zoom measured 09.10 (`docs/uvc-ptz-findings.md`); audio never tried |
| Mac ↔ Pocket direct over Wi-Fi | `pocketlink-cli` on macOS | no (same protocol as the phone) | idea only (`docs/idea-mac-direct-wifi.md`, unpushed) |
| Live skill (`tools/live/live.py`) | **Mac mic** → whisper-stream | Mac mic, not the camera | known-answer test via BlackHole passed |
| Harvest (`tools/harvest`) | clip downloaded over camera HTTP → subtitles | the camera's own audio, after the take | works |

Dead ends recorded there: pyusb control transfers (macOS UVC driver owns the interface), ffmpeg
avfoundation *video* hangs on this camera and picks the OBS virtual camera by index; use
`tools/uvc/grab.swift`. In Webcam mode the camera does not record to its card, and it sometimes
leaves Webcam mode on its own (PID `0x2ca3:0x0023` → `0x0020`).

## What works today

Nothing DJI is plugged in now. Inputs this Mac sees: MacBook Pro Microphone, Rust's iPhone
Microphone (Continuity), BlackHole 2ch, ZoomAudioDevice, Mixx Rig. So the USB audio path is
**unverified**; everything below is the plan for the first run with the camera.

supervox takes any CoreAudio input, so a Pocket 3 in Webcam mode should work with no new code.
Since this commit `mic_device` also matches a substring, ignoring case: `mic_device = "pocket"`
finds the camera whatever its exact CoreAudio name is, and a missing device fails with the list of
devices that are there (it used to say "No input device available").

## Chosen path: USB-C, Webcam mode

```bash
# 1. Pocket 3: plug USB-C into the Mac, choose "Webcam" on the camera screen.
system_profiler SPUSBDataType | grep -A3 -i pocket          # expect 0x2ca3 / 0x0023
system_profiler SPAudioDataType | grep -B1 -A4 -i -E 'pocket|dji'
ffmpeg -hide_banner -f avfoundation -list_devices true -i "" 2>&1 | grep -A20 'audio devices'

# 2. Known-answer check of the audio alone (video capture hangs, audio-only is untested):
ffmpeg -f avfoundation -i ":<audio index>" -t 5 /tmp/pocket.wav && afplay /tmp/pocket.wav

# 3. supervox
echo 'mic_device = "pocket"' >> ~/.supervox/config.toml     # or press m in live mode until it shows
supervox live
```

Video for the same session: the camera is a webcam then, so `grab.swift` in the dji repo can take
frames alongside (`swiftc -O -o grab tools/uvc/grab.swift && ./grab OsmoPocket3 ./frames 20 0.5`).
Whether two processes can open the video and the audio interface at once is untested.

## Fallback: wireless via DJI Mimo RTMP (not built)

When the camera must stay on the iPhone/Mimo and record to its card, Mimo can live-stream to a
custom RTMP URL. A Mac on the same network can receive it with no server:

```bash
ffmpeg -listen 1 -i rtmp://0.0.0.0:1935/live/pocket -vn -ac 1 -ar 16000 -f f32le -   # PCM on stdout
```

Plan: `voxkit::stream_input::StreamCapture::start_raw(url)` spawns that ffmpeg, reads f32 PCM from
stdout into the same `mpsc::Receiver<AudioChunk>` that `MicCapture::start_raw_on` returns, and
kills the child on `stop()`. `AudioPipeline::start` picks it when `mic_device` starts with
`rtmp://` or `rtsp://`. Known-answer test without the device: ffmpeg writes a lavfi sine to a file,
the capture reads it and the test checks sample count and RMS. Estimate ~120 lines + test, half a
day including one real Mimo run. Costs: 2–5 s of RTMP buffering on top of STT latency, and the
iPhone plus the Mimo app in the loop. Not built because the USB path needs no code; build it if
Rustam answers that the camera has to stay wireless.

After a take (any path): the camera's own audio is in the clip. `supervox import` does not take
MP4, so convert first: `ffmpeg -i DJI_0001.MP4 -vn -ac 1 -ar 16000 clip.wav && supervox import clip.wav`.

## Open questions for Rustam

1. Which device for live: Pocket 3 on USB (Webcam mode, no card recording), or must it record to the
   card at the same time (then RTMP or a separate mic)?
2. Do you have a DJI Mic 2 *receiver*? Its USB-C receiver is a plain USB audio input and would work
   with `mic_device = "dji"` while the Pocket records its own card. The Creator Combo ships only the
   transmitter.
3. If a DJI Mic transmitter is paired to the Pocket, does Webcam mode carry its audio over USB? Untested.
