"""Empties an MKV's HEVC CodecPrivate down to the bare 23-byte hvcC header (no parameter-set
arrays), the way some WEB rips ship it: the stream must then carry VPS/SPS/PPS in-band
(x265 repeat-headers). Rewritten in place: the freed bytes become an EBML Void element right
after it, so every offset in the file (SeekHead, Cues) stays valid. Usage: bare-hvcc.py <file>"""

import sys

CODEC_PRIVATE = b"\x63\xa2"
VOID = 0xEC


def read_size(buf: bytes, p: int) -> tuple[int, int]:
    first = buf[p]
    length = 1
    while length <= 8 and not first & (0x80 >> (length - 1)):
        length += 1
    value = first & (0xFF >> length)
    for i in range(1, length):
        value = (value << 8) | buf[p + i]
    return value, length


def write_size(value: int, length: int) -> bytes:
    return ((1 << (7 * length)) | value).to_bytes(length, "big")


def main(path: str) -> None:
    data = bytearray(open(path, "rb").read())
    p = data.find(CODEC_PRIVATE)
    while p != -1:
        size, size_len = read_size(data, p + 2)
        body = p + 2 + size_len
        # the HEVC one: configurationVersion 1, and room for arrays to drop
        if data[body] == 1 and size > 23 + 3:
            break
        p = data.find(CODEC_PRIVATE, p + 1)
    if p == -1:
        sys.exit(f"{path}: no HEVC CodecPrivate with parameter sets")
    data[p + 2 : body] = write_size(23, size_len)
    data[body + 22] = 0
    freed = size - 23
    # a Void: its ID, an 8-byte size, then filler
    data[body + 23 : body + size] = bytes([VOID]) + write_size(freed - 9, 8) + bytes(freed - 9)
    open(path, "wb").write(data)


if __name__ == "__main__":
    main(sys.argv[1])
