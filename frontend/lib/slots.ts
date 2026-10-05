/**
 * Browser-side client for places in a match. Calls go to the site's own `/api`
 * proxy, so the session cookie is sent automatically and stays first-party.
 */

export type Team = "a" | "b";

export type PlayerView = {
  name: string | null;
  avatarUrl: string | null;
  isGuest: boolean;
  /** For a guest, the display name of the player who brought them. */
  guestOf: string | null;
};

export type TeamView = { team: Team; capacity: number; players: PlayerView[] };

/** Public roster from `GET /api/matches/{shareId}/slots`. */
export type RosterView = { teams: TeamView[] };

export type MyPlace =
  | { status: "signedOut" }
  | { status: "out" }
  | { status: "in"; team: Team; guests: string[] };

export type SlotError =
  | "team_full"
  | "already_joined"
  | "match_started"
  | "guests"
  | "unauthenticated"
  | "unexpected";

export type JoinResult = { ok: true; place: MyPlace } | { ok: false; error: SlotError };
export type LeaveResult = { ok: true } | { ok: false; error: SlotError };

export type SlotsClient = {
  roster(shareId: string): Promise<RosterView>;
  mine(shareId: string): Promise<MyPlace>;
  join(shareId: string, team: Team, guests: string[]): Promise<JoinResult>;
  leave(shareId: string): Promise<LeaveResult>;
};

/** A roster with nobody in it, split like the API: team A takes the odd place. */
export function emptyRoster(slotCount: number): RosterView {
  return {
    teams: [
      { team: "a", capacity: Math.ceil(slotCount / 2), players: [] },
      { team: "b", capacity: Math.floor(slotCount / 2), players: [] },
    ],
  };
}

type PlaceBody = { joined: boolean; team?: Team; guests?: string[] };

function toPlace(body: PlaceBody): MyPlace {
  return body.joined && body.team ? { status: "in", team: body.team, guests: body.guests ?? [] } : { status: "out" };
}

const KNOWN_CONFLICTS: SlotError[] = ["team_full", "already_joined", "match_started"];

async function errorOf(response: Response): Promise<SlotError> {
  if (response.status === 401) {
    return "unauthenticated";
  }
  const body = (await response.json().catch(() => ({}))) as { error?: string; field?: string };
  if (response.status === 409 && KNOWN_CONFLICTS.includes(body.error as SlotError)) {
    return body.error as SlotError;
  }
  if (response.status === 422 && body.field === "guests") {
    return "guests";
  }
  return "unexpected";
}

export function slotsClient(fetchImpl: typeof fetch = (...args) => fetch(...args)): SlotsClient {
  const base = (shareId: string) => `/api/matches/${encodeURIComponent(shareId)}/slots`;
  return {
    async roster(shareId) {
      const response = await fetchImpl(base(shareId), { cache: "no-store" });
      if (!response.ok) {
        throw new Error(`Roster API failed with status ${response.status}`);
      }
      return (await response.json()) as RosterView;
    },
    async mine(shareId) {
      const response = await fetchImpl(`${base(shareId)}/mine`, { cache: "no-store" });
      if (response.status === 401) {
        return { status: "signedOut" };
      }
      if (!response.ok) {
        throw new Error(`Own place API failed with status ${response.status}`);
      }
      return toPlace((await response.json()) as PlaceBody);
    },
    async join(shareId, team, guests) {
      const response = await fetchImpl(base(shareId), {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ team, guests }),
      });
      if (response.status === 201) {
        return { ok: true, place: toPlace((await response.json()) as PlaceBody) };
      }
      return { ok: false, error: await errorOf(response) };
    },
    async leave(shareId) {
      const response = await fetchImpl(`${base(shareId)}/mine`, { method: "DELETE" });
      if (response.ok) {
        return { ok: true };
      }
      return { ok: false, error: await errorOf(response) };
    },
  };
}
