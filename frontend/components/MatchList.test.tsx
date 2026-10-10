import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { OpenMatch, OpenMatchPage } from "../lib/api";
import { formatVnd } from "../lib/format";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";
import { MatchList } from "./MatchList";

const t = messages.list;
// 07:00 on Saturday 3 October 2099 in Ho Chi Minh City.
const NOW = () => new Date("2099-10-03T00:00:00Z");

function open(shareId: string, overrides: Partial<OpenMatch> = {}): OpenMatch {
  return {
    shareId,
    venueName: "SSA Sports Center (Amitie Thảo Điền)",
    venueAddress: "28 Duyên Hải, An Khánh",
    startsAt: "2099-10-03T11:30:00Z",
    endsAt: "2099-10-03T13:00:00Z",
    format: "seven_a_side",
    matchType: "casual",
    levelMin: 2.5,
    levelMax: 3.5,
    totalFeeVnd: 900000,
    slotCount: 18,
    pricePerPlayerVnd: 50000,
    cancelledAt: null,
    placesLeft: 3,
    distanceM: null,
    ...overrides,
  };
}

function page(matches: OpenMatch[], nextCursor: string | null = null): OpenMatchPage {
  return { matches, nextCursor };
}

describe("MatchList", () => {
  it("loads today's open matches and links each to its page", async () => {
    const load = vi.fn().mockResolvedValue(page([open("abcdefgh")]));
    render(<MatchList load={load} now={NOW} />);

    const card = await screen.findByRole("link", { name: /SSA Sports Center/ });
    expect(card.getAttribute("href")).toBe("/m/abcdefgh");
    expect(within(card).getByText("18:30")).toBeTruthy();
    expect(within(card).getByText(fill(messages.match.placesLeft, { count: 3 }))).toBeTruthy();
    expect(within(card).getByText("50k")).toBeTruthy();
    expect(screen.getByText(fill(t.summary, { count: 1 }))).toBeTruthy();
    expect(load).toHaveBeenCalledWith({ date: "2099-10-03" });
  });

  it("switches day and match type", async () => {
    const load = vi.fn().mockResolvedValue(page([]));
    render(<MatchList load={load} now={NOW} />);
    await waitFor(() => expect(load).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: new RegExp(t.tomorrow) }));
    await waitFor(() => expect(load).toHaveBeenLastCalledWith({ date: "2099-10-04" }));

    fireEvent.click(screen.getByRole("button", { name: messages.match.matchType.competitive }));
    await waitFor(() => expect(load).toHaveBeenLastCalledWith({ date: "2099-10-04", type: "competitive" }));
    expect(screen.getByRole("button", { name: messages.match.matchType.competitive }).getAttribute("aria-pressed")).toBe(
      "true",
    );

    fireEvent.click(screen.getByRole("button", { name: t.allTypes }));
    await waitFor(() => expect(load).toHaveBeenLastCalledWith({ date: "2099-10-04" }));
  });

  it("ignores an answer that arrives after the filters changed", async () => {
    let finishFirst: (value: OpenMatchPage) => void = () => {};
    const load = vi
      .fn()
      .mockReturnValueOnce(new Promise<OpenMatchPage>((resolve) => (finishFirst = resolve)))
      .mockResolvedValue(page([open("tomorrow1", { venueName: "Sân bóng An Phú Quận 2" })]));
    render(<MatchList load={load} now={NOW} />);

    fireEvent.click(screen.getByRole("button", { name: new RegExp(t.tomorrow) }));
    expect(await screen.findByText("Sân bóng An Phú Quận 2")).toBeTruthy();
    finishFirst(page([open("today123")]));

    await waitFor(() => expect(screen.queryByRole("link", { name: /SSA Sports Center/ })).toBeNull());
  });

  it("shows distances after the visitor shares where they are", async () => {
    const load = vi
      .fn()
      .mockResolvedValueOnce(page([open("abcdefgh")]))
      .mockResolvedValue(page([open("abcdefgh", { distanceM: 1234 })]));
    const locate = vi.fn().mockResolvedValue({ lat: 10.8, lng: 106.74 });
    render(<MatchList load={load} now={NOW} locate={locate} />);
    await screen.findByRole("link", { name: /SSA Sports Center/ });

    fireEvent.click(screen.getByRole("button", { name: t.nearMe }));

    expect(await screen.findByText("1,2 km")).toBeTruthy();
    expect(load).toHaveBeenLastCalledWith({ date: "2099-10-03", near: { lat: 10.8, lng: 106.74 } });
  });

  it("says when the position is not available and keeps the list", async () => {
    const load = vi.fn().mockResolvedValue(page([open("abcdefgh")]));
    const locate = vi.fn().mockRejectedValue(new Error("denied"));
    render(<MatchList load={load} now={NOW} locate={locate} />);
    await screen.findByRole("link", { name: /SSA Sports Center/ });

    fireEvent.click(screen.getByRole("button", { name: t.nearMe }));

    expect(await screen.findByText(t.locationFailed)).toBeTruthy();
    expect(screen.getByRole("link", { name: /SSA Sports Center/ })).toBeTruthy();
  });

  it("loads more matches with the cursor", async () => {
    const load = vi
      .fn()
      .mockResolvedValueOnce(page([open("first111")], "c1"))
      .mockResolvedValue(page([open("second22", { venueName: "Khu thể thao An Phú" })]));
    render(<MatchList load={load} now={NOW} />);

    fireEvent.click(await screen.findByRole("button", { name: t.loadMore }));

    expect(await screen.findByText("Khu thể thao An Phú")).toBeTruthy();
    expect(screen.getAllByRole("link", { name: /SSA|Khu/ })).toHaveLength(2);
    expect(load).toHaveBeenLastCalledWith({ date: "2099-10-03", cursor: "c1" });
    expect(screen.queryByRole("button", { name: t.loadMore })).toBeNull();
  });

  it("invites hosting when no match needs players", async () => {
    render(<MatchList load={vi.fn().mockResolvedValue(page([]))} now={NOW} />);

    expect(await screen.findByText(t.empty)).toBeTruthy();
    expect(screen.getByRole("link", { name: messages.app.createLink }).getAttribute("href")).toBe("/create");
  });

  it("offers a retry when the list cannot be loaded", async () => {
    const load = vi.fn().mockRejectedValueOnce(new Error("down")).mockResolvedValue(page([open("abcdefgh")]));
    render(<MatchList load={load} now={NOW} />);

    fireEvent.click(await screen.findByRole("button", { name: t.retry }));

    expect(await screen.findByRole("link", { name: /SSA Sports Center/ })).toBeTruthy();
  });

  it("shows free matches as free", async () => {
    render(
      <MatchList
        load={vi.fn().mockResolvedValue(page([open("abcdefgh", { pricePerPlayerVnd: 0, totalFeeVnd: 0 })]))}
        now={NOW}
      />,
    );

    const card = await screen.findByRole("link", { name: /SSA Sports Center/ });
    expect(within(card).getByText(t.free)).toBeTruthy();
    expect(within(card).queryByText(formatVnd(0))).toBeNull();
  });
});
