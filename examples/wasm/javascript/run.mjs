import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { chromium } from "playwright";

const [bundle, component, modules, output] = process.argv.slice(2);
const read = name => readFile(path.join(bundle, name));
const digest = bytes => createHash("sha256").update(bytes).digest("hex");
const manifestBytes = await read("manifest.json");
const manifest = JSON.parse(manifestBytes);
const workspace = JSON.parse(await read("workspace.lock.json"));
assert.equal(workspace.pins.find(pin => pin.identity === manifest.identity)?.manifestSha256, digest(manifestBytes));
const corpus = await read("cases.json");
assert.equal(digest(corpus), manifest.files["cases.json"]);
const cases = JSON.parse(corpus);
const expected = cases.map(({ caseId, imports, calls }) => ({
    caseId, imports, calls: calls.map(call => ({ function: call.function, arguments: call.arguments, result: call.expected })),
}));
const files = {};
const hashes = {};
for (const name of (await readdir(modules, { recursive: true })).sort()) {
    if (/\.(wasm|js|ts)$/.test(name)) {
        files[name] = await readFile(path.join(modules, name));
        hashes[name] = digest(files[name]);
    }
}
const jco = JSON.parse(await readFile(new URL("./node_modules/@bytecodealliance/jco/package.json", import.meta.url)));
assert.equal(jco.version, "1.42.0");
const metadata = {
    participant: path.basename(component, ".wasm"), roles: ["wasm.exporter", "wasm.importer"],
    ruleSetVersion: manifest.ruleSetVersion, jcoVersion: jco.version,
    hostDriverSha256: digest(await readFile(new URL(import.meta.url))),
    world: manifest.world, releaseSha256: digest(manifestBytes), componentSha256: digest(await readFile(component)),
    modules: hashes,
};
const { replay } = await import(pathToFileURL(path.resolve(modules, "host.js")));
const nodeCases = await replay(cases, name => WebAssembly.compile(files[name]));
assert.deepEqual(nodeCases, expected, "Node guest result or host transcript differs from the corpus");
await writeFile(output + "-node.json", JSON.stringify({ ...metadata, host: "node", version: process.version, cases: nodeCases }, null, 2));

const server = createServer((req, res) => {
    const name = req.url.slice(1);
    if (!name) { res.setHeader("Content-Type", "text/html"); res.end("<!DOCTYPE html><title>Oddlang test host</title>"); return; }
    if (!Object.hasOwn(files, name)) { res.writeHead(404).end(); return; }
    res.setHeader("Content-Type", name.endsWith(".wasm") ? "application/wasm" : "text/javascript");
    res.end(files[name]);
});
await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
try {
    const browser = await chromium.launch({ headless: true });
    try {
        assert.equal(browser.version(), "145.0.7632.6");
        const page = await browser.newPage();
        await page.goto("http://127.0.0.1:" + server.address().port);
        const observed = await page.evaluate(async scenarios => {
            const { replay } = await import("/host.js");
            return replay(scenarios, name => WebAssembly.compileStreaming(fetch("/" + name)));
        }, cases);
        assert.deepEqual(observed, expected, "Chromium guest result or host transcript differs from the corpus");
        await writeFile(output + "-chromium.json", JSON.stringify({ ...metadata, host: "chromium", version: browser.version(), cases: observed }, null, 2));
    } finally { await browser.close(); }
} finally { await new Promise(resolve => server.close(resolve)); }
