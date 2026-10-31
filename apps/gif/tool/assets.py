# Writes the gif program's assets, so each frame's rectangle, disposal and
# delay is what the cases say, with no encoder's optimizing between.
#
#     python3 -I tool/assets.py
#
# GIFs by hand: a 4-color global palette (red, green, blue, and 3 for
# transparent), LZW with a clear code before every pixel, so the table
# never grows and every code is 3 bits. anim.webp is Pillow's.
import os

from PIL import Image

HERE = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'assets')
PALETTE = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0])
KEEP, BACKGROUND, PREVIOUS = 1, 2, 3


def u16(n):
    return bytes([n & 0xff, n >> 8])


def lzw(indices):
    bits, n, out = 0, 0, bytearray()
    for code in [c for i in indices for c in (4, i)] + [5]:
        bits |= code << n
        n += 3
        while n >= 8:
            out.append(bits & 0xff)
            bits >>= 8
            n -= 8
    if n:
        out.append(bits)
    blocks = bytearray([2])
    for i in range(0, len(out), 255):
        chunk = out[i:i + 255]
        blocks += bytes([len(chunk)]) + chunk
    return bytes(blocks + b'\0')


def gif(w, h, frames, loop=None):
    """frames: (left, top, width, height, palette index, disposal, delay
    in hundredths)."""
    out = b'GIF89a' + u16(w) + u16(h) + bytes([0x91, 0, 0]) + PALETTE
    if loop is not None:
        out += b'\x21\xff\x0bNETSCAPE2.0\x03\x01' + u16(loop) + b'\0'
    for left, top, fw, fh, index, dispose, delay in frames:
        out += bytes([0x21, 0xf9, 4, dispose << 2 | 1]) + u16(delay) + bytes([3, 0])
        out += b'\x2c' + u16(left) + u16(top) + u16(fw) + u16(fh) + b'\0'
        out += lzw([index] * (fw * fh))
    return out + b'\x3b'


def write(name, data):
    with open(os.path.join(HERE, name), 'wb') as f:
        f.write(data)


# Full-screen steps, red, green, blue, at 100, 200 and 150 ms, forever.
write('steps.gif', gif(40, 40, [
    (0, 0, 40, 40, 0, KEEP, 10),
    (0, 0, 40, 40, 1, KEEP, 20),
    (0, 0, 40, 40, 2, KEEP, 15),
], loop=0))
# Every disposal on three 10x10 cells, the NETSCAPE loop count 2.
write('dispose.gif', gif(30, 10, [
    (0, 0, 10, 10, 0, KEEP, 10),
    (10, 0, 10, 10, 1, BACKGROUND, 20),
    (20, 0, 10, 10, 2, PREVIOUS, 30),
    (0, 0, 30, 10, 3, 0, 40),
    (10, 0, 10, 10, 2, 0, 5),
], loop=2))
# Two frames, no NETSCAPE extension: played once. The second frame runs
# past the screen, and restoring to previous comes first.
write('once.gif', gif(20, 10, [
    (0, 0, 10, 10, 0, PREVIOUS, 7),
    (10, 5, 20, 20, 1, 0, 0),
]))
# An animated WebP: red, green, blue; 100, 200, 300 ms; loop count 3.
# Opaque: the stock engine's codec fails a frame with alpha after the
# first ("Could not getPixels for frame 1"); fl-image's tests cover alpha.
frames = [Image.new('RGBA', (20, 20), c) for c in
          [(255, 0, 0, 255), (0, 255, 0, 255), (0, 0, 255, 255)]]
frames[0].save(os.path.join(HERE, 'anim.webp'), save_all=True, append_images=frames[1:],
               duration=[100, 200, 300], loop=3, lossless=True)
