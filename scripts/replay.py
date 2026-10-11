#!/usr/bin/env python3
"""Makes a video of a Wordhord recording (see crates/wordhord-device/src/record.rs).

    scripts/replay.py <recording.whrec> <out.mp4> [options]

The screen as it was, at the speed it changed, with a soft dot where a
finger touched and a fine ring where the pen wrote. Long pauses are cut
short, so a demonstration doesn't stall while its maker thinks.

Options:
    --fps N          frames a second (30)
    --width N        the video's width; height keeps the screen's shape (1080)
    --max-pause S    no pause longer than this, in seconds (1.5)
    --from S --to S  only this stretch, in seconds of the recording
    --no-flash       a full refresh without its flash
    --frames DIR     write PNG frames there instead of a video

Needs Python with NumPy and Pillow; a video needs ffmpeg.
"""

import argparse
import re
import shutil
import struct
import subprocess
import sys
import zlib
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw


def read(path):
    data = Path(path).read_bytes()
    if not data.startswith(b"WHREC1\n"):
        sys.exit(f"{path}: not a Wordhord recording")
    w, h = struct.unpack_from("<II", data, 7)
    at, events = 15, []
    while at + 9 <= len(data):
        t, kind = struct.unpack_from("<QB", data, at)
        at += 9
        if kind == ord("D"):
            if at + 21 > len(data):
                break
            x, y, dw, dh, how, n = struct.unpack_from("<IIIIBI", data, at)
            at += 21
            if at + n > len(data):
                break
            pixels = np.frombuffer(inflate(data[at:at + n]), np.uint8)
            at += n
            events.append((t, "draw", (x, y, dw, dh, how, pixels.reshape(dh, dw))))
        elif kind == ord("G"):
            (n,) = struct.unpack_from("<H", data, at)
            at += 2
            events.append((t, "gesture", data[at:at + n].decode()))
            at += n
        elif kind == ord("P"):
            (n,) = struct.unpack_from("<I", data, at)
            at += 4
            points = [struct.unpack_from("<iiH", data, at + 10 * k) for k in range(n)]
            at += 10 * n
            events.append((t, "pen", points))
        else:
            break
    return w, h, events


def inflate(raw):
    # Raw deflate, as miniz's compress_to_vec writes it: no zlib header.
    return zlib.decompress(raw, -15)


def timeline(events, max_pause):
    """Each event's time in the video, in seconds, long pauses cut short."""
    out, last, shift = [], None, 0.0
    for t, kind, what in events:
        s = t / 1e6
        if last is not None and s - last > max_pause:
            shift += s - last - max_pause
        last = s
        out.append((s - shift, kind, what))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("recording")
    ap.add_argument("out")
    ap.add_argument("--fps", type=float, default=30)
    ap.add_argument("--width", type=int, default=1080)
    ap.add_argument("--max-pause", type=float, default=1.5)
    ap.add_argument("--from", dest="start", type=float, default=0.0)
    ap.add_argument("--to", dest="end", type=float, default=None)
    ap.add_argument("--no-flash", action="store_true")
    ap.add_argument("--frames")
    a = ap.parse_args()

    w, h, events = read(a.recording)
    events = timeline(events, a.max_pause)
    if not events:
        sys.exit("nothing recorded")
    end = a.end if a.end is not None else events[-1][0] + 1.0
    vw = a.width
    vh = round(h * vw / w / 2) * 2
    scale = vw / w

    screen = np.full((h, w), 255, np.uint8)
    # The finger: where, since when, and whether it's still down.
    finger = None
    # The pen: its points as they came, each with its time.
    pen = []
    flashes = []

    if a.frames:
        Path(a.frames).mkdir(parents=True, exist_ok=True)
        sink = None
    else:
        if not shutil.which("ffmpeg"):
            sys.exit("needs ffmpeg for a video; or --frames DIR for PNGs")
        sink = subprocess.Popen(
            ["ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24",
             "-s", f"{vw}x{vh}", "-r", str(a.fps), "-i", "-",
             "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18", "-movflags", "+faststart",
             a.out],
            stdin=subprocess.PIPE,
        )

    gesture = re.compile(r"(\w+)(?:\((-?\d+), (-?\d+)\))?")
    k, frame, last_pen_t = 0, 0, None
    t = a.start
    while t <= end:
        while k < len(events) and events[k][0] <= t:
            s, kind, what = events[k]
            if kind == "draw":
                x, y, dw, dh, how, pixels = what
                screen[y:y + dh, x:x + dw] = pixels
                if how == 1 and not a.no_flash:
                    flashes.append((s, (x, y, dw, dh)))
            elif kind == "gesture":
                m = gesture.match(what)
                name = m.group(1)
                xy = (int(m.group(2)), int(m.group(3))) if m.group(2) else None
                if name in ("Press", "Hold", "Drag") and xy:
                    finger = (xy, s, True)
                elif name in ("Tap", "Release") and xy:
                    finger = (xy, s, False)
            elif kind == "pen":
                # The points came over the time since the last batch.
                begin = last_pen_t if last_pen_t and s - last_pen_t < 0.2 else s - 0.04
                for j, (px, py, pr) in enumerate(what):
                    pen.append((begin + (s - begin) * (j + 1) / len(what), px, py))
                last_pen_t = s
            k += 1

        img = Image.fromarray(screen, "L").convert("RGB")
        # A full refresh: the region shown inverted for a moment.
        live = [r for (s, r) in flashes if t - s < 0.12]
        if live:
            arr = np.asarray(img).copy()
            for (x, y, dw, dh) in live:
                arr[y:y + dh, x:x + dw] = 255 - arr[y:y + dh, x:x + dw]
            img = Image.fromarray(arr)
        img = img.resize((vw, vh), Image.LANCZOS)
        draw = ImageDraw.Draw(img, "RGBA")
        if finger:
            (fx, fy), since, down = finger
            age = t - since
            fade = 1.0 if down else max(0.0, 1 - age / 0.4)
            if fade > 0:
                r = 30 * scale * (1 if down else 1 + age)
                cx, cy = fx * scale, fy * scale
                draw.ellipse((cx - r, cy - r, cx + r, cy + r),
                             fill=(90, 90, 90, int(70 * fade)), outline=(60, 60, 60, int(140 * fade)), width=2)
        recent = [p for p in pen if p[0] <= t]
        if recent and t - recent[-1][0] < 0.35:
            _, px, py = recent[-1]
            cx, cy, r = px * scale, py * scale, 7
            draw.ellipse((cx - r, cy - r, cx + r, cy + r), outline=(40, 40, 40, 200), width=2)

        if sink:
            sink.stdin.write(img.tobytes())
        else:
            img.save(Path(a.frames) / f"{frame:05d}.png")
        frame += 1
        t += 1 / a.fps
        flashes = [f for f in flashes if t - f[0] < 0.2]
        pen = [p for p in pen if t - p[0] < 1.0]

    if sink:
        sink.stdin.close()
        sink.wait()
    print(f"{a.out if sink else a.frames}: {frame} frames, {frame / a.fps:.1f} s")


if __name__ == "__main__":
    main()
