import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { JoinedPlace } from "../lib/slots";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";
import { PaymentPanel } from "./PaymentPanel";

const t = messages.payment;

function place(overrides: Partial<JoinedPlace> = {}): JoinedPlace {
  return {
    status: "in",
    team: "a",
    guests: ["An"],
    paymentCode: 7,
    paymentStatus: "awaiting_payment",
    // 18:42 in Ho Chi Minh City.
    holdExpiresAt: "2099-10-10T11:42:00Z",
    amountVnd: 100000,
    payment: {
      bankName: "ACB",
      accountNumber: "257678859",
      accountName: "PHAM LONG",
      amountVnd: 100000,
      memo: "DG k7Qm2xPa 7",
      qrPayload: "00020101021238530010A000000727",
    },
    ...overrides,
  };
}

describe("PaymentPanel", () => {
  it("shows the QR, transfer details and the hold deadline", async () => {
    render(<PaymentPanel place={place()} onReport={vi.fn()} />);

    expect(await screen.findByRole("img", { name: t.qrAlt })).toBeTruthy();
    expect(screen.getByText(fill(t.holdUntil, { time: "18:42" }))).toBeTruthy();
    for (const text of ["ACB", "257678859", "PHAM LONG", "DG k7Qm2xPa 7"]) {
      expect(screen.getByText(text)).toBeTruthy();
    }
    expect(screen.getByTestId("payment-amount").textContent).toMatch(/100\.000/);
  });

  it("lets the player report the transfer", async () => {
    const onReport = vi.fn().mockResolvedValue(undefined);
    render(<PaymentPanel place={place()} onReport={onReport} />);

    fireEvent.click(screen.getByRole("button", { name: t.reportPaid }));

    expect(onReport).toHaveBeenCalledTimes(1);
  });

  it("waits for the host after a reported transfer", () => {
    render(<PaymentPanel place={place({ paymentStatus: "payment_reported", holdExpiresAt: null })} onReport={vi.fn()} />);

    expect(screen.getByText(t.reported)).toBeTruthy();
    expect(screen.queryByRole("button", { name: t.reportPaid })).toBeNull();
  });

  it("says when the host confirmed, and shows nothing for a free place", () => {
    const { container, rerender } = render(
      <PaymentPanel place={place({ paymentStatus: "confirmed", holdExpiresAt: null, payment: null })} onReport={vi.fn()} />,
    );
    expect(screen.getByText(t.confirmed)).toBeTruthy();

    rerender(
      <PaymentPanel
        place={place({ paymentStatus: "confirmed", holdExpiresAt: null, payment: null, amountVnd: 0 })}
        onReport={vi.fn()}
      />,
    );
    expect(container.textContent).toBe("");
  });

  it("stops asking for money once the hold has expired", async () => {
    const onExpired = vi.fn();
    render(
      <PaymentPanel place={place({ holdExpiresAt: "2000-01-01T00:00:00Z" })} onReport={vi.fn()} onExpired={onExpired} />,
    );

    expect(await screen.findByText(t.expired)).toBeTruthy();
    expect(screen.queryByRole("img", { name: t.qrAlt })).toBeNull();
    expect(screen.queryByRole("button", { name: t.reportPaid })).toBeNull();
    expect(onExpired).toHaveBeenCalledTimes(1);
  });

  it("names what each copy button copies", () => {
    render(<PaymentPanel place={place()} onReport={vi.fn()} />);

    expect(screen.getByRole("button", { name: fill(t.copyLabel, { label: t.accountNumber }) })).toBeTruthy();
    expect(screen.getByRole("button", { name: fill(t.copyLabel, { label: t.memo }) })).toBeTruthy();
  });

  it("explains when the host has no payout account", () => {
    render(<PaymentPanel place={place({ payment: null })} onReport={vi.fn()} />);

    expect(screen.getByText(t.unavailable)).toBeTruthy();
  });
});
