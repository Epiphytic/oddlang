import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { runInNewContext } from "node:vm";

const [modulePath, id, expectedWidth, failure] = process.argv.slice(2);
const { fromBytes, FixedBytesError } = await import(pathToFileURL(modulePath));
const width = Number(expectedWidth);
const wrong = (error) => error instanceof FixedBytesError && error.id === failure;
const cases = {};

if (width > 0) {
    assert.throws(() => fromBytes(id, new Uint8Array(width - 1)), wrong);
    cases.short = failure;
}
assert.throws(() => fromBytes(id, new Uint8Array(width + 1)), wrong);
cases.long = failure;
assert.throws(() => fromBytes(id, new Uint8ClampedArray(width)), TypeError);
assert.throws(() => fromBytes("urn:unknown:value", new Uint8Array(width)), RangeError);

const expected = Array.from({ length: width }, (_, index) => index % 251);
const input = Buffer.from(expected);
Object.defineProperty(input, "length", { value: width + 99 });
input[Symbol.iterator] = () => { throw new Error("use the native byte array"); };
const value = fromBytes(id, input);
assert.equal(value.id, id);
assert.throws(() => { value.id = "urn:unknown:value"; }, TypeError);
Uint8Array.prototype.fill.call(input, 255);
const view = value.copyBytes();
assert.ok(view instanceof Uint8Array);
assert.deepEqual(Array.from(view), expected);
view.fill(254);
assert.deepEqual(Array.from(value.copyBytes()), expected);
assert.notEqual(value.copyBytes(), value.copyBytes());
cases.owned = { id, width, inputCopy: true, outputCopy: true };

const fakeWidth = new Uint8Array(width + 1);
Object.defineProperty(fakeWidth, "length", { value: width });
assert.throws(() => fromBytes(id, fakeWidth), wrong);
cases.shadowedWidth = failure;

const otherRealm = runInNewContext("Uint8Array.from(data)", { data: expected });
assert.deepEqual(Array.from(fromBytes(id, otherRealm).copyBytes()), expected);
cases.crossRealm = true;

const detached = new Uint8Array(Math.max(1, width));
structuredClone(detached.buffer, { transfer: [detached.buffer] });
if (width === 0) {
    assert.deepEqual(Array.from(fromBytes(id, detached).copyBytes()), []);
    cases.detached = "empty";
} else {
    assert.throws(() => fromBytes(id, detached), wrong);
    cases.detached = failure;
}
console.log(JSON.stringify(cases));
