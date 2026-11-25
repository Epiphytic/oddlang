// Exercise the generated client with Chromium's native window.fetch.
const { chromium } = require(process.argv[2]);
const { readFileSync } = require("node:fs");
const assert = require("node:assert/strict");
(async () => {
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.route("http://ledger.test/**", (route) => {
      const path = new URL(route.request().url()).pathname;
      if (path === "/")
        return route.fulfill({
          body: "<html></html>",
          contentType: "text/html",
        });
      if (path === "/api/accounts/A")
        return route.fulfill({
          json: { id: "A", owner: "Alice", balance: "7" },
        });
      return route.fulfill({ status: 404 });
    });
    await page.goto("http://ledger.test/");
    await page.addScriptTag({
      content: "var exports = {};\n" + readFileSync(process.argv[3], "utf8"),
    });
    for (const prefix of ["/api", "/api/"]) {
      const actual = await page.evaluate(async (prefix) => {
        const client = new exports.LedgerClient(location.origin + prefix);
        const r = await client.lookup("A");
        return r.kind === "success" ? String(r.value.balance) : r.kind;
      }, prefix);
      assert.equal(actual, "7");
    }
  } finally {
    await browser.close();
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});
