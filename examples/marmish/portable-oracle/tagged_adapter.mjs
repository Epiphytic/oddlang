import { createInterface } from "node:readline";
import { encode, decode, IncrementalDecoder, variantName, WireError } from "./wire.mts";

const bytes = (value) => Buffer.from(value, "hex");
const hex = (value) => Buffer.from(value).toString("hex");

function record({ tag, name, values }) {
  if (name !== "Data" || values.length !== 2 || typeof values[0] !== "bigint"
      || !(values[1] instanceof Uint8Array)) throw new Error("unexpected generated Data fields");
  return `${tag}:${values[0]}:${hex(values[1])}`;
}

const tags = Array.from({ length: 256 }, (_, tag) => tag).filter((tag) => variantName(tag) === "Data");
if (tags.length !== 1) throw new Error("expected one generated Data variant");
const [dataTag] = tags;
let decoder = new IncrementalDecoder();
const input = createInterface({ input: process.stdin });
for await (const line of input) {
  const [operation, first, second, third] = line.split(" ");
  try {
    switch (operation) {
      case "E":
        console.log(`E:${hex(encode(dataTag, [BigInt(first), bytes(second)]))}`);
        break;
      case "T":
        console.log(`E:${hex(encode(Number(first), [BigInt(second), bytes(third)]))}`);
        break;
      case "D":
        console.log(`D:${record(decode(bytes(first)))}`);
        break;
      case "B":
        decoder = new IncrementalDecoder();
        console.log("B");
        break;
      case "F": {
        const feed = decoder.feed(bytes(first));
        console.log(`F:${feed.consumed}:${feed.record === null ? "" : record(feed.record)}`);
        break;
      }
      case "X":
        decoder.finish();
        console.log("X");
        break;
      default:
        throw new Error("invalid adapter operation");
    }
  } catch (error) {
    if (!(error instanceof WireError)) throw error;
    console.log(`!${error.code}`);
  }
}
