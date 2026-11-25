import { createServer } from "node:http";
import { readFile, realpath, writeFile } from "node:fs/promises";
import { resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";

const [modulePath, bundle, pinFile, claimFile, output, smoke] =
  process.argv.slice(2);
const { LedgerClient } = await import(pathToFileURL(resolve(modulePath)));
const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");
const jsonFile = async (p) => JSON.parse(await readFile(p));
const bytes = await readFile(resolve(bundle, "manifest.json"));
const release = (await jsonFile(pinFile)).pins.find(
  (p) => p.manifestSha256 === sha(bytes),
);
if (!release) throw Error("manifest digest mismatch");
const manifest = JSON.parse(bytes);
if (
  release.identity !== manifest.identity ||
  release.version !== manifest.version ||
  manifest.ruleSetVersion !== 1 ||
  manifest.schemaVersion !== 1
)
  throw Error("manifest mismatch");
const payloads = new Map();
for (const [name, digest] of Object.entries(manifest.files)) {
  if (
    name.includes("\\") ||
    name.split("/").some((s) => !s || s === "." || s === "..")
  )
    throw Error("invalid payload path");
  const file = await realpath(resolve(bundle, name));
  if (!file.startsWith((await realpath(bundle)) + sep))
    throw Error("invalid payload path");
  const body = await readFile(file);
  if (sha(body) !== digest) throw Error("payload digest mismatch: " + name);
  payloads.set(name, body);
}
const { schemaVersion, claims } = await jsonFile(claimFile);
const supported = new Set([
  "urn:oddlang:ledger-http:LookupAccount",
  "urn:oddlang:ledger-http:TransferFunds",
]);
if (
  schemaVersion !== 1 ||
  !Array.isArray(claims) ||
  !claims.length ||
  claims.some(
    ({ operation, role }) =>
      role !== "http.consumer" || !supported.delete(operation),
  )
)
  throw Error("invalid consumer claim");
const stringify = (value) =>
  JSON.stringify(value, (_, v) => (typeof v === "bigint" ? v.toString() : v));
if (smoke) {
  const value = await new LedgerClient(smoke).lookup("A");
  await writeFile(output, stringify(value));
  process.exit(0);
}
function comparable(obs) {
  return obs.map((o) => {
    if (!o.request) return o;
    const req = {
      ...o.request,
      path: o.request.path.split("/").map(decodeURIComponent),
    };
    if (req.contentType === "application/json") {
      try {
        req.bodyBase64 = JSON.parse(Buffer.from(req.bodyBase64, "base64"));
      } catch {
        /* Retain malformed request bytes for the failure report. */
      }
    }
    return { ...o, request: req };
  });
}
const results = [];
for (const c of JSON.parse(payloads.get("cases.json"))) {
  if (!claims.some((p) => p.operation === c.operation && p.role === c.role))
    continue;
  const observations = [];
  const server = createServer(async (req, res) => {
    if (observations.length > c.action.replies.length) {
      res.destroy();
      return;
    }
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const request = {
      method: req.method,
      path: req.url,
      contentType: req.headers["content-type"] ?? null,
      bodyBase64: Buffer.concat(chunks).toString("base64"),
    };
    const reply = c.action.replies[observations.length];
    if (!reply) {
      observations.push({ request, response: null });
      res.destroy();
      return;
    }
    const bytes = Buffer.from(reply.bodyBase64, "base64");
    if (reply.contentType !== null)
      res.setHeader("Content-Type", reply.contentType);
    res.statusCode = reply.status;
    observations.push({
      request,
      response: {
        status: res.statusCode,
        contentType: res.getHeader("Content-Type") ?? null,
        bodyBase64: bytes.toString("base64"),
      },
    });
    res.end(bytes);
  });
  await new Promise((ok, err) =>
    server.once("error", err).listen(0, "127.0.0.1", ok),
  );
  try {
    const client = new LedgerClient(
      "http://127.0.0.1:" + server.address().port,
      (url, init) => fetch(url, { ...init, signal: AbortSignal.timeout(5000) }),
    );
    const input = c.action.input;
    const result = c.operation.endsWith(":LookupAccount")
      ? await client.lookup(input.id)
      : await client.transfer({ ...input, amount: BigInt(input.amount) });
    observations.push({ native: JSON.parse(stringify(result)) });
  } finally {
    server.closeAllConnections();
    await new Promise((ok) => server.close(ok));
  }
  const pass = isDeepStrictEqual(
    comparable(observations),
    comparable(c.expected),
  );
  results.push({
    caseId: c.caseId,
    outcome: pass ? "pass" : "fail",
    observations,
  });
}
await writeFile(
  output,
  JSON.stringify({
    schemaVersion: 1,
    release,
    ruleSetVersion: 1,
    claims,
    results,
  }),
);

if (results.some((r) => r.outcome !== "pass")) process.exitCode = 1;
