#!/usr/bin/env python3
"""Replay checked embedded stream delivery using a fixed field-meter peer."""
from base64 import b64encode
from hashlib import sha256
import json
from pathlib import Path
import tempfile

from check import ODDLANG, SUFFIX, checked, field, go, literal, serve, when


def body(value):
    return json.dumps(value, separators=(',', ':')).encode()


def encode(value):
    return b64encode(value).decode('ascii')


def wrap(payload, *, digest=None):
    return body({'encoded': encode(payload), 'digest': digest or sha256(payload).hexdigest()})


def main():
    stdout, stderr = b'measurement\x00', b'warning\xff'
    sample = {'type': 'measurement', 'indexes': [8, 9], 'binding': sha256(b'{"scale":"C","precision":2}').hexdigest(),
              'stdout': encode(stdout), 'stderr': encode(stderr), 'outCursor': len(stdout),
              'errCursor': len(stderr), 'produced': len(stdout), 'exit': 7}
    graph = {
        'format': 'oddlang-local-command-workflow-v1', 'id': 'urn:meter:workflow',
        'socket_argument': 'port', 'delivery': True,
        'transport': {'deadline_ms': 1500, 'maximum_frame_bytes': 1024, 'maximum_exchanges': 2},
        'errors': {name: {'exit': code, 'stderr': name} for code, name in enumerate(
            ('binding', 'io', 'peer', 'frame_size', 'frame_json', 'deadline', 'protocol', 'budget'), 6)},
        'entry': SUFFIX + 'step:read',
        'steps': [
            {'id': SUFFIX + 'step:read', 'request': [field('action', literal('read'))],
             'embedded': {'id': SUFFIX + 'embedded', 'bytes': ['encoded'], 'sha256': ['digest'], 'maximum_bytes': 512},
             'transitions': [
                 go('ack', [when('$opened.type', 'measurement'),
                            when('$opened.indexes.1', 9),
                            {'path': ['$opened', 'outCursor'], 'equals_path': ['$opened', 'produced']},
                            {'path': ['$opened', 'binding'], 'equals_source': {'kind': 'sha256', 'fields': [
                                field('scale', literal('C')), field('precision', literal(2))]}}], 'ack',
                    capture=[{'name': 'exit-status', 'path': ['$opened', 'exit'],
                              'value_kind': {'kind': 'exit_status'}}]) | {
                         'outputs': [{'id': SUFFIX + 'output:' + channel, 'channel': channel,
                                      'bytes': ['$opened', channel], 'cursor': ['$opened', cursor],
                                      'maximum_bytes': 32}
                                     for channel, cursor in [('stdout', 'outCursor'), ('stderr', 'errCursor')]],
                    }]},
            {'id': SUFFIX + 'step:ack', 'request': [field('confirm', literal('measurement'))],
             'transitions': [{'id': SUFFIX + 'transition:exit', 'when': [when('confirmed', True)],
                              'carry': ['exit-status'], 'effect': {'kind': 'exit_captured', 'name': 'exit-status'}}]},
        ],
    }
    overlay = {'format': 'oddlang-local-command-client-rust-v1', 'entry': 'measure',
               'delivery_provider': 'base64-sha256'}
    with tempfile.TemporaryDirectory(prefix='owd-', dir='/tmp') as directory:
        root = Path(directory)
        (root / 'graph.json').write_bytes(body(graph))
        (root / 'rust.json').write_bytes(body(overlay))
        product = root / 'projected'
        checked(ODDLANG, 'build', 'local-command-client', 'rust', root / 'graph.json', root / 'rust.json', product)
        source = next(product.glob('*.rs'))
        (root / 'src').mkdir()
        (root / 'src/generated.rs').write_bytes(source.read_bytes())
        (root / 'src/main.rs').write_text('''mod generated;
fn main() -> std::process::ExitCode {
    let command = clap::Command::new("field-meter").subcommand(
        clap::Command::new("read").arg(clap::Arg::new("port").long("port")
            .value_parser(clap::value_parser!(std::path::PathBuf))));
    let mut args = command.get_matches();
    generated::measure(args.remove_subcommand().unwrap().1)
}
''')
        (root / 'Cargo.toml').write_text('''[package]
name = "field-meter"
version = "1.0.0"
edition = "2024"
[dependencies]
base64 = "=0.22.1"
clap = "=4.6.7"
libc = "=0.2.189"
serde = { version = "=1.0.229", features = ["derive"] }
serde_json = "=1.0.151"
sha2 = "=0.11.2"
tokio = { version = "=1.54.1", features = ["io-util", "net", "rt", "time"] }
''')
        checked('cargo', '+1.97.1', 'generate-lockfile', '--offline', cwd=root)
        checked('cargo', '+1.97.1', 'build', '--offline', '--locked', '--release', cwd=root)
        binary = root / 'target/release/field-meter'
        path = root / 'sock'
        read = b'{"action":"read"}'
        confirm = b'{"confirm":"measurement"}'
        serve(binary, path, [(read, wrap(body(sample))), (confirm, b'{"confirmed":true}')],
              (7, stdout, stderr), unit=False)
        serve(binary, path, [(read, wrap(body(sample))), (confirm, b'{"confirmed":false}')],
              (12, stdout, stderr + b'protocol\n'), unit=False)
        for reply in (wrap(body(sample), digest='0' * 64),
                      wrap(body(sample | {'binding': '0' * 64})),
                      wrap(body(sample | {'errCursor': len(stderr) + 1})),
                      wrap(body(sample | {'stderr': '!!!!'})),
                      wrap(body(sample | {'exit': 256})),
                      wrap(body(sample | {'produced': len(stdout) + 1})),
                      wrap(body(sample | {'indexes': [8]})),
                      wrap(body(sample), digest=sha256(body(sample)).hexdigest().upper()),
                      body(json.loads(wrap(body(sample))) | {'$opened': sample}),
                      body(json.loads(wrap(body(sample))) | {'encoded': 'YR=='}),
                      wrap(body(sample | {'padding': 'x' * 512})),
                      wrap(body(sample)[:-1] + b',"exit":7}')):
            serve(binary, path, [(read, reply)], (12, b'', b'protocol\n'), unit=False)
        obligations = (product / 'workflow-obligations.json').read_bytes()
        pin = json.loads((product / 'projection.lock').read_bytes())
        assert pin['workflowObligationsSha256'] == sha256(obligations).hexdigest()
        assert json.loads(obligations)['delivery']['cursorOrigin'] == 0
    print('field-meter checked nested binding, binary output, cursor and acknowledgement passed')


if __name__ == '__main__':
    main()
