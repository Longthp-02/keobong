import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import messages from "../messages/vi.json";
import type { MatchView } from "../lib/api";
import type { RosterView } from "../lib/slots";
import { fill } from "../lib/text";
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
  pricePerPlayerVnd: 65000,
  cancelledAt: null,
};

describe("MatchCard", () => {
  it("marks a cancelled match", () => {
    const { rerender } = render(<MatchCard match={match} />);
    expect(screen.queryByText(messages.match.cancelled)).toBeNull();

    rerender(<MatchCard match={{ ...match, cancelledAt: "2026-10-09T10:00:00Z" }} />);
    expect(screen.getByText(messages.match.cancelled)).toBeTruthy();
  });

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

  it("highlights the price each player pays", () => {
    render(<MatchCard match={match} />);

    expect(screen.getByTestId("match-price").textContent).toMatch(/65\.000/);
  });

  it("shows how many have joined and how many places are left", () => {
    const player = { name: "P", avatarUrl: null, isGuest: false, guestOf: null };
    const roster: RosterView = {
      teams: [
        { team: "a", capacity: 7, players: [player, player, player] },
        { team: "b", capacity: 7, players: [player] },
      ],
      cancelled: false,
    };
    render(<MatchCard match={match} roster={roster} />);

    expect(screen.getByText(fill(messages.match.placesLeft, { count: 10 }))).toBeTruthy();
    expect(screen.getByTestId("match-joined").textContent).toBe("4/14");
  });

  it("says when the match is full", () => {
    const player = { name: "P", avatarUrl: null, isGuest: false, guestOf: null };
    const roster: RosterView = {
      teams: [
        { team: "a", capacity: 1, players: [player] },
        { team: "b", capacity: 1, players: [player] },
      ],
      cancelled: false,
    };
    render(<MatchCard match={{ ...match, slotCount: 2 }} roster={roster} />);

    expect(screen.getByText(messages.match.matchFull)).toBeTruthy();
    expect(screen.getByTestId("match-joined").textContent).toBe("2/2");
  });

  it("marks the match cancelled when only the fresher roster knows", () => {
    const roster: RosterView = {
      teams: [
        { team: "a", capacity: 7, players: [] },
        { team: "b", capacity: 7, players: [] },
      ],
      cancelled: true,
    };
    render(<MatchCard match={match} roster={roster} />);

    expect(screen.getByText(messages.match.cancelled)).toBeTruthy();
    expect(screen.queryByText(fill(messages.match.placesLeft, { count: 14 }))).toBeNull();
  });
});
