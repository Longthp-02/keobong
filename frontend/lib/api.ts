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
};

type Options = {
  baseUrl?: string;
  fetchImpl?: typeof fetch;
};

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

export type CreateMatchInput = Omit<MatchView, "shareId" | "pricePerPlayerVnd">;

export type CreateMatchResult = { ok: true; match: MatchView } | { ok: false; field: string };

/** Creates a match; returns the offending field on a 422 and throws on any other failure. */
export async function createMatch(
  input: CreateMatchInput,
  options: Options = {},
): Promise<CreateMatchResult> {
  const baseUrl = options.baseUrl ?? apiBaseUrl();
  const fetchImpl = options.fetchImpl ?? fetch;

  const response = await fetchImpl(`${baseUrl}/api/matches`, {
    method: "POST",
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify(input),
    cache: "no-store",
  });

  if (response.status === 422) {
    const body = (await response.json()) as { field?: string };
    return { ok: false, field: body.field ?? "unknown" };
  }
  if (!response.ok) {
    throw new Error(`Create match API failed with status ${response.status}`);
  }
  return { ok: true, match: (await response.json()) as MatchView };
}
