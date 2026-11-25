// Generated from the pinned HTTP contract.
export interface Account {
  "balance": bigint;
  "id": string;
  "owner": string;
}
export interface Lookup {
  "id": string;
}
export interface Pair {
  "destination": Account;
  "source": Account;
}
export interface Transfer {
  "amount": bigint;
  "destination": string;
  "source": string;
}
const records: Record<string,Field[]> = {"Account":[{"kind":"I64","max":"9223372036854775807","min":"0","name":"balance","wire":"balance"},{"kind":"Text","max":"9223372036854775807","min":"-9223372036854775808","name":"id","wire":"id"},{"kind":"Text","max":"9223372036854775807","min":"-9223372036854775808","name":"owner","wire":"owner"}],"Lookup":[{"kind":"Text","max":"9223372036854775807","min":"-9223372036854775808","name":"id","wire":"id"}],"Pair":[{"kind":"Account","max":"9223372036854775807","min":"-9223372036854775808","name":"destination","wire":"destination"},{"kind":"Account","max":"9223372036854775807","min":"-9223372036854775808","name":"source","wire":"source"}],"Transfer":[{"kind":"I64","max":"9223372036854775807","min":"-9223372036854775808","name":"amount","wire":"amount"},{"kind":"Text","max":"9223372036854775807","min":"-9223372036854775808","name":"destination","wire":"destination"},{"kind":"Text","max":"9223372036854775807","min":"-9223372036854775808","name":"source","wire":"source"}]};
const operations: Record<string,Operation> = {"GET":{"errors":{"urn:oddlang:http:error:AccountMissing":404,"urn:oddlang:http:error:InvalidRequest":400},"method":"GET","path":"/accounts/{id}","request":"Lookup","response":"Account","status":200},"POST":{"errors":{"urn:oddlang:http:error:InvalidRequest":400,"urn:oddlang:http:error:UnsupportedMediaType":415,"urn:oddlang:ledger:error:DestinationMissing":404,"urn:oddlang:ledger:error:DestinationOverflow":409,"urn:oddlang:ledger:error:InsufficientFunds":409,"urn:oddlang:ledger:error:NonPositiveAmount":422,"urn:oddlang:ledger:error:SameAccount":422,"urn:oddlang:ledger:error:SourceMissing":404},"method":"POST","path":"/transfers","request":"Transfer","response":"Pair","status":200}};
interface Field {
  name: string;
  wire: string;
  kind: string;
  min: string;
  max: string;
}
interface Operation {
  method: string;
  path: string;
  status: number;
  request: string;
  response: string;
  errors: Record<string, number>;
}
export type Result<T> =
  | { kind: "success"; value: T }
  | { kind: "namedError"; error: string }
  | { kind: "protocolError" }
  | { kind: "localRejection" };
