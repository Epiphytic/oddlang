const assert = require("node:assert/strict");
const {execFileSync} = require("node:child_process");
const {decodeAccount, transfer} = require("./dist/generated/contract.js");
const {account, transfer: cases, version} = require("./generated/vectors.json");
const [service, cli] = process.argv.slice(2);

function call(exe, args, input) {
  return JSON.parse(execFileSync(exe, args, {input: JSON.stringify(input), encoding: "utf8"}));
}
const cliPayload = call(cli, [], account);
const result = call(service, ["account"], cliPayload);
assert.deepEqual(cliPayload, account);
assert.deepEqual(result, account);
const decoded = decodeAccount(result);
assert.equal(decoded.balance, 9223372036854775807n);
assert.throws(() => decodeAccount({...result, balance: Number(result.balance)}));
if (version === "v2") {
  assert.equal(decoded.currencyCode, account.currencyCode);
  const {currencyCode, ...incomplete} = result;
  assert.throws(() => decodeAccount(incomplete));
  assert.throws(() => decodeAccount({...result, currencyCode:""}));
  assert.equal(transfer, undefined);
  assert.equal(cases, undefined);
  process.exit(0);
}
const send = request => call(service, ["transfer"], {state: cases.state, request});
function invoke(request) {
  return transfer(send, request.source, request.destination, BigInt(request.amount));
}
const success = invoke(cases.request);
assert.equal(success.ok, cases.success.ok);
for (const [id, value] of Object.entries(cases.success.balances)) {
  assert.equal(success.balances[id], BigInt(value));
}
assert.deepEqual(invoke(cases.errorRequest), cases.error);
