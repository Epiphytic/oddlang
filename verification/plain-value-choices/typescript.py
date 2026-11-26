"""Compare a source-free TypeScript projection with independent Rust choices."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

COMPILER = Path(sys.argv[1]).resolve()
TSC = ["npx", "--offline", "--package=typescript@5.9.3", "tsc"]
RUSTC = ["rustc", "+1.97.1", "--edition=2024"]
DOMAIN = "urn:fixture:survey:choices"
CASES = {"Sky": ["Clear", "Mist", "Cloud"], "Side": ["Left", "Right"], "Tier": ["Local", "Wide"]}
CAPS = {"Sky": "none", "Side": "partial", "Tier": "total"}

OBSERVER = '''
import { Sky, Side, Tier } from "./values";
const input = (value: unknown) => Sky.fromOrdinal(value);
for (const invalid of [-1, 3, 1.5, "0", null, undefined, {}, NaN, Infinity]) {
  if (input(invalid) !== undefined) throw new Error("invalid ordinal accepted");
}
if (input(0) === undefined || Sky.ordinal(input(0)!) !== 0 || 1 / Sky.ordinal(input(-0)!) !== Infinity)
  throw new Error("closed ordinal conversion changed");
if (!Object.isFrozen(Sky) || "equals" in Sky || "partialEquals" in Sky || "equals" in Side || "partialEquals" in Tier)
  throw new Error("comparison surface differs");
if (Sky.equality !== "none" || Side.equality !== "partial" || Tier.equality !== "total")
  throw new Error("published equality capability differs");
console.log(JSON.stringify({
  Sky: [Sky.ordinal(Sky.Clear), Sky.ordinal(Sky.Mist), Sky.ordinal(Sky.Cloud)],
  Side: [Side.ordinal(Side.Left), Side.ordinal(Side.Right), Side.partialEquals(Side.Left, Side.Left),
    Side.partialEquals(Side.Left, Side.Right)],
  Tier: [Tier.ordinal(Tier.Local), Tier.ordinal(Tier.Wide), Tier.equals(Tier.Wide, Tier.Wide),
    Tier.equals(Tier.Local, Tier.Wide)],
}));
'''
NEGATIVE = '''
import { Sky, Side, Tier } from "./values";
// @ts-expect-error An unchecked ordinal has no choice identity.
const wrong: Sky = 0;
// @ts-expect-error Choices with overlapping ordinals have distinct brands.
const mixed: Sky = Side.Left;
// @ts-expect-error Equality is absent on Sky.
Sky.equals(Sky.Clear, Sky.Cloud);
// @ts-expect-error Partial equality makes no total capability available.
Side.equals(Side.Left, Side.Right);
// @ts-expect-error Total equality still requires a pair of the same choice.
Tier.equals(Tier.Local, Side.Left);
// @ts-expect-error Exposed constants are immutable.
Sky.Clear = Sky.Cloud;
'''

RUST = r'''
#[derive(Clone, Copy)] enum Sky { INPUT }
#[derive(Clone, Copy, PartialEq)] enum Side { Left, Right }
#[derive(Clone, Copy, PartialEq, Eq)] enum Tier { Local, Wide }
fn main() {
    println!(
        "{{\"Sky\":[{},{},{}],\"Side\":[{},{},{},{}],\"Tier\":[{},{},{},{}]}}",
        Sky::Clear as isize, Sky::Mist as isize, Sky::Cloud as isize,
        Side::Left as isize, Side::Right as isize, Side::Left == Side::Left, Side::Left == Side::Right,
        Tier::Local as isize, Tier::Wide as isize, Tier::Wide == Tier::Wide, Tier::Local == Tier::Wide,
    );
}
'''


def run(*command, cwd=None, check=True):
    process = subprocess.run(command, cwd=cwd, capture_output=True, timeout=90)
    if check and process.returncode:
        raise RuntimeError((process.stdout + process.stderr).decode(errors="replace"))
    return process


def write(path, data):
    path.write_text(json.dumps(data, indent=2) + "\n")


def fixtures(sky):
    types, names, variants, rust_types = {}, {}, {}, {}
    for name, labels in (CASES | {"Sky": sky}).items():
        id = f"{DOMAIN}:{name}"
        ids = [f"{id}:{label}" for label in labels]
        types[id] = {"kind": "choice", "cases": ids, "equality": CAPS[name]}
        names[id] = name
        variants.update(zip(ids, labels))
        rust_types[id] = {"name": name, "order": ids, "traits": ["copy"]}
    graph = {"format": "oddlang-checked-values-v1", "id": DOMAIN, "types": types}
    binding = {"format": "oddlang-checked-values-typescript-v1", "types": names, "variants": variants}
    rust = {"format": "oddlang-checked-values-rust-v1", "types": rust_types, "variants": variants, "errors": {}}
    return graph, binding, rust


def verify(root, sky):
    root.mkdir()
    graph, binding, rust = fixtures(sky)
    manifest, overlay = root / "graph.json", root / "typescript.json"
    write(manifest, graph)
    write(overlay, binding)
    out = root / "ts-first"
    run(COMPILER, "build", "checked-values", "typescript", manifest, overlay, out)
    write(overlay, {**binding, "variants": dict(reversed(list(binding["variants"].items())))})
    again = root / "ts-second"
    run(COMPILER, "build", "checked-values", "typescript", manifest, overlay, again)
    for file in ("values.ts", "obligations.json", "projection.lock"):
        assert (out / file).read_bytes() == (again / file).read_bytes()
    lock = json.loads((out / "projection.lock").read_bytes())
    for field, file in (("sourceSha256", "values.ts"), ("obligationsSha256", "obligations.json")):
        assert hashlib.sha256((out / file).read_bytes()).hexdigest() == lock[field]

    native = root / "rust.json"
    write(native, rust)
    run(COMPILER, "build", "checked-values", "rust", manifest, native, root / "native")
    assert (root / "native/obligations.json").read_bytes() == (out / "obligations.json").read_bytes()
    (root / "reference.rs").write_text(RUST.replace("INPUT", ", ".join(sky)))
    binary = root / "reference"
    run(*RUSTC, root / "reference.rs", "-o", binary)
    original = run(binary).stdout

    (out / "observer.ts").write_text(OBSERVER)
    (out / "negative.ts").write_text(NEGATIVE)
    flags = ["--strict", "--target", "ES2022", "--module", "commonjs"]
    run(*TSC, *flags, "--noEmit", "negative.ts", cwd=out)
    run(*TSC, *flags, "--declaration", "--outDir", "compiled", "observer.ts", cwd=out)
    compiled = run("node", out / "compiled/observer.js").stdout
    assert compiled == original
    return json.loads(compiled)


assert run(*TSC, "--version").stdout.strip() == b"Version 5.9.3"
with tempfile.TemporaryDirectory(prefix="odd-choice-ts-") as temporary:
    root = Path(temporary)
    first = verify(root / "baseline", CASES["Sky"])
    changed = verify(root / "mutated", ["Cloud", "Clear", "Mist"])
    assert first["Sky"] == [0, 1, 2] and changed["Sky"] == [1, 2, 0]
    assert first["Side"] == changed["Side"] and first["Tier"] == changed["Tier"]
    graph, overlay, _ = fixtures(CASES["Sky"])
    manifest, target = root / "bad-graph.json", root / "bad-overlay.json"
    write(manifest, graph)
    duplicate = json.loads(json.dumps(graph))
    choices = duplicate["types"][f"{DOMAIN}:Sky"]["cases"]
    choices[2] = choices[0]
    wrong = [
        (duplicate, overlay, b"repeated checked value identity"),
        (graph, {**overlay, "types": {**overlay["types"], f"{DOMAIN}:Sky": "arbitrary text"}}, b"invalid TypeScript choice name"),
        (graph, {**overlay, "variants": {**overlay["variants"], f"{DOMAIN}:Unknown": "Unknown"}}, b"unused TypeScript choice case"),
    ]
    for changed_graph, binding, message in wrong:
        write(manifest, changed_graph)
        write(target, binding)
        failure = run(COMPILER, "build", "checked-values", "typescript", manifest, target,
                      root / "invalid-output", check=False)
        assert failure.returncode and message in failure.stderr and not (root / "invalid-output").exists()
print("Rust and TypeScript agree on both choice revisions, capability checks and exact ordinals.")
