import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import messages from "../messages/vi.json";
import type { MatchView } from "../lib/api";
import { MatchCard } from "./MatchCard";

const match: MatchView = {
  shareId: "k7Qm2xPa",
  venueName: "SSA Sports Center",
  startsAt: "2026-10-10T11:30:00Z",
  endsAt: "2026-10-10T13:00:00Z",
  format: "seven_a_side",
  matchType: "casual",
  levelMin: 2.5,
  levelMax: 3.5,
  totalFeeVnd: 900000,
  slotCount: 14,
};

describe("MatchCard", () => {
  it("shows venue, format and match type labels", () => {
    render(<MatchCard match={match} />);

    expect(screen.getByRole("heading", { name: "SSA Sports Center" })).toBeTruthy();
    expect(screen.getByText(messages.match.format.seven_a_side)).toBeTruthy();
    expect(screen.getByText(messages.match.matchType.casual)).toBeTruthy();
  });

  it("shows the time range in Ho Chi Minh City time", () => {
    render(<MatchCard match={match} />);

    // 11:30Z-13:00Z is 18:30-20:00 in Asia/Ho_Chi_Minh (UTC+7).
    expect(screen.getByText("18:30 – 20:00")).toBeTruthy();
    expect(screen.getByText(/10\/10/)).toBeTruthy();
  });

  it("shows level range, total fee and slot count", () => {
    render(<MatchCard match={match} />);

    expect(screen.getByText("2.5 – 3.5")).toBeTruthy();
    expect(screen.getByText(/900\.000/)).toBeTruthy();
    expect(screen.getByText(`14 ${messages.match.slotsUnit}`)).toBeTruthy();
  });
});
