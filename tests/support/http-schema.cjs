// Offline standards checks use the exact Ajv and OpenAPI schema from setup.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const tools = path.resolve('target/http-tools');
const Ajv = require(path.join(tools, 'node_modules/ajv/dist/2020.js')).default;
const load = (root, name) => JSON.parse(fs.readFileSync(path.join(root, name)));
const release = process.argv[2];
const schema = load(release, 'schema.json');
const openapi = load(release, 'openapi.json');
const ajv = new Ajv({strict: false, validateFormats: false});
assert(ajv.validateSchema(schema), ajv.errorsText());
// This pinned standalone metaschema has one dynamic anchor and no overriding
// dialect. Resolve its four references before compiling; Ajv otherwise
// dispatches a nested Schema Object to its enclosing Parameter/Object rule.
const meta = load(tools, 'openapi-3.1-2022-10-07.json');
assert.equal(meta.$defs.schema.$dynamicAnchor, 'meta');
const normalized = JSON.parse(JSON.stringify(meta, (key, value) => {
    if (value && value.$dynamicRef === '#meta') return {$ref: '#/$defs/schema'};
    return value;
}));
const validateOpenapi = ajv.compile(normalized);
assert(validateOpenapi(openapi), ajv.errorsText(validateOpenapi.errors));
const negative = structuredClone(openapi);
negative.paths['/transfers'].post.requestBody.content = 'invalid';
assert(!validateOpenapi(negative));
const uri = 'urn:example:independent-schema-check';
ajv.addSchema(schema, uri);
const transfer = ajv.compile({$ref: uri + '#/$defs/urn:oddlang:ledger-http:Transfer'});
const account = ajv.compile({$ref: uri + '#/$defs/urn:oddlang:ledger-http:Account'});
const cases = load(release, 'cases.json');
const find = (operation, position, kind) => {
    const c = cases.find(c => c.role === 'http.provider' && c.operation.endsWith(operation)
        && c.slots.some(s => s.position === position && s.kind === kind));
    assert(c, [operation, position, kind].join(' '));
    return c.expected[0];
};
const body = raw => JSON.parse(Buffer.from(raw.bodyBase64, 'base64').toString('utf8'));
assert(account(body(find('LookupAccount', 'behavior', 'maximum').response)));
assert(transfer(body(find('TransferFunds', 'behavior', 'success').request)));
for (const kind of ['omitted','numeric','noncanonical']) {
    assert(!transfer(body(find('TransferFunds', 'request/amount', kind).request)));
}
const overflow = find('TransferFunds', 'request/amount', 'overflow');
assert(transfer(body(overflow.request)));
assert.equal(overflow.response.status, 400);
assert.deepEqual(schema.$defs['urn:oddlang:ledger-http:Transfer'].properties.amount['x-decimal-range'],
    ['-9223372036854775808', '9223372036854775807']);
