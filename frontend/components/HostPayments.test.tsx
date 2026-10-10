import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { HostParty, SlotsClient } from "../lib/slots";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";
import { HostPayments } from "./HostPayments";

const t = messages.host;
const FUTURE = "2099-10-10T11:30:00Z";

function party(overrides: Partial<HostParty> = {}): HostParty {
  return {
    paymentCode: 7,
    team: "a",
    holderName: "Long",
    guests: ["An"],
    amountVnd: 100000,
    paymentStatus: "payment_reported",
    holdExpiresAt: null,
    ...overrides,
  };
}

function client(overrides: Partial<SlotsClient>): SlotsClient {
  return {
    roster: vi.fn(),
    mine: vi.fn(),
    join: vi.fn(),
    leave: vi.fn(),
    reportPayment: vi.fn(),
    hostParties: vi.fn().mockResolvedValue({ status: "host", parties: [party()] }),
    hostAction: vi.fn().mockResolvedValue({ ok: true }),
    cancelMatch: vi.fn().mockResolvedValue({ ok: true }),
    ...overrides,
  };
}

describe("HostPayments", () => {
  it("shows nothing to players who are not the host", async () => {
    const api = client({ hostParties: vi.fn().mockResolvedValue({ status: "notHost" }) });
    const { container } = render(<HostPayments shareId="k7Qm2xPa" startsAt={FUTURE} cancelled={false} client={api} onChange={vi.fn()} />);

    await waitFor(() => expect(api.hostParties).toHaveBeenCalled());
    expect(container.textContent).toBe("");
  });

  it("lists each party with its amount, status and transfer code", async () => {
    render(<HostPayments shareId="k7Qm2xPa" startsAt={FUTURE} cancelled={false} client={client({})} onChange={vi.fn()} />);

    expect(await screen.findByText("Long")).toBeTruthy();
    expect(screen.getByText(fill(t.withGuests, { names: "An" }))).toBeTruthy();
    expect(screen.getByText(fill(t.code, { code: 7 }))).toBeTruthy();
    expect(screen.getByText(t.status.payment_reported)).toBeTruthy();
  });

  it("confirms a transfer, then refreshes the list and the roster", async () => {
    const hostParties = vi
      .fn()
      .mockResolvedValueOnce({ status: "host", parties: [party()] })
      .mockResolvedValue({ status: "host", parties: [party({ paymentStatus: "confirmed" })] });
    const api = client({ hostParties });
    const onChange = vi.fn();
    render(<HostPayments shareId="k7Qm2xPa" startsAt={FUTURE} cancelled={false} client={api} onChange={onChange} />);

    fireEvent.click(await screen.findByRole("button", { name: t.confirm }));

    expect(await screen.findByText(t.status.confirmed)).toBeTruthy();
    expect(api.hostAction).toHaveBeenCalledWith("k7Qm2xPa", 7, "confirm");
    expect(onChange).toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: t.reject })).toBeNull();
  });

  it("asks before releasing a party whose transfer is missing", async () => {
    const api = client({});
    render(<HostPayments shareId="k7Qm2xPa" startsAt={FUTURE} cancelled={false} client={api} onChange={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: t.reject }));
    expect(screen.getByText(fill(t.rejectConfirm, { count: 2, name: "Long" }))).toBeTruthy();
    expect(api.hostAction).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: t.rejectNo }));
    expect(screen.queryByRole("button", { name: t.rejectYes })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: t.reject }));
    fireEvent.click(screen.getByRole("button", { name: t.rejectYes }));
    await waitFor(() => expect(api.hostAction).toHaveBeenCalledWith("k7Qm2xPa", 7, "reject"));
  });

  it("explains a refused action", async () => {
    const api = client({ hostAction: vi.fn().mockResolvedValue({ ok: false, error: "already_confirmed" }) });
    render(<HostPayments shareId="k7Qm2xPa" startsAt={FUTURE} cancelled={false} client={api} onChange={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: t.reject }));
    fireEvent.click(screen.getByRole("button", { name: t.rejectYes }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.already_confirmed);
  });

  it("offers rejection only after the player reported a transfer", async () => {
    const api = client({
      hostParties: vi.fn().mockResolvedValue({
        status: "host",
        parties: [party({ paymentStatus: "awaiting_payment", holdExpiresAt: "2099-10-10T10:00:00Z" })],
      }),
    });
    render(<HostPayments shareId="k7Qm2xPa" startsAt={FUTURE} cancelled={false} client={api} onChange={vi.fn()} />);

    expect(await screen.findByRole("button", { name: t.confirm })).toBeTruthy();
    expect(screen.queryByRole("button", { name: t.reject })).toBeNull();
  });

  it("offers no rejection once the match has started", async () => {
    render(<HostPayments shareId="k7Qm2xPa" startsAt="2000-01-01T00:00:00Z" cancelled={false} client={client({})} onChange={vi.fn()} />);

    expect(await screen.findByRole("button", { name: t.confirm })).toBeTruthy();
    expect(screen.queryByRole("button", { name: t.reject })).toBeNull();
  });

  it("cancels the match after asking", async () => {
    const api = client({});
    const onChange = vi.fn();
    render(<HostPayments shareId="k7Qm2xPa" startsAt={FUTURE} cancelled={false} client={api} onChange={onChange} />);

    fireEvent.click(await screen.findByRole("button", { name: t.cancel }));
    expect(screen.getByText(t.cancelConfirm)).toBeTruthy();
    expect(api.cancelMatch).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: t.cancelYes }));

    await waitFor(() => expect(api.cancelMatch).toHaveBeenCalledWith("k7Qm2xPa"));
    expect(onChange).toHaveBeenCalled();
  });

  it("keeps the list but offers no actions once the match is cancelled", async () => {
    render(<HostPayments shareId="k7Qm2xPa" startsAt={FUTURE} cancelled client={client({})} onChange={vi.fn()} />);

    expect(await screen.findByText(t.cancelledNote)).toBeTruthy();
    expect(screen.getByText("Long")).toBeTruthy();
    for (const name of [t.confirm, t.reject, t.cancel]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
  });
});
