
import { Sky, Side, Tier } from "./values";
const input = (value: unknown) => Sky.fromOrdinal(value);
for (const invalid of [-1, 3, 1.5, "0", null, undefined, {}, NaN, Infinity]) {
  if (input(invalid) !== undefined) throw new Error("invalid ordinal accepted");
}
if (input(0) === undefined || Sky.ordinal(input(0)!) !== 0 || 1 / Sky.ordinal(input(-0)!) !== Infinity)
  throw new Error("closed ordinal conversion changed");
if (!Object.isFrozen(Sky) || "equals" in Sky || "partialEquals" in Sky || "equals" in Side || "partialEquals" in Tier)
  throw new Error("comparison surface differs");
if (Sky.equality !== "none" || Side.equality !== "partial" || Tier.equality !== "total")
  throw new Error("published equality capability differs");
if (Sky.Clear !== Sky.Clear || Sky.Clear === Sky.Mist)
  throw new Error("JavaScript strict comparison on the numeric choice differs");
console.log(JSON.stringify({
  Sky: [Sky.ordinal(Sky.Clear), Sky.ordinal(Sky.Mist), Sky.ordinal(Sky.Cloud)],
  Side: [Side.ordinal(Side.Left), Side.ordinal(Side.Right), Side.partialEquals(Side.Left, Side.Left),
    Side.partialEquals(Side.Left, Side.Right)],
  Tier: [Tier.ordinal(Tier.Local), Tier.ordinal(Tier.Wide), Tier.equals(Tier.Wide, Tier.Wide),
    Tier.equals(Tier.Local, Tier.Wide)],
}));
