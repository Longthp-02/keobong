"use client";

import { useCallback, useEffect, useState } from "react";
import { formatClock, formatVnd } from "../lib/format";
import type { HostActionError, HostParty, SlotsClient } from "../lib/slots";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";

const t = messages.host;

type Props = {
  shareId: string;
  /** Rejecting a transfer and cancelling close at kickoff. */
  startsAt: string;
  /** A cancelled match keeps the list (for refunds) without actions. */
  cancelled: boolean;
  client: SlotsClient;
  /** Called after a change so the roster can refresh. */
  onChange: () => void;
};

function statusText(party: HostParty, cancelled: boolean): string {
  if (party.paymentStatus === "awaiting_payment" && cancelled) {
    // Holds are frozen at cancellation; the old deadline no longer means anything.
    return t.status.awaiting_payment_frozen;
  }
  if (party.paymentStatus === "awaiting_payment" && party.holdExpiresAt) {
    return fill(t.status.awaiting_payment, { time: formatClock(party.holdExpiresAt) });
  }
  return t.status[party.paymentStatus];
}

/** The host's list of parties to check transfers against. Hidden for everyone else. */
export function HostPayments({ shareId, startsAt, cancelled, client, onChange }: Props) {
  const [parties, setParties] = useState<HostParty[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The party whose release the host is being asked to confirm. */
  const [confirmingReject, setConfirmingReject] = useState<number | null>(null);
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  // Decided after hydration so server and client render the same markup.
  const [started, setStarted] = useState(false);
  useEffect(() => setStarted(Date.parse(startsAt) <= Date.now()), [startsAt]);

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

  async function cancelMatch() {
    setConfirmingCancel(false);
    setBusy(true);
    setError(null);
    try {
      const result = await client.cancelMatch(shareId);
      if (!result.ok) {
        setError(t.errors[result.error]);
      }
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
  const canAct = !cancelled;
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
      {cancelled ? <p className="notice">{t.cancelledNote}</p> : null}
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
                {statusText(party, cancelled)}
              </span>
            </div>
            {!canAct ? null : party.paymentStatus !== "confirmed" && confirmingReject === party.paymentCode ? (
              <div className="host-party__actions">
                <span aria-live="polite">
                  {fill(t.rejectConfirm, {
                    count: 1 + party.guests.length,
                    name: party.holderName ?? messages.auth.anonymousName,
                  })}
                </span>
                <button type="button" className="button-small" onClick={() => act(party, "reject")} disabled={busy}>
                  {t.rejectYes}
                </button>
                <button type="button" className="link-button" onClick={() => setConfirmingReject(null)} autoFocus>
                  {t.rejectNo}
                </button>
              </div>
            ) : party.paymentStatus !== "confirmed" ? (
              <div className="host-party__actions">
                <button type="button" className="button-small" onClick={() => act(party, "confirm")} disabled={busy}>
                  {t.confirm}
                </button>
                {/* Only a reported transfer can be rejected, and only before kickoff (Long, 2026-10-09). */}
                {party.paymentStatus === "payment_reported" && !started ? (
                  <button
                    type="button"
                    className="link-button"
                    onClick={() => setConfirmingReject(party.paymentCode)}
                    disabled={busy}
                  >
                    {t.reject}
                  </button>
                ) : null}
              </div>
            ) : null}
          </li>
        ))}
      </ul>
      {canAct && !started ? (
        confirmingCancel ? (
          <div className="host-party__actions">
            <span aria-live="polite">{t.cancelConfirm}</span>
            <button type="button" className="button-danger" onClick={cancelMatch} disabled={busy}>
              {t.cancelYes}
            </button>
            {/* Focus lands on the safe choice when the question appears. */}
            <button type="button" className="link-button" onClick={() => setConfirmingCancel(false)} autoFocus>
              {t.cancelNo}
            </button>
          </div>
        ) : (
          <button type="button" className="button-secondary" onClick={() => setConfirmingCancel(true)} disabled={busy}>
            {t.cancel}
          </button>
        )
      ) : null}
    </section>
  );
}
