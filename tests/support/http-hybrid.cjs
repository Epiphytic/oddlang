const assert = require("node:assert/strict");

const [origin, playwrightPath, provider, balanceKey] = process.argv.slice(2);
const {chromium} = require(playwrightPath);

async function run() {
  let browser;
  try {
    const ssr = await fetch(origin, {signal: AbortSignal.timeout(5000)});
    assert.equal(ssr.status, 200);
    const html = await ssr.text();
    assert.match(html, /data-account="A">Alice <strong>10<\/strong>/);
    assert.match(html, /data-account="B">Bob <strong>7<\/strong>/);

    browser = await chromium.launch({headless: true});
    const page = await browser.newPage();
    page.setDefaultTimeout(6000);
    await page.goto(origin + "/web");
    await page.waitForFunction(() => document.querySelector('[data-account="A"] strong')?.textContent === "10");
    const amount = page.getByLabel("Amount from A to B");
    const transfer = page.getByRole("button", {name: "Transfer"});
    await amount.fill("3");
    const wireSuccess = page.waitForResponse(r => r.url() === origin + "/transfers");
    await transfer.click();
    const response = await wireSuccess;
    assert.equal(response.status(), 200);
    const result = await response.json();
    assert.equal(result.source[balanceKey], "7");
    assert.equal(result.destination[balanceKey], "10");
    assert.equal(Object.hasOwn(result.source, balanceKey === "balance" ? "balanceMinor" : "balance"), false);
    await page.waitForFunction(() => document.querySelector('[data-account="A"] strong')?.textContent === "7");
    assert.equal(await page.locator('[data-account="B"] strong').textContent(), "10");

    await amount.fill("8");
    await transfer.click();
    await page.getByText("urn:oddlang:ledger:error:InsufficientFunds", {exact: true}).waitFor();
    for (const [id, balance] of [["A", "7"], ["B", "10"]]) {
      const check = await fetch(provider + "/accounts/" + id, {signal: AbortSignal.timeout(5000)});
      assert.equal(check.status, 200);
      assert.equal((await check.json())[balanceKey], balance);
    }
  } finally {
    if (browser) await browser.close();
  }
}
run().catch(error => { console.error(error); process.exitCode = 1; });
