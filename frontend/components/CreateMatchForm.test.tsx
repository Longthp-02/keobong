import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import messages from "../messages/vi.json";
import type { Venue } from "../lib/api";
import { CreateMatchForm } from "./CreateMatchForm";

const t = messages.create;
const VENUES: Venue[] = [
  { id: "ssa-amitie", name: "SSA Sports Center (Amitie Thảo Điền)", address: "28 Duyên Hải, An Khánh" },
  { id: "an-phu-nguyen-hoang", name: "Sân bóng An Phú Quận 2", address: "93 Nguyễn Hoàng, Bình Trưng" },
];

function field(label: string) {
  return screen.getByLabelText(label) as HTMLInputElement;
}

function fillRequired() {
  fireEvent.change(field(t.venue), { target: { value: "ssa-amitie" } });
  fireEvent.change(field(t.date), { target: { value: "2099-10-10" } });
  fireEvent.change(field(t.startTime), { target: { value: "18:30" } });
  fireEvent.change(field(t.endTime), { target: { value: "20:00" } });
  fireEvent.change(field(t.totalFee), { target: { value: "900000" } });
}

describe("CreateMatchForm", () => {
  it("defaults the slot count by format and previews the price per player", () => {
    render(<CreateMatchForm venues={VENUES} onSubmit={vi.fn()} />);
    fillRequired();

    expect(field(t.slotCount).value).toBe("18");
    expect(screen.getByTestId("price-per-player").textContent).toMatch(/50\.000/);

    fireEvent.click(screen.getByRole("radio", { name: messages.match.format.five_a_side }));

    expect(field(t.slotCount).value).toBe("14");
    expect(screen.getByTestId("price-per-player").textContent).toMatch(/65\.000/);
  });

  it("keeps a slot count the host typed when the format changes", () => {
    render(<CreateMatchForm venues={VENUES} onSubmit={vi.fn()} />);
    fireEvent.change(field(t.slotCount), { target: { value: "20" } });

    fireEvent.click(screen.getByRole("radio", { name: messages.match.format.eleven_a_side }));

    expect(field(t.slotCount).value).toBe("20");
  });

  it("submits times in Ho Chi Minh City converted to UTC", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} />);
    fillRequired();

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit).toHaveBeenCalledWith({
      venueId: "ssa-amitie",
      // Read by an API from before venues existed; the current API ignores it.
      venueName: "SSA Sports Center (Amitie Thảo Điền)",
      startsAt: "2099-10-10T11:30:00.000Z",
      endsAt: "2099-10-10T13:00:00.000Z",
      format: "seven_a_side",
      matchType: "casual",
      levelMin: 2.5,
      levelMax: 3.5,
      totalFeeVnd: 900000,
      slotCount: 18,
    });
  });

  it("shows the message for the field the server rejected", async () => {
    const onSubmit = vi.fn().mockResolvedValue({ field: "startsAt" });
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} />);
    fillRequired();

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.startsAt);
  });

  it("asks to sign in again when the session has expired", async () => {
    const onSubmit = vi.fn().mockResolvedValue({ field: "unauthenticated" });
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} />);
    fillRequired();

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.unauthenticated);
  });

  it.each([
    ["date", t.errors.startsAt],
    ["startTime", t.errors.startsAt],
    ["endTime", t.errors.endsAt],
  ] as const)("asks for a missing %s instead of failing generically", async (missing, message) => {
    const onSubmit = vi.fn();
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} />);
    fillRequired();
    fireEvent.change(field(t[missing]), { target: { value: "" } });

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", message);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("does not treat an empty fee as a free match", async () => {
    const onSubmit = vi.fn();
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} />);
    fillRequired();
    fireEvent.change(field(t.totalFee), { target: { value: "" } });

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.totalFeeVnd);
    expect(field(t.totalFee).getAttribute("aria-invalid")).toBe("true");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("rejects a fractional slot count before submitting", async () => {
    const onSubmit = vi.fn();
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} />);
    fillRequired();
    fireEvent.change(field(t.slotCount), { target: { value: "18.5" } });

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.slotCount);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it.each([
    ["before the start (no matches past midnight)", "22:30", "00:30"],
    ["equal to the start", "18:30", "18:30"],
    ["more than 4 hours after the start", "18:30", "22:31"],
  ])("rejects an end time %s", async (_case, start, end) => {
    const onSubmit = vi.fn();
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} />);
    fillRequired();
    fireEvent.change(field(t.startTime), { target: { value: start } });
    fireEvent.change(field(t.endTime), { target: { value: end } });

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.endsAt);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("accepts a match of exactly 4 hours", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} />);
    fillRequired();
    fireEvent.change(field(t.endTime), { target: { value: "22:30" } });

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
  });

  it("stays disabled after a successful submit so a second tap cannot duplicate the match", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} />);
    fillRequired();

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("button", { name: t.submitting }).hasAttribute("disabled")).toBe(true);
  });

  it("announces the price preview politely to screen readers", () => {
    render(<CreateMatchForm venues={VENUES} onSubmit={vi.fn()} />);

    expect(screen.getByTestId("price-per-player").closest("[aria-live]")?.getAttribute("aria-live")).toBe(
      "polite",
    );
  });

  it("asks for a payout account before posting a paid match", async () => {
    const onSubmit = vi.fn();
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} hasPayout={false} />);
    fillRequired();

    const link = screen.getByRole("link", { name: t.payoutLink });
    expect(link.getAttribute("href")).toBe("/account/payout?next=%2Fcreate");
    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.payout);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("posts a free match without a payout account", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} hasPayout={false} />);
    fillRequired();
    fireEvent.change(field(t.totalFee), { target: { value: "0" } });

    expect(screen.queryByRole("link", { name: t.payoutLink })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
  });

  it("offers only the listed venues and says where to ask for another", () => {
    render(<CreateMatchForm venues={VENUES} onSubmit={vi.fn()} />);

    const options = Array.from((field(t.venue) as unknown as HTMLSelectElement).options).map((o) => o.textContent);
    expect(options).toEqual([t.venuePlaceholder, ...VENUES.map((v) => v.name)]);
    expect(screen.getByText(t.venueMissing)).toBeTruthy();
  });

  it("asks for a venue before sending anything", async () => {
    const onSubmit = vi.fn();
    render(<CreateMatchForm venues={VENUES} onSubmit={onSubmit} />);
    fillRequired();
    fireEvent.change(field(t.venue), { target: { value: "" } });

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.venueId);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("shows the chosen venue's address", () => {
    render(<CreateMatchForm venues={VENUES} onSubmit={vi.fn()} />);

    fireEvent.change(field(t.venue), { target: { value: "an-phu-nguyen-hoang" } });

    expect(screen.getByText("93 Nguyễn Hoàng, Bình Trưng")).toBeTruthy();
  });

  it("says when the venue list could not be loaded", () => {
    render(<CreateMatchForm venues={[]} onSubmit={vi.fn()} />);

    expect(screen.getByText(t.venuesUnavailable)).toBeTruthy();
  });
});