function invalid(): never {
  throw new Error("invalid ledger message");
}
function scalarString(value: unknown): string {
  if (typeof value !== "string" || /[\uD800-\uDFFF]/u.test(value)) invalid();
  return value;
}
// This profile accepts only objects, strings and booleans. Decode each name before
// duplicate detection and retain null prototypes through validation.
function parse(bytes: ArrayBuffer): unknown {
  const text = new TextDecoder("utf-8", {
    fatal: true,
    ignoreBOM: true,
  }).decode(bytes);
  let pos = 0;
  function whitespace() {
    while (pos < text.length && /[ \r\n\t]/.test(text[pos])) pos++;
  }
  function string(): string {
    const start = pos;
    if (text[pos++] !== '"') invalid();
    while (pos < text.length) {
      const c = text[pos++];
      if (c === '"') return scalarString(JSON.parse(text.slice(start, pos)));
      if (c === "\\") pos++;
    }
    return invalid();
  }
  function value(): unknown {
    whitespace();
    if (text[pos] === '"') return string();
    for (const token of ["true", "false"])
      if (text.startsWith(token, pos)) {
        pos += token.length;
        return token === "true";
      }
    if (text[pos++] !== "{") invalid();
    const obj = Object.create(null);
    whitespace();
    if (text[pos] === "}") {
      pos++;
      return obj;
    }
    while (true) {
      whitespace();
      const key = string();
      if (Object.hasOwn(obj, key)) invalid();
      whitespace();
      if (text[pos++] !== ":") invalid();
      obj[key] = value();
      whitespace();
      if (text[pos] === "}") {
        pos++;
        return obj;
      }
      if (text[pos++] !== ",") invalid();
    }
  }
  const result = value();
  whitespace();
  if (pos !== text.length) invalid();
  return result;
}
function record(
  shape: string,
  value: unknown,
  encode: boolean,
): Record<string, unknown> {
  if (!value || typeof value !== "object") invalid();
  const fields = records[shape];
  if (Object.keys(value).length !== fields.length) invalid();
  const result = Object.create(null);
  for (const f of fields) {
    const key = encode ? f.name : f.wire;
    if (!Object.hasOwn(value, key)) invalid();
    let v = (value as Record<string, unknown>)[key];
    if (f.kind === "I64") {
      if (
        encode
          ? typeof v !== "bigint"
          : typeof v !== "string" ||
            !/^(?:0|-?[1-9][0-9]*)$/.test(v) ||
            v.trim() !== v
      )
        invalid();
      const n = BigInt(v as string | bigint);
      if (n < BigInt(f.min) || n > BigInt(f.max)) invalid();
      v = encode ? n.toString() : n;
    } else if (f.kind === "Text") scalarString(v);
    else if (f.kind === "Boolean") {
      if (typeof v !== "boolean") invalid();
    } else v = record(f.kind, v, encode);
    result[encode ? f.wire : f.name] = v;
  }
  return result;
}
export class LedgerClient {
  constructor(
    readonly baseUrl: string,
    readonly send: typeof fetch = globalThis.fetch.bind(globalThis),
  ) {}
  lookup(id: string): Promise<Result<Account>> {
    return this.call("GET", { id });
  }
  transfer(input: Transfer): Promise<Result<Pair>> {
    return this.call("POST", input);
  }
  private async call<T>(
    method: string,
    input: Lookup | Transfer,
  ): Promise<Result<T>> {
    const op = operations[method];
    let path = op.path;
    const init: RequestInit = { method, redirect: "manual" };
    try {
      const wire = record(op.request, input, true);
      if (method === "GET") {
        const id = scalarString(wire.id);
        if (!id || id === "." || id === "..") invalid();
        path = path.replace("{id}", () => encodeURIComponent(id));
      } else {
        init.headers = { "Content-Type": "application/json" };
        init.body = JSON.stringify(wire);
      }
    } catch {
      return { kind: "localRejection" };
    }
    try {
      const base = new URL(this.baseUrl);
      if (base.search || base.hash) invalid();
      base.pathname = base.pathname.replace(/\/+$/, "") + path;
      const send = this.send;
      const r = await send(base, init);
      if (
        !/^application\/json(?:\s*;\s*charset\s*=\s*(?:utf-8|"utf-8"))?$/i.test(
          r.headers.get("content-type")?.trim() ?? "",
        )
      )
        invalid();
      const obj = parse(await r.arrayBuffer());
      if (r.status === op.status)
        return { kind: "success", value: record(op.response, obj, false) as T };
      if (
        !obj ||
        typeof obj !== "object" ||
        Object.keys(obj).length !== 1 ||
        !Object.hasOwn(obj, "error")
      )
        invalid();
      const error = scalarString((obj as { error: unknown }).error);
      if (!Object.hasOwn(op.errors, error) || op.errors[error] !== r.status)
        invalid();
      return { kind: "namedError", error };
    } catch {
      return { kind: "protocolError" };
    }
  }
}
