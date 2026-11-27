import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { chromium } from "playwright";

const [modules, input] = process.argv.slice(2);
const { cases, names } = JSON.parse(await readFile(input));
const files = Object.fromEntries(await Promise.all(names.map(async name =>
    [name, await readFile(path.join(modules, name))])));
const { replay } = await import(pathToFileURL(path.resolve(modules, "host.js")));
const nodeCases = await replay(cases, name => WebAssembly.compile(files[name]));
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
        const page = await browser.newPage();
        await page.goto("http://127.0.0.1:" + server.address().port);
        const observed = await page.evaluate(async scenarios => {
            const { replay } = await import("/host.js");
            return replay(scenarios, name => WebAssembly.compileStreaming(fetch("/" + name)));
        }, cases);
        process.stdout.write(JSON.stringify([
            { host: "node", version: process.version, cases: nodeCases },
            { host: "chromium", version: browser.version(), cases: observed },
        ]));
    } finally { await browser.close(); }
} finally { await new Promise(resolve => server.close(resolve)); }
