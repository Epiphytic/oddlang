const fs = require("node:fs");
const { roundTrip } = require(process.argv[2]);
const cases = JSON.parse(fs.readFileSync(0, "utf8"));
const results = cases.map(({caseId, operation, action}) => {
  const bytes = Buffer.from(action.bodyBase64, "base64");
  let native;
  try {
    native = {kind: "success", value: roundTrip(operation, bytes)};
  } catch {
    native = {kind: "localRejection"};
  }
  return {caseId, outcome: "pass", observations: [{native}]};
});
process.stdout.write(JSON.stringify(results));
