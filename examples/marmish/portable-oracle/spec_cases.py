"""Executable expectations authored from the pinned stream and copy layouts."""
import base64
import hashlib
import json
import struct


MAX_OFFSET = (1 << 63) - 1
MIN_RETENTION = 604860


def canonical(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def sha(data):
    return hashlib.sha256(data).digest()


def b64(data):
    return base64.b64encode(data).decode("ascii")


def case(name, operation, data, expected):
    return {"name": name, "operation": operation, "input": data, "expected": expected}


def error(name, detail=None):
    if detail is None:
        return {"error": name}
    return {"error": name, "detail": detail}


def invalid(detail):
    return error("Invalid", detail)


def counted(data):
    return struct.pack(">I", len(data)) + data


def frame(tag, data):
    return bytes([tag]) + counted(data)


def field_bytes(value, *keys):
    return b"".join(bytes.fromhex(value[key]) for key in keys)


def record_cases():
    account = {"transfer": "11" * 32, "client": "22" * 32, "server": "33" * 32}
    flags = {"overwrite": False, "preserve_mode": True, "preserve_mtime": True}
    record = {
        "version": 1, **account, "direction": "get", "source": b64(b"a\xff"),
        "destination": b64(b"b/c"), **flags,
    }
    launch = {"version": 1, "mode": "resume", **{k: v for k, v in record.items() if k != "version"}}
    binding_bytes = field_bytes(record, "client", "server", "transfer") + b"\x02"
    namespace = ".marmish-copy-v1-" + sha(b"marmish-copy-state/1\0" + binding_bytes).hex()
    body = (
        field_bytes(record, "transfer", "client", "server") + b"\x02"
        + counted(b"a\xff") + counted(b"b/c") + b"\0\x01\x01"
    )
    launch_bytes = struct.pack(">IB", 1, 2) + body + b"\x01"
    launch_hash = sha(b"marmish-copy-launch/1\0" + launch_bytes)
    checksum = sha(b"test")
    manifest = {
        **record, "size": 4, "sha256": checksum.hex(), "mode": 0o640,
        "mtime": {"seconds": -2, "nanos": 999999999},
    }
    manifest_bytes = (
        struct.pack(">I", 1) + body + struct.pack(">Q", 4) + checksum
        + struct.pack(">BIBqI", 1, 0o640, 1, -2, 999999999)
    )
    identities = [{"device": 101, "inode": n} for n in (202, 203, 204)]
    checkpoint = {
        "manifest": manifest, "phase": 5, "normalized_path": "622f63",
        "parent": identities[0], "private": identities[1], "data": identities[2],
        "durable_offset": 4, "prefix_sha256": checksum.hex(), "last_stream": "44" * 16,
        "created": -4, "updated": -3, "expires": MIN_RETENTION - 3,
        "committed_final": identities[2],
    }
    checkpoint_payload = (
        struct.pack(">I", 1) + counted(manifest_bytes) + b"\x05" + counted(b"b/c")
        + struct.pack(">7Q", 101, 202, 101, 203, 101, 204, 4)
        + checksum + b"\x44" * 16 + struct.pack(">3q", -4, -3, MIN_RETENTION - 3)
        + struct.pack(">BQQ", 1, 101, 204)
    )
    checkpoint_bytes = checkpoint_payload + sha(b"marmish-copy-checkpoint/1\0" + checkpoint_payload)
    context = {
        "context_version": 1, "helper_protocol": 1, "client": account["client"],
        "server": account["server"], "group": "ff00", "session": "55" * 16,
        "stream": "66" * 16, "transfer": account["transfer"], "direction": 2,
        "launch_digest": launch_hash.hex(), "retention_seconds": MIN_RETENTION,
        "expected_version": "0.2.3", "expected_target": "aarch64-apple-darwin",
        "expected_commit": "77" * 20,
    }
    context_bytes = (
        b"marmish-copy-context/1\0" + struct.pack(">II", 1, 1)
        + field_bytes(context, "client", "server") + counted(b"\xff\0")
        + field_bytes(context, "session", "stream", "transfer") + b"\x02" + launch_hash
        + struct.pack(">Q", MIN_RETENTION) + counted(b"0.2.3")
        + counted(b"aarch64-apple-darwin") + b"\x77" * 20 + counted(b"__copy-helper-v1")
    )
    test_hash = {"sha256": checksum.hex()}
    test_offset = {"offset": 4, **test_hash}
    test_size = {"size": 4, **test_hash}
    offset_hash = struct.pack(">Q", 4) + checksum
    frames = [
        (1, {"kind": "start", "launch": launch, "codec": "zlib"}, launch_bytes),
        (2, {"kind": "manifest", "manifest": manifest}, manifest_bytes),
        (3, {"kind": "resume", "phase": 2, **test_offset}, b"\x02" + offset_hash),
        (4, {"kind": "prefix", **test_offset}, offset_hash),
        (5, {"kind": "data", "offset": 4, "bytes": "ff"}, struct.pack(">Q", 4) + b"\xff"),
        (6, {"kind": "checkpoint", **test_offset}, offset_hash),
        (7, {"kind": "done", **test_size}, offset_hash),
        (8, {"kind": "commit", "transfer": account["transfer"], **test_size},
         b"\x11" * 32 + offset_hash + b"\x01"),
        (9, {"kind": "finish", "transfer": account["transfer"], **test_hash}, b"\x11" * 32 + checksum),
        (10, {"kind": "progress", "stage": "PrefixHash", "examined": 1, "total": 4},
         struct.pack(">BQQ", 2, 1, 4)),
        (11, {"kind": "error", "code": "HashMismatch"}, b"\x09\0"),
    ]
    cases = []
    for tag, shape, payload in frames:
        kind = shape["kind"]
        encoded = frame(tag, payload).hex()
        cases += [
            case(f"spec-frame-{kind}-encode", "frame_encode", shape, encoded),
            case(f"spec-frame-{kind}-decode", "frame_decode", encoded, shape),
        ]
    for name, shape, encoded in [
        ("manifest", manifest, manifest_bytes),
        ("checkpoint", checkpoint, checkpoint_bytes),
        ("context", context, context_bytes),
    ]:
        cases += [
            case(f"spec-record-{name}-validate", f"{name}_validate", shape, "ok"),
            case(f"spec-record-{name}-encode", f"{name}_encode", shape, encoded.hex()),
            case(f"spec-record-{name}-decode", f"{name}_decode", encoded.hex(), shape),
        ]
    changed_checksum = checkpoint_bytes[:-1] + bytes([checkpoint_bytes[-1] ^ 1])
    matched = {"launch": launch, "manifest": manifest}
    start = {"context": context, "launch": launch, "codec": "zlib"}
    build = {"context": context, "version": "0.2.3", "target": "aarch64-apple-darwin", "commit": "77" * 20}
    cases += [
        case("launch-validate", "launch_validate", launch, "ok"),
        case("launch-invalid-version", "launch_validate", launch | {"version": 0}, invalid("copy_version")),
        case("launch-digest", "launch_digest", {"launch": launch, "codec": "zlib"}, launch_hash.hex()),
        case("launch-namespace", "launch_namespace", launch, namespace),
        case("manifest-namespace", "manifest_namespace", manifest, namespace),
        case("launch-matches-manifest-ignores-attempt-mode", "launch_matches_manifest", matched, True),
        case("launch-mismatches-manifest", "launch_matches_manifest",
             matched | {"manifest": manifest | {"overwrite": True}}, False),
        case("manifest-offset-before-metadata", "manifest_validate", manifest | {
             "size": MAX_OFFSET + 1, "mode": None,
        }, error("Offset")),
        case("manifest-mode-required", "manifest_validate", manifest | {"mode": None}, invalid("copy_metadata")),
        case("manifest-nanos-plus-one", "manifest_validate", manifest | {
             "mtime": {"seconds": -2, "nanos": 1000000000},
        }, invalid("copy_metadata")),
        case("checkpoint-corrupt-checksum", "checkpoint_decode", changed_checksum.hex(),
             invalid("copy_checkpoint_checksum")),
        case("checkpoint-invalid-offset", "checkpoint_validate", checkpoint | {"durable_offset": 3},
             invalid("copy_checkpoint")),
        case("checkpoint-retention-minus-one", "checkpoint_validate",
             checkpoint | {"expires": MIN_RETENTION - 4}, invalid("copy_checkpoint")),
        case("context-start", "context_start", start, "ok"),
        case("context-different-start-codec", "context_start", start | {"codec": "none"},
             invalid("copy_context_launch")),
        case("context-build", "context_build", build, "ok"),
        case("context-different-build-commit", "context_build", build | {"commit": "66" * 20},
             invalid("copy_context_build")),
        case("context-retention-minus-one", "context_validate",
             context | {"retention_seconds": MIN_RETENTION - 1}, invalid("copy_context")),
        case("context-retention-maximum", "context_validate",
             context | {"retention_seconds": 365 * 86400}, "ok"),
        case("context-retention-plus-one", "context_validate",
             context | {"retention_seconds": 365 * 86400 + 1}, invalid("copy_context")),
        case("context-unsupported-entrypoint", "context_decode",
             (context_bytes[:-1] + b"x").hex(), invalid("copy_context_entrypoint")),
    ]
    return cases, launch


def wire_cases(launch):
    now = 900000
    binding = {
        "stream": struct.pack(">Q", now).hex() + "11" * 8,
        "session": "12" * 16, "client": launch["client"], "server": launch["server"],
    }
    request_sha = "aa" * 32
    request = {
        "hello": {"codecs": ["none", "zlib"]},
        "copy_hello": {"versions": [1, 2], "codecs": ["none"]},
        "open_copy": {"launch": launch, "codec": "zlib"},
        "open": {"argv": [b64(b"/p"), b64(b"\xff")], "codec": "none"},
        "exchange": {"stdin": None, "stdin_eof": 0, "stdout_ack": 0, "stderr_ack": 0},
        "cancel": None,
        "status": None,
    }
    response = {
        "copy_hello": {"version": 1, "codecs": ["none"], "frame_bytes": 8192, "window_bytes": 262144},
        "copy_refused": {"code": "busy", "prior_binding": binding},
        "hello": {"codecs": ["none"], "frame_bytes": 8192, "window_bytes": 262144},
        "opened": {"codec": "zlib", "open_sha256": "bb" * 32, "disposition": "existing"},
        "exchange": {
            "stdin_accepted": 0, "stdin_delivered": 0, "stdin_closed": True,
            "stdout": None, "stderr": None, "stdout_eof": 0, "stderr_eof": 0,
            "cancel_requested": False, "terminal": {"kind": "exited", "data": {"code": 7}},
        },
        "error": {"code": "closed"},
    }
    cases = []
    for direction, bodies in [("request", request), ("response", response)]:
        for operation, data in bodies.items():
            body = {"op": operation}
            if data is not None:
                body["data"] = data
            shape = {"binding": binding, "sequence": 42, "body": body}
            message_data = body
            if direction == "response":
                shape["request_sha256"] = request_sha
                message_data = {"request_sha256": request_sha, "body": body}
            envelope = canonical({
                "protocol": "marmish-stream/1", "version": 1,
                "binding": binding, "sequence": 42,
                "message": {"direction": direction, "data": message_data},
            })
            cases += [
                case(f"spec-{direction}-{operation}-encode", f"{direction}_encode", shape, envelope),
                case(f"spec-{direction}-{operation}-decode", f"{direction}_decode", envelope, shape),
            ]
    open_body = {"op": "open", "data": request["open"]}
    cases.append(case("open-digest", "open_digest", {"argv": ["2f70", "ff"], "codec": "none"},
                      sha(canonical(open_body).encode()).hex()))
    for elapsed in [-61, -60, -59, 604799, 604800, 604801]:
        expected = "ok" if -60 <= elapsed <= 604800 else invalid("stream_age")
        cases.append(case(f"open-age-{elapsed}", "request_age", {
            "binding": binding, "sequence": 42, "body": open_body, "now": now + elapsed,
        }, expected))
    cases += [
        case("open-argv-before-age", "request_age", {
            "binding": binding, "sequence": 42, "body": {"op": "open", "data": {"argv": [], "codec": "none"}},
            "now": 0,
        }, error("Limit")),
        case("exchange-ack-before-unclean-terminal", "response_encode", {
            "binding": binding, "sequence": 42, "request_sha256": request_sha,
            "body": {"op": "exchange", "data": response["exchange"] | {
                "stdin_delivered": 1, "stdin_closed": False,
            }},
        }, error("Ack")),
    ]
    return cases


def codec_cases():
    shape = {"offset": MAX_OFFSET - 1, "codec": "none", "raw_len": 1, "sha256": sha(b"x").hex(), "data": "eA=="}
    return [
        case("codec-block-end-boundary", "codec_end", shape, MAX_OFFSET),
        case("codec-block-end-overflow", "codec_end", shape | {"offset": MAX_OFFSET}, error("Offset")),
        case("codec-block-is-valid", "codec_validate", shape, "ok"),
        case("codec-block-length-before-offset", "codec_validate", shape | {
            "offset": MAX_OFFSET, "raw_len": 8193,
        }, error("Limit")),
        case("codec-block-offset-before-codec", "codec_validate", shape | {
            "offset": MAX_OFFSET, "codec": "zlib",
        }, error("Offset")),
        case("codec-block-validate-does-not-check-hash", "codec_validate",
             shape | {"sha256": "00" * 32}, "ok"),
        case("codec-block-decode-does-check-hash", "codec_decode", {
            "block": shape | {"sha256": "00" * 32}, "negotiated": "none",
        }, error("Codec")),
    ]


def cases():
    records, launch = record_cases()
    observed = records + wire_cases(launch) + codec_cases() + [
        case("public-constants", "constants", None, {
            "MAX_BLOCK": 8192, "MAX_ENCODED": 8192, "MAX_CONTENT": 32768,
            "WINDOW_BYTES": 262144, "MAX_OFFSET": MAX_OFFSET, "MAX_RECORDS": 256,
            "PROTOCOL": "marmish-stream/1", "FORMAT": 1, "VERSION": 1, "MAX_PATH": 4096,
            "MAX_CONTROL": 16384, "DURABLE_WINDOW": 2097152, "CHECKPOINT_INTERVAL": 1048576,
            "MIN_RETENTION_SECS": MIN_RETENTION, "MAX_RETENTION_SECS": 31536000,
            "CONTEXT_VERSION": 1, "MAX_CONTEXT": 4096, "HELPER_ENTRYPOINT": "__copy-helper-v1",
        }),
        case("numbered-enums-all-u8", "enum_conversions", None, {
            "AttemptMode": ["Create", "Resume"], "Direction": ["Put", "Get"],
            "Phase": ["Created", "Receiving", "Verifying", "CommitPrepared", "Committed"],
            "Stage": ["Hashing", "PrefixHash", "Verifying", "Syncing"],
            "CopyError": ["Unsupported", "Protocol", "PermissionDenied", "NotFound", "UnsafePath",
                          "Busy", "Conflict", "SourceChanged", "HashMismatch", "DiskFull", "Quota",
                          "CommitUncertain", "LocalIo", "IdentityChanged"],
        }),
    ]
    # Enumerate the entire u8 domain, including each invalid discriminant.
    entries = observed[-1]["expected"]
    for kind, valid in entries.items():
        entries[kind] = [invalid("copy_enum"), *valid, *([invalid("copy_enum")] * (255 - len(valid)))]
    return observed
