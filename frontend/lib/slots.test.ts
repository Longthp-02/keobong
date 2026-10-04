import { describe, expect, it, vi } from "vitest";
import { slotsClient } from "./slots";

function respond(status: number, body?: unknown) {
  return vi.fn().mockResolvedValue(
    new Response(body === undefined ? null : JSON.stringify(body), {
      status,
      headers: { "content-type": "application/json" },
    }),
  );
}

const roster = { teams: [{ team: "a", capacity: 9, players: [] }, { team: "b", capacity: 9, players: [] }] };

describe("slotsClient", () => {
  it("reads a fresh roster through the site's own /api proxy", async () => {
    const fetchImpl = respond(200, roster);

    expect(await slotsClient(fetchImpl).roster("k7Qm2xPa")).toEqual(roster);
    const [url, init] = fetchImpl.mock.calls[0];
    expect(url).toBe("/api/matches/k7Qm2xPa/slots");
    expect(init.cache).toBe("no-store");
  });

  it("reports signed out, not joined and joined", async () => {
    expect(await slotsClient(respond(401, { error: "unauthenticated" })).mine("k7Qm2xPa")).toEqual({
      status: "signedOut",
    });
    expect(await slotsClient(respond(200, { joined: false })).mine("k7Qm2xPa")).toEqual({ status: "out" });
    expect(
      await slotsClient(respond(200, { joined: true, team: "b", guests: ["An"] })).mine("k7Qm2xPa"),
    ).toEqual({ status: "in", team: "b", guests: ["An"] });
  });

  it("joins with a team and guest names", async () => {
    const fetchImpl = respond(201, { joined: true, team: "a", guests: ["An"] });

    const result = await slotsClient(fetchImpl).join("k7Qm2xPa", "a", ["An"]);

    expect(result).toEqual({ ok: true, place: { status: "in", team: "a", guests: ["An"] } });
    const [url, init] = fetchImpl.mock.calls[0];
    expect(url).toBe("/api/matches/k7Qm2xPa/slots");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ team: "a", guests: ["An"] });
  });

  it("maps refusals to error codes", async () => {
    const cases: [number, unknown, string][] = [
      [409, { error: "team_full" }, "team_full"],
      [409, { error: "already_joined" }, "already_joined"],
      [409, { error: "match_started" }, "match_started"],
      [422, { error: "invalid_slot_request", field: "guests" }, "guests"],
      [401, { error: "unauthenticated" }, "unauthenticated"],
      [500, { error: "internal_error" }, "unexpected"],
    ];
    for (const [status, body, code] of cases) {
      expect(await slotsClient(respond(status, body)).join("k7Qm2xPa", "a", [])).toEqual({
        ok: false,
        error: code,
      });
    }
  });

  it("leaves with a DELETE", async () => {
    const fetchImpl = respond(204);

    expect(await slotsClient(fetchImpl).leave("k7Qm2xPa")).toEqual({ ok: true });
    const [url, init] = fetchImpl.mock.calls[0];
    expect(url).toBe("/api/matches/k7Qm2xPa/slots/mine");
    expect(init.method).toBe("DELETE");
    expect(await slotsClient(respond(409, { error: "match_started" })).leave("x")).toEqual({
      ok: false,
      error: "match_started",
    });
  });
});
