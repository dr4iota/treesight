#!/usr/bin/env python3
"""Regenerate app-icon.png, the one source `cargo tauri icon` expands.

Run from this directory:

    python3 app-icon.py && cargo tauri icon

Everything under icons/ is generated from the PNG this writes — edit the
geometry here, never the outputs. Kept to the standard library on purpose: an
icon that needs a package manager to rebuild is an icon nobody rebuilds.

The mark is a listing indenting to the right: three rules sharing a right
margin, each starting one step further in than the last. Flat colour and hard
edges, because at 32px an outline is mud.
"""
import struct, zlib

N = 1024
BG = (0x0d, 0x47, 0xa1)   # Material Blue 800
INK = (0xf0, 0xf6, 0xfc)

# Fractions of the canvas, so the geometry survives a change of N.
RIGHT = 0.875            # every rule ends here
BAR_H = 33 / 512      # the height the mark has always had, as a ratio
ROWS = ((0.25, 0.125), (0.50, 0.1875), (0.75, 0.25))   # (top, left)

def main():
    px = bytearray()
    for y in range(N):
        row = bytearray()
        for x in range(N):
            c = BG
            for top, left in ROWS:
                if top * N <= y < (top + BAR_H) * N and left * N <= x < RIGHT * N:
                    c = INK
                    break
            row += bytes(c) + b'\xff'
        px += b'\x00' + row

    def chunk(tag, body):
        c = tag + body
        return struct.pack('>I', len(body)) + c + struct.pack('>I', zlib.crc32(c))

    open('app-icon.png', 'wb').write(
        b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', N, N, 8, 6, 0, 0, 0))
        + chunk(b'IDAT', zlib.compress(bytes(px), 9))
        + chunk(b'IEND', b''))
    print(f'app-icon.png {N}x{N}')

if __name__ == '__main__':
    main()
