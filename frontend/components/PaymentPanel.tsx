"use client";

import QRCode from "qrcode";
import { useEffect, useState } from "react";
import { formatClock, formatVnd } from "../lib/format";
import type { JoinedPlace } from "../lib/slots";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";

const t = messages.payment;

type Props = {
  place: JoinedPlace;
  onReport: () => Promise<void> | void;
  busy?: boolean;
};

/** Renders the VietQR payload in the browser, so bank details never go to a QR service. */
function useQrDataUrl(payload: string | null): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!payload) {
      setUrl(null);
      return;
    }
    let active = true;
    QRCode.toString(payload, { type: "svg", errorCorrectionLevel: "M", margin: 1 }).then(
      (svg) => active && setUrl(`data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`),
      () => active && setUrl(null),
    );
    return () => {
      active = false;
    };
  }, [payload]);
  return url;
}

function CopyRow({ label, value }: { label: string; value: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
    } catch {
      // Clipboard can be blocked; the value stays visible to copy by hand.
    }
  }
  return (
    <div className="pay-row">
      <dt>{label}</dt>
      <dd>
        <span>{value}</span>
        <button type="button" className="link-button" onClick={copy}>
          {copied ? t.copied : t.copy}
        </button>
      </dd>
    </div>
  );
}

/** What a player who holds a place still has to do to pay the host. */
export function PaymentPanel({ place, onReport, busy = false }: Props) {
  const qr = useQrDataUrl(place.paymentStatus === "confirmed" ? null : (place.payment?.qrPayload ?? null));

  if (place.paymentStatus === "confirmed") {
    return place.amountVnd > 0 ? <p className="pay-status pay-status--done">{t.confirmed}</p> : null;
  }
  const payment = place.payment;
  if (!payment) {
    return <p className="muted">{t.unavailable}</p>;
  }
  return (
    <section className="payment" aria-labelledby="payment-title">
      <h3 id="payment-title">{t.title}</h3>
      <p className="payment__amount">
        <span>{t.amount}</span>
        <strong data-testid="payment-amount">{formatVnd(payment.amountVnd)}</strong>
      </p>
      {place.paymentStatus === "awaiting_payment" && place.holdExpiresAt ? (
        <p className="pay-status">{fill(t.holdUntil, { time: formatClock(place.holdExpiresAt) })}</p>
      ) : null}
      {qr ? <img className="payment__qr" src={qr} alt={t.qrAlt} width={220} height={220} /> : null}
      <p className="muted payment__hint">{t.scanHint}</p>
      <dl className="payment__details">
        <div className="pay-row">
          <dt>{t.bank}</dt>
          <dd>
            <span>{payment.bankName}</span>
          </dd>
        </div>
        <CopyRow label={t.accountNumber} value={payment.accountNumber} />
        <div className="pay-row">
          <dt>{t.accountName}</dt>
          <dd>
            <span>{payment.accountName}</span>
          </dd>
        </div>
        <CopyRow label={t.memo} value={payment.memo} />
      </dl>
      {place.paymentStatus === "awaiting_payment" ? (
        <button type="button" className="button-primary" onClick={() => onReport()} disabled={busy}>
          {busy ? t.reporting : t.reportPaid}
        </button>
      ) : (
        <p className="pay-status">{t.reported}</p>
      )}
    </section>
  );
}
