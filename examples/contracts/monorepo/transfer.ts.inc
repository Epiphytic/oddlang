type ErrorIRI = typeof errors[number];
type Reply = {error: ErrorIRI} | {ok: true, balances: Record<string, bigint>};
type Transport = (request: {source: string, destination: string, amount: string}) => unknown;

export function transfer(send: Transport, source: string, destination: string, amount: bigint): Reply {
  if (amount < minAmount || amount > maxAmount) throw new Error("amount outside i64");
  const reply = object(send({source, destination, amount: amount.toString()}));
  if ("error" in reply) {
    const error = errors.find(iri => iri === reply.error);
    if (!error) throw new Error("unknown transfer error");
    return {error};
  }
  if (reply.ok !== true) throw new Error("missing transfer outcome");
  return {ok: true, balances: Object.fromEntries(
    Object.entries(object(reply.balances)).map(([id, value]) => [id, balance(value)])
  )};
}
