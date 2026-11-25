// jco supplies ledger.js and its TypeScript declarations from this exact component.
import { instantiate, type Account, type Root } from "./ledger.js";

interface WireAccount { id: string; owner: string; balance: string }
interface Call { function: string; arguments: unknown[] }
interface HostCall extends Call { returns: WireAccount[] }
interface Scenario { caseId: string; imports: HostCall[]; calls: Call[] }
interface Instrumented {
    fixtureLoad(accounts: Account[]): void;
    completeSnapshot(): Account[];
}

function decode(account: WireAccount): Account {
    return { ...account, balance: BigInt(account.balance) };
}
function encode(account: Account): WireAccount {
    return { ...account, balance: account.balance.toString() };
}

function observe(root: Root, call: Call): unknown {
    const args = call.arguments;
    switch (call.function) {
        case "lookup": {
            const found = root.lookup(String(args[0]));
            return found === undefined ? { none: true } : { some: encode(found) };
        }
        case "transfer":
            try {
                root.transfer(String(args[0]), String(args[1]), BigInt(String(args[2])));
                return { ok: null };
            } catch (error) {
                if (root._util.isComponentError(error)) return { err: error.payload };
                throw error;
            }
        case "fixture-load":
            (root as Root & Instrumented).fixtureLoad((args[0] as WireAccount[]).map(decode));
            return null;
        case "complete-snapshot":
            return (root as Root & Instrumented).completeSnapshot().map(encode);
        default:
            throw new Error("Unknown ledger operation " + call.function);
    }
}

export async function replay(cases: Scenario[], getCoreModule: (name: string) => Promise<WebAssembly.Module>) {
    const observed = [];
    for (const scenario of cases) {
        const imports: HostCall[] = [];
        const root = await instantiate(getCoreModule, {
            "oddlang:ledger/ledger-types@1.0.0": {},
            "initial-state": { default() {
                const next = scenario.imports[imports.length];
                if (next?.function !== "initial-state") throw new Error("unexpected initial-state call");
                const values = next.returns.map(decode);
                imports.push({ function: "initial-state", arguments: [], returns: values.map(encode) });
                return values;
            } },
        });
        try {
            const calls = scenario.calls.map(call => {
                const input = { function: call.function, arguments: call.arguments };
                try { return { ...input, result: observe(root, call) }; }
                catch (error) { return { ...input, trap: String(error) }; }
            });
            observed.push({ caseId: scenario.caseId, imports, calls });
        } finally {
            root._util.stop();
        }
    }
    return observed;
}
