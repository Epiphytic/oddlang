"""Compose independently locked framing and facade producers for the Marmish workspace."""
import json
from pathlib import Path
import shutil
import tomllib

from prepare import ROOT, digest, prepare

FACADE_DEST = "crates/marmish-core/src/lib.rs"


def fields(row):
    return "".join(f"{k} = {json.dumps(v)}\n" for k,v in row.items())


def compose(upstream, frame_import, frame_projection, facade_import, facade_projection, out):
    prepare(upstream, frame_import, frame_projection, out)
    path = out / "project.toml"
    prior = tomllib.loads(path.read_text())
    store = out / "store"
    frame = {k:prior[k] for k in ("graphSha256","importSha256","projectionSha256")}
    facade = {
        "graphSha256": digest((facade_import/"domain.ttl").read_bytes()),
        "importSha256": digest((facade_import/"import.lock").read_bytes()),
        "projectionSha256": digest((facade_projection/"projection.json").read_bytes()),
    }
    release = json.loads((ROOT/"examples/marmish/facade-release.json").read_bytes())
    if release != facade:
        raise ValueError("facade package differs from the published lock")
    projection = json.loads((facade_projection/"projection.json").read_bytes())
    data = (facade_projection/"lib.rs").read_bytes()
    if projection != {"version":1,"graphSha256":facade["graphSha256"],"files":{"lib.rs":digest(data)}}:
        raise ValueError("facade module differs from its projection")
    shutil.copytree(facade_import,store/"imports"/facade["importSha256"])
    shutil.copytree(facade_projection,store/"projections"/facade["projectionSha256"])
    rows = prior["files"]
    replaced = next(row for row in rows if row["path"]==FACADE_DEST)
    original = replaced["sha256"]
    if replaced["producer"] != "retained" or digest(data)==original:
        raise ValueError("expected a retained source to replace")
    for row in rows:
        if row["producer"]=="generated":
            row["emitter"]="framing-v1"
    replaced.update(producer="generated",module="lib.rs",emitter="rust-facade-v1",sha256=digest(data))
    if all(row["producer"]!="retained" or row["sha256"]!=original for row in rows):
        (store/"retained"/original).unlink()
    header = {k:prior[k] for k in ("crates","target","features","cargoLockSha256")}
    body = "version = 2\n"+fields(header)
    for profile, pins in [("framing-v1",frame),("rust-facade-v1",facade)]:
        body += "\n[[emitters]]\n"+fields({"profile":profile,"package":"marmish-core",**pins})
    body += "".join("\n[[files]]\n"+fields(row) for row in rows)
    path.write_text(body)
    return path
