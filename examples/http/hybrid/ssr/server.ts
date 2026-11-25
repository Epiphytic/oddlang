import { createServer, request, type IncomingMessage, type ServerResponse } from "node:http";
import { readFile } from "node:fs/promises";
import { LedgerClient } from "../generated/client";

const provider = new URL(process.env.ODDLANG_PROVIDER_URL ?? "");
const port = Number(process.env.ODDLANG_HYBRID_PORT ?? "0");
const ledger = new LedgerClient(provider.href, (input, init) =>
  fetch(input, { ...init, signal: AbortSignal.timeout(5000) }),
);

function html(value: string): string {
  return value.replace(/[&<>"]/g, char =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[char]!,
  );
}

function proxy(incoming: IncomingMessage, outgoing: ServerResponse) {
  const headers = { ...incoming.headers, host: provider.host };
  const upstream = request(provider, {
    path: incoming.url,
    method: incoming.method,
    headers,
    timeout: 5000,
  }, response => {
    outgoing.writeHead(response.statusCode ?? 502, response.headers);
    response.pipe(outgoing);
  });
  upstream.on("timeout", () => upstream.destroy());
  upstream.on("error", () => {
    if (!outgoing.headersSent) outgoing.writeHead(502);
    outgoing.end();
  });
  incoming.pipe(upstream);
}

const web = new URL("../web/", import.meta.url);
const server = createServer(async (incoming, outgoing) => {
  const path = new URL(incoming.url ?? "/", "http://localhost").pathname;
  try {
    if (path === "/transfers" || path.startsWith("/accounts/")) {
      proxy(incoming, outgoing);
      return;
    }
    if (path === "/") {
      const accounts = await Promise.all(["A", "B"].map(id => ledger.lookup(id)));
      const rows = accounts.map(result => {
        if (result.kind !== "success") throw new Error("lookup failed");
        const { id, owner, balance } = result.value;
        return `<li data-account="${html(id)}">${html(owner)} <strong>${balance}</strong></li>`;
      });
      outgoing.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
      outgoing.end(`<!doctype html><html lang="en"><title>Ledger accounts</title><main><h1>Ledger accounts</h1><ul>${rows.join("")}</ul><a href="/web">Transfer funds</a></main></html>`);
      return;
    }
    if (path === "/web" || /^\/assets\/[\w.-]+\.js$/.test(path)) {
      const entry = path === "/web";
      const asset = await readFile(new URL(entry ? "index.html" : path.slice(1), web));
      outgoing.writeHead(200, {
        "Content-Type": entry ? "text/html; charset=utf-8" : "text/javascript; charset=utf-8",
      });
      outgoing.end(asset);
      return;
    }
    outgoing.writeHead(404).end();
  } catch {
    if (!outgoing.headersSent) outgoing.writeHead(502);
    outgoing.end();
  }
});
server.listen(port, "127.0.0.1", () => {
  const address = server.address();
  if (address && typeof address === "object") console.log(`listening ${address.address}:${address.port}`);
});
