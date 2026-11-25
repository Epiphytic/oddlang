const assert = require("node:assert/strict");
const path = require("node:path");
const [clients, v1, v2] = process.argv.slice(2);
async function check() {
  for (const [clientVersion, expectedKey] of [["v1","balance"],["v2","balanceMinor"]]) {
    const {LedgerClient} = require(path.join(clients,clientVersion,"client.js"));
    for (const [providerVersion, url, emittedKey] of [["v1",v1,"balance"],["v2",v2,"balanceMinor"]]) {
      let wire;
      const client = new LedgerClient(url, async (...args) => {
        const response = await fetch(...args);
        assert.equal(response.status,200);
        wire = await response.clone().json();
        return response;
      });
      const result = await client.lookup("A");
      assert.deepEqual(wire, {id:"A", owner:"Alice", [emittedKey]:"10"});
      if (clientVersion === providerVersion) {
        assert.equal(result.kind,"success");
        assert.deepEqual({...result.value},{id:"A",owner:"Alice",balance:10n});
      } else {
        assert.equal(Object.hasOwn(wire,expectedKey),false);
        assert.deepEqual(result,{kind:"protocolError"});
      }
    }
  }
  const {LedgerClient} = require(path.join(clients,"v1","client.js"));
  let transfer;
  const mixed = new LedgerClient(v2, async (...args) => {
    const response = await fetch(...args);
    assert.equal(response.status,200);
    transfer = await response.clone().json();
    return response;
  });
  assert.deepEqual(await mixed.transfer({source:"A",destination:"B",amount:3n}),{kind:"protocolError"});
  assert.equal(transfer.source.balanceMinor,"7");
  assert.equal(transfer.destination.balanceMinor,"10");
  for (const [id, balance] of [["A","7"],["B","10"]]) {
    const response = await fetch(v2+"/accounts/"+id);
    assert.equal(response.status,200);
    assert.equal((await response.json()).balanceMinor,balance);
  }
}
check().catch(e => { console.error(e); process.exitCode = 1; });
