import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";

const { IncrementalDecoder, WireError } = await import(pathToFileURL(process.argv[2]).href);
const bytes = (...values) => new Uint8Array(values);
const empty = bytes();
const ping = bytes(62, 0, 0, 0, 3, 1, 2, 3);
const expected = { tag: 62, name: "Ping", values: [bytes(1, 2, 3)] };
const error = (action, code) => assert.throws(
  action,
  (err) => err instanceof WireError && err.code === code,
);

for (let split = 0; split < ping.length; split++) {
  const decoder = new IncrementalDecoder();
  assert.deepEqual(decoder.feed(ping.subarray(0, split)), { consumed: split, record: null });
  assert.deepEqual(decoder.feed(empty), { consumed: 0, record: null });
  if (split > 0) error(() => decoder.finish(), "Incomplete");
  const shared = Buffer.concat([ping.subarray(split), ping]);
  const { consumed, record } = decoder.feed(shared);
  assert.equal(consumed, ping.length - split);
  assert.deepEqual(record, expected);
  assert.deepEqual(decoder.feed(shared.subarray(consumed)), { consumed: ping.length, record: expected });
  decoder.finish();
}

const decoder = new IncrementalDecoder();
assert.deepEqual(decoder.feed(bytes(255, 0, 0, 0)), { consumed: 4, record: null });
error(() => decoder.feed(bytes(26, ...ping)), "Limit");
error(() => decoder.feed(ping), "Failed");
error(() => decoder.finish(), "Incomplete");

const malformed = new IncrementalDecoder();
error(() => malformed.feed(bytes(62, 0, 0, 0, 2, 1, 2)), "Truncated");
error(() => malformed.feed(empty), "Failed");
