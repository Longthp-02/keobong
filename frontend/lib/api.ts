import type { Bank, PayoutAccount } from "./payout";
import type { RosterView } from "./slots";

/** Public match view returned by `GET /api/matches/{shareId}`. */
export type MatchView = {
  shareId: string;
  venueName: string;
  startsAt: string;
  endsAt: string;
  format: "five_a_side" | "seven_a_side" | "eleven_a_side";
  matchType: "casual" | "competitive" | "beginner_friendly";
  levelMin: number;
  levelMax: number;
  totalFeeVnd: number;
  slotCount: number;
  pricePerPlayerVnd: number;
  /** Set when the host cancelled the match. */
  cancelledAt: string | null;
};

type Options = {
  baseUrl?: string;
  fetchImpl?: typeof fetch;
  /** The browser's `Cookie` header, forwarded so the API can see the session. */
  cookie?: string;
};

/** The signed-in user's own view, from `GET /api/me`. */
export type MeView = {
  displayName: string | null;
  avatarUrl: string | null;
};

export const SESSION_COOKIE = "daghep_session";

/** `Cookie` header carrying only the session, so other browser cookies stay out of API calls. */
export function sessionCookieHeader(session: string | undefined): string {
  return session ? `${SESSION_COOKIE}=${session}` : "";
}

/** Where to send the browser to sign in with Google and come back to `next`. */
export function signInUrl(next: string): string {
  return `/api/auth/google/start?next=${encodeURIComponent(next)}`;
}

/** Returns the signed-in user for these browser cookies, or `null` when signed out. */
export async function getMe(cookie: string, options: Options = {}): Promise<MeView | null> {
  if (!cookie.split(";").some((pair) => pair.trim().startsWith(`${SESSION_COOKIE}=`))) {
    return null;
  }
  const baseUrl = options.baseUrl ?? apiBaseUrl();
  const fetchImpl = options.fetchImpl ?? fetch;

  const response = await fetchImpl(`${baseUrl}/api/me`, {
    headers: { accept: "application/json", cookie },
    cache: "no-store",
  });

  if (response.status === 401) {
    return null;
  }
  if (!response.ok) {
    throw new Error(`Me API failed with status ${response.status}`);
  }
  return (await response.json()) as MeView;
}

/** Server-side API location. Falls back to localhost only outside production. */
export function apiBaseUrl(): string {
  const configured = process.env.API_BASE_URL;
  if (configured) {
    return configured;
  }
  if (process.env.NODE_ENV === "production") {
    throw new Error("API_BASE_URL is not configured");
  }
  return "http://localhost:8080";
}

/** True when a backend is configured for this deployment (used to hide unfinished features). */
export function isApiConfigured(): boolean {
  return Boolean(process.env.API_BASE_URL) || process.env.NODE_ENV !== "production";
}

/** Returns the match, `null` when it does not exist, and throws on any other failure. */
export async function getMatch(shareId: string, options: Options = {}): Promise<MatchView | null> {
  const baseUrl = options.baseUrl ?? apiBaseUrl();
  const fetchImpl = options.fetchImpl ?? fetch;

  const response = await fetchImpl(`${baseUrl}/api/matches/${encodeURIComponent(shareId)}`, {
    headers: { accept: "application/json" },
    // Matches the page's ISR window (see app/m/[shareId]/page.tsx).
    next: { revalidate: 30 },
  } as RequestInit);

  if (response.status === 404) {
    return null;
  }
  if (!response.ok) {
    throw new Error(`Match API failed with status ${response.status}`);
  }
  return (await response.json()) as MatchView;
}

/** Public roster for the match page; `null` when the match does not exist. */
export async function getRoster(shareId: string, options: Options = {}): Promise<RosterView | null> {
  const baseUrl = options.baseUrl ?? apiBaseUrl();
  const fetchImpl = options.fetchImpl ?? fetch;

  const response = await fetchImpl(`${baseUrl}/api/matches/${encodeURIComponent(shareId)}/slots`, {
    headers: { accept: "application/json" },
    // Same ISR window as the match itself.
    next: { revalidate: 30 },
  } as RequestInit);

  if (response.status === 404) {
    return null;
  }
  if (!response.ok) {
    throw new Error(`Roster API failed with status ${response.status}`);
  }
  return (await response.json()) as RosterView;
}

/** The signed-in user's payout account, or `null` when there is none or nobody is signed in. */
export async function getPayout(cookie: string, options: Options = {}): Promise<PayoutAccount | null> {
  if (!cookie.split(";").some((pair) => pair.trim().startsWith(`${SESSION_COOKIE}=`))) {
    return null;
  }
  const baseUrl = options.baseUrl ?? apiBaseUrl();
  const fetchImpl = options.fetchImpl ?? fetch;

  const response = await fetchImpl(`${baseUrl}/api/me/payout`, {
    headers: { accept: "application/json", cookie },
    cache: "no-store",
  });
  if (response.status === 404 || response.status === 401) {
    return null;
  }
  if (!response.ok) {
    throw new Error(`Payout API failed with status ${response.status}`);
  }
  return (await response.json()) as PayoutAccount;
}

/** Banks that accept VietQR transfers. */
export async function getBanks(options: Options = {}): Promise<Bank[]> {
  const baseUrl = options.baseUrl ?? apiBaseUrl();
  const fetchImpl = options.fetchImpl ?? fetch;

  const response = await fetchImpl(`${baseUrl}/api/banks`, {
    headers: { accept: "application/json" },
    next: { revalidate: 86400 },
  } as RequestInit);
  if (!response.ok) {
    throw new Error(`Banks API failed with status ${response.status}`);
  }
  return ((await response.json()) as { banks: Bank[] }).banks;
}

export type CreateMatchInput = Omit<MatchView, "shareId" | "pricePerPlayerVnd" | "cancelledAt">;

export type CreateMatchResult = { ok: true; match: MatchView } | { ok: false; field: string };

/**
 * Creates a match as the signed-in user. Returns the offending field on a 422,
 * `unauthenticated` on a 401, and throws on any other failure.
 */
export async function createMatch(
  input: CreateMatchInput,
  options: Options = {},
): Promise<CreateMatchResult> {
  const baseUrl = options.baseUrl ?? apiBaseUrl();
  const fetchImpl = options.fetchImpl ?? fetch;

  const response = await fetchImpl(`${baseUrl}/api/matches`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      accept: "application/json",
      ...(options.cookie ? { cookie: options.cookie } : {}),
    },
    body: JSON.stringify(input),
    cache: "no-store",
  });

  if (response.status === 401) {
    return { ok: false, field: "unauthenticated" };
  }
  if (response.status === 422) {
    const body = (await response.json()) as { field?: string };
    return { ok: false, field: body.field ?? "unknown" };
  }
  if (!response.ok) {
    throw new Error(`Create match API failed with status ${response.status}`);
  }
  return { ok: true, match: (await response.json()) as MatchView };
}
