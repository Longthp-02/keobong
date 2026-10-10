import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { MatchView } from "../lib/api";
import { formatVnd } from "../lib/format";
import type { JoinedPlace, MyPlace, RosterView, SlotsClient, Team } from "../lib/slots";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";
import { TeamSlots } from "./TeamSlots";

const t = messages.slots;
const SHARE_ID = "k7Qm2xPa";
const FUTURE = "2099-10-10T11:30:00Z";
const MATCH: MatchView = {
  shareId: SHARE_ID,
  venueName: "SSA",
  startsAt: FUTURE,
  endsAt: "2099-10-10T13:00:00Z",
  format: "five_a_side",
  matchType: "casual",
  levelMin: 2.5,
  levelMax: 3.5,
  totalFeeVnd: 0,
  slotCount: 6,
  pricePerPlayerVnd: 0,
  cancelledAt: null,
};

function roster(aPlayers: string[] = [], bPlayers: string[] = [], capacity = 3): RosterView {
  const player = (name: string) => ({ name, avatarUrl: null, isGuest: false, guestOf: null });
  return {
    teams: [
      { team: "a", capacity, players: aPlayers.map(player) },
      { team: "b", capacity, players: bPlayers.map(player) },
    ],
    cancelled: false,
  };
}

function client(mine: MyPlace, overrides: Partial<SlotsClient> = {}): SlotsClient {
  return {
    roster: vi.fn().mockResolvedValue(roster()),
    mine: vi.fn().mockResolvedValue(mine),
    join: vi.fn(),
    leave: vi.fn(),
    reportPayment: vi.fn(),
    hostParties: vi.fn().mockResolvedValue({ status: "notHost" }),
    hostAction: vi.fn(),
    cancelMatch: vi.fn(),
    ...overrides,
  };
}

function joined(team: Team, guests: string[], overrides: Partial<JoinedPlace> = {}): JoinedPlace {
  return {
    status: "in",
    team,
    guests,
    paymentCode: 7,
    paymentStatus: "confirmed",
    holdExpiresAt: null,
    amountVnd: 0,
    payment: null,
    ...overrides,
  };
}

function team(name: string) {
  return screen.getByRole("region", { name });
}

