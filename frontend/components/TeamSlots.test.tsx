import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { MyPlace, RosterView, SlotsClient } from "../lib/slots";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";
import { TeamSlots } from "./TeamSlots";

const t = messages.slots;
const SHARE_ID = "k7Qm2xPa";
const FUTURE = "2099-10-10T11:30:00Z";

function roster(aPlayers: string[] = [], bPlayers: string[] = [], capacity = 3): RosterView {
  const player = (name: string) => ({ name, avatarUrl: null, isGuest: false, guestOf: null });
  return {
    teams: [
      { team: "a", capacity, players: aPlayers.map(player) },
      { team: "b", capacity, players: bPlayers.map(player) },
    ],
  };
}

function client(mine: MyPlace, overrides: Partial<SlotsClient> = {}): SlotsClient {
  return {
    roster: vi.fn().mockResolvedValue(roster()),
    mine: vi.fn().mockResolvedValue(mine),
    join: vi.fn(),
    leave: vi.fn(),
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

    render(<TeamSlots shareId={SHARE_ID} startsAt={FUTURE} initialRoster={initial} client={client({ status: "out" })} />);

    const a = team(t.team.a);
    expect(within(a).getByText("Long")).toBeTruthy();
    expect(within(a).getByText(fill(t.guestOf, { name: "Long" }))).toBeTruthy();
    expect(within(a).getByText(fill(t.openPlaces, { count: 1 }))).toBeTruthy();
    expect(within(team(t.team.b)).getByText(fill(t.openPlaces, { count: 3 }))).toBeTruthy();
  });

  it("replaces the cached roster with a fresh one after loading", async () => {
    const api = client({ status: "out" }, { roster: vi.fn().mockResolvedValue(roster(["Long"])) });

    render(<TeamSlots shareId={SHARE_ID} startsAt={FUTURE} initialRoster={roster()} client={api} />);

    expect(await within(team(t.team.a)).findByText("Long")).toBeTruthy();
    expect(api.roster).toHaveBeenCalledWith(SHARE_ID);
  });

  it("asks signed-out visitors to sign in and come back to this match", async () => {
    render(
      <TeamSlots shareId={SHARE_ID} startsAt={FUTURE} initialRoster={roster()} client={client({ status: "signedOut" })} />,
    );

    const link = await screen.findByRole("link", { name: t.signInToJoin });
    expect(link.getAttribute("href")).toBe(`/api/auth/google/start?next=${encodeURIComponent(`/m/${SHARE_ID}`)}`);
  });

  it("joins the chosen team with a guest and shows the updated roster", async () => {
    const api = client(
      { status: "out" },
      {
        join: vi.fn().mockResolvedValue({ ok: true, place: { status: "in", team: "b", guests: ["An"] } }),
        // First the refresh on load, then the refresh after joining.
        roster: vi.fn().mockResolvedValueOnce(roster()).mockResolvedValue(roster([], ["Long", "An"])),
      },
    );
    render(<TeamSlots shareId={SHARE_ID} startsAt={FUTURE} initialRoster={roster()} client={api} />);

    fireEvent.click(await screen.findByRole("radio", { name: t.team.b }));
    fireEvent.click(screen.getByRole("button", { name: t.addGuest }));
    fireEvent.change(screen.getByPlaceholderText(t.guestPlaceholder), { target: { value: "An" } });
    fireEvent.click(screen.getByRole("button", { name: fill(t.join, { team: t.team.b }) }));

    expect(await screen.findByText(fill(t.joined, { team: t.team.b }))).toBeTruthy();
    expect(screen.getByText(fill(t.withGuests, { names: "An" }))).toBeTruthy();
    expect(api.join).toHaveBeenCalledWith(SHARE_ID, "b", ["An"]);
    expect(within(team(t.team.b)).getByText("Long")).toBeTruthy();
  });

  it("allows at most two guests", async () => {
    render(<TeamSlots shareId={SHARE_ID} startsAt={FUTURE} initialRoster={roster()} client={client({ status: "out" })} />);

    fireEvent.click(await screen.findByRole("button", { name: t.addGuest }));
    fireEvent.click(screen.getByRole("button", { name: t.addGuest }));

    expect(screen.getAllByPlaceholderText(t.guestPlaceholder)).toHaveLength(2);
    expect(screen.queryByRole("button", { name: t.addGuest })).toBeNull();
  });

  it("disables a team that cannot fit the party", async () => {
    const current = roster(["P1", "P2"], ["P3", "P4", "P5"], 3);
    render(
      <TeamSlots
        shareId={SHARE_ID}
        startsAt={FUTURE}
        initialRoster={current}
        client={client({ status: "out" }, { roster: vi.fn().mockResolvedValue(current) })}
      />,
    );

    const teamA = (await screen.findByRole("radio", { name: t.team.a })) as HTMLInputElement;
    const teamB = screen.getByRole("radio", { name: t.team.b }) as HTMLInputElement;
    expect(teamA.disabled).toBe(false);
    expect(teamB.disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: t.addGuest }));
    expect(teamA.disabled).toBe(true);
  });

  it("explains a refusal and refreshes the roster", async () => {
    const api = client(
      { status: "out" },
      {
        join: vi.fn().mockResolvedValue({ ok: false, error: "team_full" }),
        roster: vi.fn().mockResolvedValueOnce(roster()).mockResolvedValue(roster(["X", "Y", "Z"], [])),
      },
    );
    render(<TeamSlots shareId={SHARE_ID} startsAt={FUTURE} initialRoster={roster()} client={api} />);

    fireEvent.click(await screen.findByRole("button", { name: fill(t.join, { team: t.team.a }) }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.team_full);
    await waitFor(() => expect(within(team(t.team.a)).getByText(t.full)).toBeTruthy());
  });

  it("lets a player who joined leave the match", async () => {
    const api = client(
      { status: "in", team: "a", guests: [] },
      { leave: vi.fn().mockResolvedValue({ ok: true }) },
    );
    render(<TeamSlots shareId={SHARE_ID} startsAt={FUTURE} initialRoster={roster(["Long"])} client={api} />);

    fireEvent.click(await screen.findByRole("button", { name: t.leave }));

    expect(await screen.findByRole("button", { name: fill(t.join, { team: t.team.a }) })).toBeTruthy();
    expect(api.leave).toHaveBeenCalledWith(SHARE_ID);
  });

  it("shows no actions once the match has started", async () => {
    const api = client({ status: "out" });
    render(
      <TeamSlots shareId={SHARE_ID} startsAt="2000-01-01T00:00:00Z" initialRoster={roster()} client={api} />,
    );

    expect(await screen.findByText(t.started)).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
