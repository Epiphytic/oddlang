import React, { useEffect, useState, type FormEvent } from "react";
import { createRoot } from "react-dom/client";
import { LedgerClient, type Account } from "../generated/client";

const ledger = new LedgerClient(window.location.origin);

function TransferForm() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [message, setMessage] = useState("");
  const [sending, setSending] = useState(false);

  useEffect(() => {
    let mounted = true;
    Promise.all(["A", "B"].map(id => ledger.lookup(id))).then(results => {
      if (!mounted) return;
      const values = results.flatMap(result => result.kind === "success" ? [result.value] : []);
      setAccounts(values);
      if (values.length !== results.length) setMessage("One or more accounts could not be read.");
    });
    return () => { mounted = false; };
  }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const amount = String(new FormData(event.currentTarget).get("amount") ?? "");
    if (!/^[1-9][0-9]*$/.test(amount)) {
      setMessage("Enter a positive whole number.");
      return;
    }
    setSending(true);
    try {
      const result = await ledger.transfer({source: "A", destination: "B", amount: BigInt(amount)});
      if (result.kind === "success") {
        setAccounts([result.value.source, result.value.destination]);
        setMessage("Transfer complete.");
      } else if (result.kind === "namedError") {
        setMessage(result.error);
      } else {
        setMessage("The transfer response could not be read.");
      }
    } finally {
      setSending(false);
    }
  }

  return <main>
    <h1>Transfer funds</h1>
    <ul aria-label="Account balances">
      {accounts.map(account => <li key={account.id} data-account={account.id}>
        {account.owner} <strong>{account.balance.toString()}</strong>
      </li>)}
    </ul>
    <form onSubmit={submit}>
      <label>Amount from A to B <input name="amount" inputMode="numeric" required /></label>
      <button disabled={sending || accounts.length !== 2}>Transfer</button>
    </form>
    <p role="status">{message}</p>
  </main>;
}

createRoot(document.getElementById("root")!).render(<TransferForm />);