describe("TeamSlots", () => {
  it("lists players and open places for each team", () => {
    const initial = roster(["Long"], [], 3);
    initial.teams[0].players.push({ name: "An", avatarUrl: null, isGuest: true, guestOf: "Long" });

    render(<TeamSlots match={MATCH} initialRoster={initial} client={client({ status: "out" })} />);

    const a = team(t.team.a);
    expect(within(a).getByText("Long")).toBeTruthy();
    expect(within(a).getByText(fill(t.guestOf, { name: "Long" }))).toBeTruthy();
    expect(within(a).getByText(fill(t.openPlaces, { count: 1 }))).toBeTruthy();
    expect(within(team(t.team.b)).getByText(fill(t.openPlaces, { count: 3 }))).toBeTruthy();
  });

  it("replaces the cached roster with a fresh one after loading", async () => {
    const api = client({ status: "out" }, { roster: vi.fn().mockResolvedValue(roster(["Long"])) });

    render(<TeamSlots match={MATCH} initialRoster={roster()} client={api} />);

    expect(await within(team(t.team.a)).findByText("Long")).toBeTruthy();
    expect(api.roster).toHaveBeenCalledWith(SHARE_ID);
  });

  it("asks signed-out visitors to sign in and come back to this match", async () => {
    render(
      <TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "signedOut" })} />,
    );

    const link = await screen.findByRole("link", { name: t.signInToJoin });
    expect(link.getAttribute("href")).toBe(`/api/auth/google/start?next=${encodeURIComponent(`/m/${SHARE_ID}`)}`);
  });

  it("joins the chosen team with a guest and shows the updated roster", async () => {
    const api = client(
      { status: "out" },
      {
        join: vi.fn().mockResolvedValue({ ok: true, place: joined("b", ["An"]) }),
        // First the refresh on load, then the refresh after joining.
        roster: vi.fn().mockResolvedValueOnce(roster()).mockResolvedValue(roster([], ["Long", "An"])),
      },
    );
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={api} />);

    fireEvent.click(await screen.findByRole("button", { name: fill(t.takeSlot, { team: t.team.b }) }));
    fireEvent.click(screen.getByRole("button", { name: t.addGuest }));
    fireEvent.change(screen.getByPlaceholderText(t.guestPlaceholder), { target: { value: "An" } });
    fireEvent.click(screen.getByRole("button", { name: fill(t.join, { team: t.team.b }) }));

    expect(await screen.findByText(fill(t.joined, { team: t.team.b }))).toBeTruthy();
    expect(screen.getByText(fill(t.withGuests, { names: "An" }))).toBeTruthy();
    expect(api.join).toHaveBeenCalledWith(SHARE_ID, "b", ["An"]);
    expect(within(team(t.team.b)).getByText("Long")).toBeTruthy();
  });

  it("allows at most two guests", async () => {
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "out" })} />);

    fireEvent.click(await screen.findByRole("button", { name: t.addGuest }));
    fireEvent.click(screen.getByRole("button", { name: t.addGuest }));

    expect(screen.getAllByPlaceholderText(t.guestPlaceholder)).toHaveLength(2);
    expect(screen.queryByRole("button", { name: t.addGuest })).toBeNull();
  });

  it("disables a team that cannot fit the party", async () => {
    const current = roster(["P1", "P2"], ["P3"], 3);
    render(
      <TeamSlots
        match={MATCH}
        initialRoster={current}
        client={client({ status: "out" }, { roster: vi.fn().mockResolvedValue(current) })}
      />,
    );

    const slotsA = () => screen.getAllByRole("button", { name: fill(t.takeSlot, { team: t.team.a }) }) as HTMLButtonElement[];
    const slotsB = () => screen.getAllByRole("button", { name: fill(t.takeSlot, { team: t.team.b }) }) as HTMLButtonElement[];
    await screen.findAllByRole("button", { name: fill(t.takeSlot, { team: t.team.a }) });
    expect(slotsA().every((slot) => !slot.disabled)).toBe(true);
    expect(slotsB().every((slot) => !slot.disabled)).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: t.addGuest }));
    expect(slotsA().every((slot) => slot.disabled)).toBe(true);
    expect(slotsB().every((slot) => !slot.disabled)).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: t.addGuest }));
    expect(slotsB().every((slot) => slot.disabled)).toBe(true);
  });

  it("explains a refusal and refreshes the roster", async () => {
    const api = client(
      { status: "out" },
      {
        join: vi.fn().mockResolvedValue({ ok: false, error: "team_full" }),
        roster: vi.fn().mockResolvedValueOnce(roster()).mockResolvedValue(roster(["X", "Y", "Z"], [])),
      },
    );
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={api} />);

    fireEvent.click(await screen.findByRole("button", { name: fill(t.join, { team: t.team.a }) }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.team_full);
    await waitFor(() => expect(within(team(t.team.a)).getByText(t.full)).toBeTruthy());
  });

  it("lets a player who joined leave the match", async () => {
    const api = client(
      joined("a", []),
      { leave: vi.fn().mockResolvedValue({ ok: true }) },
    );
    render(<TeamSlots match={MATCH} initialRoster={roster(["Long"])} client={api} />);

    fireEvent.click(await screen.findByRole("button", { name: t.leave }));

    expect(await screen.findByRole("button", { name: fill(t.join, { team: t.team.a }) })).toBeTruthy();
    expect(api.leave).toHaveBeenCalledWith(SHARE_ID);
  });

  it("offers sign-in again when the session expired while joining", async () => {
    const api = client({ status: "out" }, { join: vi.fn().mockResolvedValue({ ok: false, error: "unauthenticated" }) });
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={api} />);

    fireEvent.click(await screen.findByRole("button", { name: fill(t.join, { team: t.team.a }) }));

    expect(await screen.findByRole("link", { name: t.signInToJoin })).toBeTruthy();
  });

  it("closes the form when the server says the match has started", async () => {
    const api = client({ status: "out" }, { join: vi.fn().mockResolvedValue({ ok: false, error: "match_started" }) });
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={api} />);

    fireEvent.click(await screen.findByRole("button", { name: fill(t.join, { team: t.team.a }) }));

    expect(await screen.findByText(t.started)).toBeTruthy();
    expect(screen.queryByRole("button", { name: fill(t.join, { team: t.team.a }) })).toBeNull();
  });

  it("offers a retry when the player's own place cannot be loaded", async () => {
    const mine = vi.fn().mockRejectedValueOnce(new Error("down")).mockResolvedValue({ status: "out" });
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "out" }, { mine })} />);

    fireEvent.click(await screen.findByRole("button", { name: t.retry }));

    expect(await screen.findByRole("button", { name: fill(t.join, { team: t.team.a }) })).toBeTruthy();
  });

  it("shows no actions once the match has started", async () => {
    const api = client({ status: "out" });
    render(
      <TeamSlots match={{ ...MATCH, startsAt: "2000-01-01T00:00:00Z" }} initialRoster={roster()} client={api} />,
    );

    expect(await screen.findByText(t.started)).toBeTruthy();
    expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual([t.share]);
  });

  it("shows how to pay after joining a paid match and reports the transfer", async () => {
    const payment = {
      bankName: "ACB",
      accountNumber: "257678859",
      accountName: "PHAM LONG",
      amountVnd: 50000,
      memo: "DG k7Qm2xPa 7",
      qrPayload: "000201",
    };
    const awaiting = joined("a", [], {
      paymentStatus: "awaiting_payment",
      holdExpiresAt: "2099-10-10T11:42:00Z",
      amountVnd: 50000,
      payment,
    });
    const api = client(awaiting, {
      reportPayment: vi.fn().mockResolvedValue({
        ok: true,
        place: { ...awaiting, paymentStatus: "payment_reported", holdExpiresAt: null },
      }),
    });
    render(<TeamSlots match={MATCH} initialRoster={roster(["Long"])} client={api} />);

    fireEvent.click(await screen.findByRole("button", { name: messages.payment.reportPaid }));

    expect(await screen.findByText(messages.payment.reported)).toBeTruthy();
    expect(api.reportPayment).toHaveBeenCalledWith(SHARE_ID);
  });

  it("shows the host's payment list to the host", async () => {
    const api = client(
      { status: "out" },
      {
        hostParties: vi.fn().mockResolvedValue({
          status: "host",
          parties: [
            {
              paymentCode: 7,
              team: "a",
              holderName: "Minh",
              guests: [],
              amountVnd: 50000,
              paymentStatus: "payment_reported",
              holdExpiresAt: null,
            },
          ],
        }),
      },
    );
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={api} />);

    expect(await screen.findByRole("heading", { name: messages.host.title })).toBeTruthy();
  });

  it("shows a cancelled match without any way to join", async () => {
    const cancelled = { ...roster(["Long"]), cancelled: true };
    const api = client({ status: "out" }, { roster: vi.fn().mockResolvedValue(cancelled) });
    render(<TeamSlots match={MATCH} initialRoster={cancelled} client={api} />);

    expect(await screen.findByText(t.cancelled)).toBeTruthy();
    expect(screen.queryByRole("button", { name: fill(t.join, { team: t.team.a }) })).toBeNull();
  });

  it("tells a player who joined a cancelled match how to get a refund", async () => {
    const cancelled = { ...roster(["Long"]), cancelled: true };
    const place = joined("a", [], { paymentStatus: "payment_reported", amountVnd: 50000 });
    const api = client(place, { roster: vi.fn().mockResolvedValue(cancelled) });
    render(<TeamSlots match={MATCH} initialRoster={cancelled} client={api} />);

    expect(await screen.findByText(t.cancelledRefund)).toBeTruthy();
    expect(screen.queryByRole("button", { name: messages.payment.reportPaid })).toBeNull();
    expect(screen.queryByRole("button", { name: t.leave })).toBeNull();
  });

  it("switches to the cancelled view when the server refuses a report because of a cancel", async () => {
    const place = joined("a", [], {
      paymentStatus: "awaiting_payment",
      holdExpiresAt: "2099-10-10T11:00:00Z",
      amountVnd: 50000,
      payment: {
        bankName: "ACB",
        accountNumber: "257678859",
        accountName: "PHAM LONG",
        amountVnd: 50000,
        memo: "DAGHEP 7",
        qrPayload: "000201",
      },
    });
    const api = client(place, {
      roster: vi
        .fn()
        .mockResolvedValueOnce(roster(["Long"]))
        .mockResolvedValue({ ...roster(["Long"]), cancelled: true }),
      reportPayment: vi.fn().mockResolvedValue({ ok: false, error: "match_cancelled" }),
    });
    render(<TeamSlots match={MATCH} initialRoster={roster(["Long"])} client={api} />);

    fireEvent.click(await screen.findByRole("button", { name: messages.payment.reportPaid }));

    expect(await screen.findByText(t.cancelledRefund)).toBeTruthy();
    expect(screen.queryByRole("button", { name: messages.payment.reportPaid })).toBeNull();
  });

  it("shows the price of the whole party on the join button", async () => {
    const paid = { ...MATCH, totalFeeVnd: 300000, pricePerPlayerVnd: 50000 };
    render(<TeamSlots match={paid} initialRoster={roster()} client={client({ status: "out" })} />);

    const single = `${fill(t.join, { team: t.team.a })} · ${formatVnd(50000)}`;
    expect(await screen.findByRole("button", { name: single })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: t.addGuest }));
    expect(screen.getByRole("button", { name: `${fill(t.join, { team: t.team.a })} · ${formatVnd(100000)}` })).toBeTruthy();
  });

  it("explains how paying works before joining a paid match", async () => {
    const paid = { ...MATCH, totalFeeVnd: 300000, pricePerPlayerVnd: 50000 };
    const { rerender } = render(<TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "out" })} />);
    await screen.findByRole("button", { name: fill(t.join, { team: t.team.a }) });
    expect(screen.queryByText(t.payInfoTitle)).toBeNull();

    rerender(<TeamSlots match={paid} initialRoster={roster()} client={client({ status: "out" })} />);
    expect(await screen.findByText(t.payInfoTitle)).toBeTruthy();
  });

  it("marks the chosen place in the roster", async () => {
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "out" })} />);

    fireEvent.click(await screen.findByRole("button", { name: fill(t.takeSlot, { team: t.team.b }) }));

    expect(within(team(t.team.b)).getByText(t.you)).toBeTruthy();
    expect(within(team(t.team.a)).queryByText(t.you)).toBeNull();
    expect(screen.getByRole("button", { name: fill(t.join, { team: t.team.b }) })).toBeTruthy();
  });

  it("shows the header with live counts", async () => {
    const api = client({ status: "out" }, { roster: vi.fn().mockResolvedValue(roster(["A1", "A2"], ["B1"])) });
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={api} />);

    await waitFor(() => expect(screen.getByTestId("match-joined").textContent).toBe("3/6"));
  });

  it("shares the match link with the system share sheet", async () => {
    const share = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "share", { value: share, configurable: true });
    try {
      render(<TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "out" })} />);

      fireEvent.click(await screen.findByRole("button", { name: t.share }));

      await waitFor(() => expect(share).toHaveBeenCalled());
      expect(share.mock.calls[0][0].url).toBe(`${window.location.origin}/m/${SHARE_ID}`);
    } finally {
      Reflect.deleteProperty(navigator, "share");
    }
  });

  it("copies the match link when the browser cannot share", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    try {
      render(<TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "out" })} />);

      fireEvent.click(await screen.findByRole("button", { name: t.share }));

      expect(await screen.findByText(t.linkCopied)).toBeTruthy();
      expect(writeText).toHaveBeenCalledWith(`${window.location.origin}/m/${SHARE_ID}`);
    } finally {
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });

  it("offers one choice per team to assistive technology", async () => {
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "out" })} />);

    // Three open places in each team, but one button per team; the other circles only repeat it.
    expect(await screen.findAllByRole("button", { name: fill(t.takeSlot, { team: t.team.a }) })).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: fill(t.takeSlot, { team: t.team.b }) })).toHaveLength(1);
  });

  it("marks a place for each guest in the chosen team", async () => {
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "out" })} />);

    fireEvent.click(await screen.findByRole("button", { name: t.addGuest }));
    fireEvent.click(screen.getByRole("button", { name: t.addGuest }));

    const a = team(t.team.a);
    expect(within(a).getAllByText(t.you)).toHaveLength(1);
    expect(within(a).getAllByText(t.guestShort)).toHaveLength(2);
  });

  it("does not ask the host to pay for their own match", async () => {
    const paid = { ...MATCH, totalFeeVnd: 300000, pricePerPlayerVnd: 50000 };
    const api = client({ status: "out" }, { hostParties: vi.fn().mockResolvedValue({ status: "host", parties: [] }) });
    render(<TeamSlots match={paid} initialRoster={roster()} client={api} />);

    expect(await screen.findByRole("button", { name: fill(t.join, { team: t.team.a }) })).toBeTruthy();
    expect(screen.queryByText(t.payInfoTitle)).toBeNull();
  });

  it("leaves the counts out of the header while the roster is unknown", async () => {
    const api = client({ status: "out" }, { roster: vi.fn().mockRejectedValue(new Error("down")) });
    render(<TeamSlots match={MATCH} initialRoster={null} client={api} />);

    await waitFor(() => expect(api.roster).toHaveBeenCalled());
    expect(screen.queryByTestId("match-joined")).toBeNull();
    expect(screen.getByText(`${MATCH.slotCount} ${messages.match.slotsUnit}`)).toBeTruthy();
  });

  it("stops showing places left once the match has started", async () => {
    const api = client({ status: "out" });
    render(<TeamSlots match={{ ...MATCH, startsAt: "2000-01-01T00:00:00Z" }} initialRoster={roster()} client={api} />);

    expect(await screen.findByText(t.started)).toBeTruthy();
    expect(screen.queryByText(fill(messages.match.placesLeft, { count: 6 }))).toBeNull();
  });

  it("does nothing when the visitor closes the share sheet", async () => {
    const share = vi.fn().mockRejectedValue(new DOMException("closed", "AbortError"));
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "share", { value: share, configurable: true });
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    try {
      render(<TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "out" })} />);

      fireEvent.click(await screen.findByRole("button", { name: t.share }));

      await waitFor(() => expect(share).toHaveBeenCalled());
      expect(writeText).not.toHaveBeenCalled();
      expect(screen.getByRole("status").textContent).toBe("");
    } finally {
      Reflect.deleteProperty(navigator, "share");
      Reflect.deleteProperty(navigator, "clipboard");
    }
  });

  it("says when the link could not be shared or copied", async () => {
    render(<TeamSlots match={MATCH} initialRoster={roster()} client={client({ status: "out" })} />);

    fireEvent.click(await screen.findByRole("button", { name: t.share }));

    // jsdom has neither a share sheet nor a clipboard.
    expect(await screen.findByText(t.shareFailed)).toBeTruthy();
  });
});
