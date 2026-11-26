"""Reproducible stream stimuli and expected observations from independent arithmetic."""
import struct

from spec_cases import MAX_OFFSET, case, error, frame, invalid, sha


class Seed:
    def __init__(self, seed):
        self.bytes = bytes.fromhex(seed)
        if len(self.bytes) != 32:
            raise ValueError("seed must be 32 bytes")
        self.counter = 0

    def take(self, count):
        blocks = []
        for _ in range((count + 31) // 32):
            blocks.append(sha(self.bytes + self.counter.to_bytes(8, "big")))
            self.counter += 1
        return b"".join(blocks)[:count]

    def below(self, count):
        return int.from_bytes(self.take(8), "big") % count


def fragment_observation(consumed, record=None):
    return {"consumed": consumed, "frame": record}


def data_cases(source, index):
    raw = source.take(1 + source.below(129))
    start = (0, MAX_OFFSET - len(raw), source.below(1 << 32))[index % 3]
    first = {"kind": "data", "offset": start, "bytes": raw.hex()}
    second = {"kind": "data", "offset": 71, "bytes": source.take(5).hex()}
    packet = frame(5, struct.pack(">Q", start) + raw)
    packet2 = frame(5, struct.pack(">Q", 71) + bytes.fromhex(second["bytes"]))
    cut = 1 + source.below(len(packet) - 1)
    tag = 12 + source.below(244)
    bad_tag = bytes([tag]) + packet[1:]
    # Header, lengths and byte counts are explicit. A second complete record
    # in one supplied slice still requires a separate call to consume its bytes.
    return [
        case(f"seed-{index}-data-write", "frame_encode", first, packet.hex()),
        case(f"seed-{index}-data-read", "frame_decode", packet.hex(), first),
        case(f"seed-{index}-fragmented", "frame_fragments",
             [packet[:cut].hex(), (packet[cut:] + packet2).hex(), packet2.hex(), "finish"], [
                 fragment_observation(cut), fragment_observation(len(packet) - cut, first),
                 fragment_observation(len(packet2), second), "ok",
             ]),
        case(f"seed-{index}-uncompleted-finish-retry", "frame_fragments",
             [packet[:cut].hex(), "finish", packet[cut:].hex(), "finish"], [
                 fragment_observation(cut), invalid("copy_truncated"),
                 fragment_observation(len(packet) - cut, first), "ok",
             ]),
        case(f"seed-{index}-unknown-tag", "frame_fragments", [bad_tag.hex(), packet.hex(), "finish"], [
             invalid("copy_frame_tag"), invalid("copy_decoder_failed"), invalid("copy_truncated"),
        ]),
        case(f"seed-{index}-truncated-tag-precedence", "frame_decode", bad_tag[:-1].hex(),
             invalid("copy_frame_length")),
        case(f"seed-{index}-offset-overflow", "frame_decode",
             frame(5, struct.pack(">Q", MAX_OFFSET - len(raw) + 1) + raw).hex(), error("Offset")),
    ]


def window_case(source, index):
    raw = source.take(2 + source.below(80))
    end = len(raw)
    split = 1 + source.below(end - 1)
    altered = bytes([raw[0] ^ 1]) + raw[1:]
    receive = lambda accepted, delivered, eof=None: {
        "accepted": accepted, "delivered": delivered, "buffered": accepted - delivered,
        "credit": 262144 - accepted + delivered if eof is None else 0,
        "eof": eof, "drained": eof == delivered,
    }
    steps = [
        ({"op": "accept", "offset": 0, "bytes": raw.hex()}, True),
        ({"op": "deliver", "offset": split}, "ok"),
        ({"op": "accept", "offset": 0, "bytes": altered.hex()}, error("Conflict")),
        ({"op": "state"}, receive(end, split)),
        ({"op": "finish", "offset": end + 1}, error("Eof")),
        ({"op": "accept", "offset": end + 1, "bytes": "00"}, error("Gap")),
        ({"op": "deliver", "offset": end + 1}, error("Ack")),
        ({"op": "state"}, receive(end, split)),
        ({"op": "accept", "offset": 0, "bytes": raw.hex()}, False),
        ({"op": "finish", "offset": end}, "ok"),
        ({"op": "accept", "offset": end, "bytes": "00"}, error("Eof")),
        ({"op": "accept", "offset": 0, "bytes": raw.hex()}, False),
        ({"op": "pending", "limit": end}, raw[split:].hex()),
        ({"op": "deliver", "offset": end}, "ok"),
        ({"op": "state"}, receive(end, end, end)),
    ]
    operations, expected = zip(*steps)
    return case(f"seed-{index}-receive-retry", "receive_window", list(operations), list(expected))


def capacity_case():
    operations = [{"op": "accept", "offset": n, "bytes": "66"} for n in range(256)]
    operations.extend([
        {"op": "state"}, {"op": "accept", "offset": 256, "bytes": "66"},
        {"op": "deliver", "offset": 1}, {"op": "accept", "offset": 256, "bytes": "77"},
        {"op": "accept", "offset": 0, "bytes": "66"}, {"op": "accept", "offset": 1, "bytes": "66"},
        {"op": "state"},
    ])
    return case("receipt-capacity-eviction", "receive_window", operations, [
        *([True] * 256),
        {"accepted": 256, "delivered": 0, "buffered": 256, "credit": 0, "eof": None, "drained": False},
        error("Limit"), "ok", True, error("OldRetry"), False,
        {"accepted": 257, "delivered": 1, "buffered": 256, "credit": 0, "eof": None, "drained": False},
    ])


def cases(seed, rounds):
    source = Seed(seed)
    return [
        row
        for index in range(rounds)
        for row in [*data_cases(source, index), window_case(source, index)]
    ] + [capacity_case()]
