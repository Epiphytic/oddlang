export function object(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw new Error("expected JSON object");
  return value as Record<string, unknown>;
}
export function text(value: unknown): string {
  if (typeof value !== "string") throw new Error("expected text");
  return value;
}
export function decimal(value: unknown, pattern: RegExp, min: bigint, max: bigint): bigint {
  const s = text(value);
  if (!pattern.test(s)) throw new Error("invalid decimal encoding");
  const n = BigInt(s);
  if (n.toString() !== s || n < min || n > max) throw new Error("decimal outside range");
  return n;
}
