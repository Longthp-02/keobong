import { describe, expect, it, vi } from "vitest";
import { getMatch, type MatchView } from "./api";

const sample: MatchView = {
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

function respond(status: number, body: unknown) {
  return vi.fn().mockResolvedValue(
    new Response(JSON.stringify(body), {
      status,
      headers: { "content-type": "application/json" },
    }),
  );
}

describe("getMatch", () => {
  it("returns the match when the API answers 200", async () => {
    const fetchImpl = respond(200, sample);

    await expect(getMatch("k7Qm2xPa", { baseUrl: "http://api", fetchImpl })).resolves.toEqual(
      sample,
    );
    expect(fetchImpl).toHaveBeenCalledWith("http://api/api/matches/k7Qm2xPa", expect.anything());
  });

  it("returns null when the match does not exist", async () => {
    const fetchImpl = respond(404, { error: "match_not_found" });

    await expect(getMatch("missing01", { baseUrl: "http://api", fetchImpl })).resolves.toBeNull();
  });

  it("throws on unexpected API errors instead of hiding them", async () => {
    const fetchImpl = respond(500, { error: "internal_error" });

    await expect(getMatch("k7Qm2xPa", { baseUrl: "http://api", fetchImpl })).rejects.toThrow(
      /500/,
    );
  });

  it("encodes the share id so it cannot change the request path", async () => {
    const fetchImpl = respond(404, { error: "match_not_found" });

    await getMatch("../admin", { baseUrl: "http://api", fetchImpl });

    expect(fetchImpl).toHaveBeenCalledWith(
      "http://api/api/matches/..%2Fadmin",
      expect.anything(),
    );
  });
});
