"use client";

import { type FormEvent, useState } from "react";
import { type Bank, type PayoutAccount, type PayoutError, savePayout, toAccountName } from "../lib/payout";
import messages from "../messages/vi.json";

const t = messages.payout;

type Props = {
  banks: Bank[];
  initial: PayoutAccount | null;
  /** Where the host came from; `/create` offers a way back after saving. */
  next: string | null;
  save?: (account: PayoutAccount) => Promise<{ ok: true } | { ok: false; error: PayoutError }>;
};

export function PayoutForm({ banks, initial, next, save = savePayout }: Props) {
  const [bankBin, setBankBin] = useState(initial?.bankBin ?? "");
  const [accountNumber, setAccountNumber] = useState(initial?.accountNumber ?? "");
  const [accountName, setAccountName] = useState(initial?.accountName ?? "");
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<PayoutError | null>(null);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setSaved(false);
    setError(null);
    try {
      const result = await save({
        bankBin,
        accountNumber: accountNumber.trim(),
        accountName: toAccountName(accountName),
      });
      if (result.ok) {
        setSaved(true);
      } else {
        setError(result.error);
      }
    } catch {
      setError("unexpected");
    } finally {
      setBusy(false);
    }
  }

  const invalid = (field: PayoutError) =>
    error === field ? { "aria-invalid": true, "aria-describedby": "payout-error" } : {};

  return (
    <form className="create-form" onSubmit={submit} noValidate>
      <label className="field">
        <span>{t.bank}</span>
        <select value={bankBin} onChange={(e) => setBankBin(e.target.value)} {...invalid("bankBin")}>
          <option value="" disabled>
            {t.chooseBank}
          </option>
          {banks.map((bank) => (
            <option key={bank.bin} value={bank.bin}>
              {bank.name}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span>{t.accountNumber}</span>
        <input
          type="text"
          inputMode="numeric"
          autoComplete="off"
          maxLength={19}
          value={accountNumber}
          onChange={(e) => setAccountNumber(e.target.value)}
          {...invalid("accountNumber")}
        />
      </label>
      <label className="field">
        <span>{t.accountName}</span>
        <input
          type="text"
          autoComplete="off"
          maxLength={50}
          value={accountName}
          onChange={(e) => setAccountName(toAccountName(e.target.value, { typing: true }))}
          {...invalid("accountName")}
        />
      </label>
      <p className="muted field-hint">{t.accountNameHint}</p>
      {error ? (
        <p className="form-error" role="alert" id="payout-error">
          {t.errors[error]}
        </p>
      ) : null}
      {saved ? (
        <p className="pay-status pay-status--done" role="status">
          {t.saved}
        </p>
      ) : null}
      <button type="submit" className="button-primary" disabled={busy}>
        {busy ? t.saving : t.save}
      </button>
      {saved && next === "/create" ? (
        <a href="/create" className="button-secondary">
          {t.backToCreate}
        </a>
      ) : null}
    </form>
  );
}
