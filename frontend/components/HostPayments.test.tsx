import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { HostParty, SlotsClient } from "../lib/slots";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";
import { HostPayments } from "./HostPayments";

const t = messages.host;

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
    ...overrides,
  };
}

describe("HostPayments", () => {
  it("shows nothing to players who are not the host", async () => {
    const api = client({ hostParties: vi.fn().mockResolvedValue({ status: "notHost" }) });
    const { container } = render(<HostPayments shareId="k7Qm2xPa" client={api} onChange={vi.fn()} />);

    await waitFor(() => expect(api.hostParties).toHaveBeenCalled());
    expect(container.textContent).toBe("");
  });

  it("lists each party with its amount, status and transfer code", async () => {
    render(<HostPayments shareId="k7Qm2xPa" client={client({})} onChange={vi.fn()} />);

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
    render(<HostPayments shareId="k7Qm2xPa" client={api} onChange={onChange} />);

    fireEvent.click(await screen.findByRole("button", { name: t.confirm }));

    expect(await screen.findByText(t.status.confirmed)).toBeTruthy();
    expect(api.hostAction).toHaveBeenCalledWith("k7Qm2xPa", 7, "confirm");
    expect(onChange).toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: t.reject })).toBeNull();
  });

  it("asks before releasing a party whose transfer is missing", async () => {
    const api = client({});
    render(<HostPayments shareId="k7Qm2xPa" client={api} onChange={vi.fn()} />);

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
    render(<HostPayments shareId="k7Qm2xPa" client={api} onChange={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: t.reject }));
    fireEvent.click(screen.getByRole("button", { name: t.rejectYes }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.already_confirmed);
  });
});
