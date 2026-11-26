import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";

const { encode, decode, variantName, fieldNames, WireError } = await import(
  pathToFileURL(process.argv[2]).href
);
const bytes = (...values) => new Uint8Array(values);
const maximum = 18446744073709551615n;
const index = maximum - 2n;
const data = bytes(255);
const fixture = bytes(42, 0, 0, 0, 9, 255, 255, 255, 255, 255, 255, 255, 253, 255);
assert.deepEqual(encode(42, [index, data]), fixture);
assert.deepEqual(decode(fixture), { tag: 42, name: "Chunk", values: [index, data] });
assert.deepEqual(decode(Buffer.from(fixture)), { tag: 42, name: "Chunk", values: [index, data] });
assert.equal(variantName(62), "Ping");
assert.deepEqual(fieldNames(42), ["index", "data"]);
assert.deepEqual(encode(62, [bytes(1, 2, 3)]), bytes(62, 0, 0, 0, 3, 1, 2, 3));

const error = (action, code) => assert.throws(
  action,
  (err) => err instanceof WireError && err.code === code,
);
error(() => encode(42, [maximum, bytes()]), "Limit");
error(() => encode(42, [maximum, data]), "Range");
error(() => encode(42, [maximum + 1n, data]), "Invalid");
error(() => encode(42, [-1n, data]), "Invalid");
error(() => encode(42, [1, data]), "Invalid");
error(() => encode(42, [1n, [255]]), "Invalid");
error(() => encode(42, [1n, data, data]), "Invalid");
error(() => encode(42, [1n]), "Invalid");
error(() => encode(42, null), "Invalid");
error(() => encode("42", [index, data]), "Invalid");
error(() => encode(62, [bytes(1, 2)]), "Invalid");

error(() => decode([]), "Invalid");
error(() => decode(bytes(255, 0, 0, 0)), "Header");
error(() => decode(bytes(255, 0, 0, 0, 26)), "Limit");
error(() => decode(bytes(255, 0, 0, 0, 1)), "Length");
error(() => decode(bytes(255, 0, 0, 0, 0)), "Tag");
error(() => decode(bytes(62, 0, 0, 0, 2, 1, 2)), "Truncated");
error(() => decode(bytes(62, 0, 0, 0, 4, 1, 2, 3, 4)), "Trailing");
error(() => decode(bytes(42, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 255)), "Range");
error(() => decode(bytes(42, 0, 0, 0, 9, 255, 255, 255, 255, 255, 255, 255, 254, 255)), "Range");
