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
};

type Options = {
  baseUrl?: string;
  fetchImpl?: typeof fetch;
};

export function apiBaseUrl(): string {
  return process.env.API_BASE_URL ?? "http://localhost:8080";
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
