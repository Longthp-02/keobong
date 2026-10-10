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
export type RosterView = { teams: TeamView[]; cancelled: boolean };

export type PaymentStatus = "awaiting_payment" | "payment_reported" | "confirmed";

/** Where to transfer; only sent to the player holding the place. */
export type PaymentInfo = {
  bankName: string;
  accountNumber: string;
  accountName: string;
  amountVnd: number;
  memo: string;
  qrPayload: string;
};

export type JoinedPlace = {
  status: "in";
  team: Team;
  guests: string[];
  paymentCode: number;
  paymentStatus: PaymentStatus;
  /** While awaiting payment: when the place is released. */
  holdExpiresAt: string | null;
  amountVnd: number;
  /** Present while a transfer is still expected. */
  payment: PaymentInfo | null;
};

export type MyPlace = { status: "signedOut" } | { status: "out" } | JoinedPlace;

/** One party as the host sees it. */
export type HostParty = {
  paymentCode: number;
  team: Team;
  holderName: string | null;
  guests: string[];
  amountVnd: number;
  paymentStatus: PaymentStatus;
  holdExpiresAt: string | null;
};

export type HostView = { status: "notHost" } | { status: "host"; parties: HostParty[] };
export type HostActionError =
  | "already_confirmed"
  | "party_not_found"
  | "not_reported"
  | "match_started"
  | "match_cancelled"
  | "not_host"
  | "unexpected";

export type SlotError =
  | "team_full"
  | "already_joined"
  | "match_started"
  | "guests"
  | "not_joined"
  | "match_cancelled"
  | "unauthenticated"
  | "unexpected";

export type JoinResult = { ok: true; place: MyPlace } | { ok: false; error: SlotError };
export type LeaveResult = { ok: true } | { ok: false; error: SlotError };

export type SlotsClient = {
  roster(shareId: string): Promise<RosterView>;
  mine(shareId: string): Promise<MyPlace>;
  join(shareId: string, team: Team, guests: string[]): Promise<JoinResult>;
  leave(shareId: string): Promise<LeaveResult>;
  reportPayment(shareId: string): Promise<JoinResult>;
  hostParties(shareId: string): Promise<HostView>;
  hostAction(
    shareId: string,
    paymentCode: number,
    action: "confirm" | "reject",
  ): Promise<{ ok: true } | { ok: false; error: HostActionError }>;
  cancelMatch(shareId: string): Promise<{ ok: true } | { ok: false; error: HostActionError }>;
};

/** A roster with nobody in it, split like the API: team A takes the odd place. */
export function emptyRoster(slotCount: number, cancelled = false): RosterView {
  return {
    teams: [
      { team: "a", capacity: Math.ceil(slotCount / 2), players: [] },
      { team: "b", capacity: Math.floor(slotCount / 2), players: [] },
    ],
    cancelled,
  };
}

type PlaceBody = { joined: false } | ({ joined: true } & Omit<JoinedPlace, "status">);

function toPlace(body: PlaceBody): MyPlace {
  if (!body.joined) {
    return { status: "out" };
  }
  const { joined: _joined, ...place } = body;
  return { status: "in", ...place };
}

const KNOWN_HOST_ERRORS: HostActionError[] = [
  "already_confirmed",
  "party_not_found",
  "not_reported",
  "match_started",
  "match_cancelled",
  "not_host",
];

const KNOWN_CONFLICTS: SlotError[] = ["team_full", "already_joined", "match_started", "not_joined", "match_cancelled"];

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
    async reportPayment(shareId) {
      const response = await fetchImpl(`${base(shareId)}/mine/report-payment`, { method: "POST" });
      if (response.ok) {
        return { ok: true, place: toPlace((await response.json()) as PlaceBody) };
      }
      return { ok: false, error: await errorOf(response) };
    },
    async hostParties(shareId) {
      const response = await fetchImpl(`/api/matches/${encodeURIComponent(shareId)}/payments`, {
        cache: "no-store",
      });
      if (response.status === 401 || response.status === 403) {
        return { status: "notHost" };
      }
      if (!response.ok) {
        throw new Error(`Payments API failed with status ${response.status}`);
      }
      const body = (await response.json()) as { parties: HostParty[] };
      return { status: "host", parties: body.parties };
    },
    async hostAction(shareId, paymentCode, action) {
      const response = await fetchImpl(
        `/api/matches/${encodeURIComponent(shareId)}/payments/${paymentCode}/${action}`,
        { method: "POST" },
      );
      if (response.ok) {
        return { ok: true };
      }
      const body = (await response.json().catch(() => ({}))) as { error?: string };
      const known: HostActionError[] = KNOWN_HOST_ERRORS;
      return {
        ok: false,
        error: known.includes(body.error as HostActionError) ? (body.error as HostActionError) : "unexpected",
      };
    },
    async cancelMatch(shareId) {
      const response = await fetchImpl(`/api/matches/${encodeURIComponent(shareId)}/cancel`, { method: "POST" });
      if (response.ok) {
        return { ok: true };
      }
      const body = (await response.json().catch(() => ({}))) as { error?: string };
      return {
        ok: false,
        error: KNOWN_HOST_ERRORS.includes(body.error as HostActionError) ? (body.error as HostActionError) : "unexpected",
      };
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
