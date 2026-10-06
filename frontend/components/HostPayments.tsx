"use client";

import { useCallback, useEffect, useState } from "react";
import { formatClock, formatVnd } from "../lib/format";
import type { HostActionError, HostParty, SlotsClient } from "../lib/slots";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";

const t = messages.host;

type Props = {
  shareId: string;
  client: SlotsClient;
  /** Called after a change so the roster can refresh. */
  onChange: () => void;
};

function statusText(party: HostParty): string {
  if (party.paymentStatus === "awaiting_payment" && party.holdExpiresAt) {
    return fill(t.status.awaiting_payment, { time: formatClock(party.holdExpiresAt) });
  }
  return t.status[party.paymentStatus];
}

/** The host's list of parties to check transfers against. Hidden for everyone else. */
export function HostPayments({ shareId, client, onChange }: Props) {
  const [parties, setParties] = useState<HostParty[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The party whose release the host is being asked to confirm. */
  const [confirmingReject, setConfirmingReject] = useState<number | null>(null);

  const load = useCallback(async () => {
    try {
      const view = await client.hostParties(shareId);
      setParties(view.status === "host" ? view.parties : null);
    } catch {
      setError(t.errors.unexpected);
    }
  }, [client, shareId]);

  useEffect(() => {
    void load();
  }, [load]);

  async function act(party: HostParty, action: "confirm" | "reject") {
    setConfirmingReject(null);
    setBusy(true);
    setError(null);
    try {
      const result = await client.hostAction(shareId, party.paymentCode, action);
      if (!result.ok) {
        setError(t.errors[result.error satisfies HostActionError]);
      }
      await load();
      onChange();
    } catch {
      setError(t.errors.unexpected);
    } finally {
      setBusy(false);
    }
  }

  if (!parties) {
    return null;
  }
  return (
    <section className="host-payments" aria-labelledby="host-payments-title">
      <h2 id="host-payments-title" className="team-slots__title">
        {t.title}
      </h2>
      {error ? (
        <p className="form-error" role="alert">
          {error}
        </p>
      ) : null}
      {parties.length === 0 ? <p className="muted">{t.empty}</p> : null}
      <ul className="host-payments__list">
        {parties.map((party) => (
          <li key={party.paymentCode} className="host-party">
            <div className="host-party__who">
              <strong>{party.holderName ?? messages.auth.anonymousName}</strong>
              {party.guests.length > 0 ? (
                <span className="muted">{fill(t.withGuests, { names: party.guests.join(", ") })}</span>
              ) : null}
              <span className="muted">
                {messages.slots.team[party.team]} · <span>{fill(t.code, { code: party.paymentCode })}</span>
              </span>
            </div>
            <div className="host-party__money">
              <strong>{formatVnd(party.amountVnd)}</strong>
              <span className={party.paymentStatus === "confirmed" ? "pay-status--done" : "muted"}>
                {statusText(party)}
              </span>
            </div>
            {party.paymentStatus !== "confirmed" && confirmingReject === party.paymentCode ? (
              <div className="host-party__actions">
                <span>
                  {fill(t.rejectConfirm, {
                    count: 1 + party.guests.length,
                    name: party.holderName ?? messages.auth.anonymousName,
                  })}
                </span>
                <button type="button" className="button-small" onClick={() => act(party, "reject")} disabled={busy}>
                  {t.rejectYes}
                </button>
                <button type="button" className="link-button" onClick={() => setConfirmingReject(null)}>
                  {t.rejectNo}
                </button>
              </div>
            ) : party.paymentStatus !== "confirmed" ? (
              <div className="host-party__actions">
                <button type="button" className="button-small" onClick={() => act(party, "confirm")} disabled={busy}>
                  {t.confirm}
                </button>
                <button
                  type="button"
                  className="link-button"
                  onClick={() => setConfirmingReject(party.paymentCode)}
                  disabled={busy}
                >
                  {t.reject}
                </button>
              </div>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}
