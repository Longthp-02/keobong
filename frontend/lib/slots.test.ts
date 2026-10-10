import { describe, expect, it, vi } from "vitest";
import { emptyRoster, slotsClient } from "./slots";

function respond(status: number, body?: unknown) {
  return vi.fn().mockResolvedValue(
    new Response(body === undefined ? null : JSON.stringify(body), {
      status,
      headers: { "content-type": "application/json" },
    }),
  );
}

const payment = {
  bankName: "ACB",
  accountNumber: "257678859",
  accountName: "PHAM LONG",
  amountVnd: 100000,
  memo: "DG k7Qm2xPa 7",
  qrPayload: "000201...",
};
const joinedBody = {
  joined: true,
  team: "b",
  guests: ["An"],
  paymentCode: 7,
  paymentStatus: "awaiting_payment",
  holdExpiresAt: "2099-10-01T00:30:00Z",
  amountVnd: 100000,
  payment,
};
const joinedPlace = {
  status: "in",
  team: "b",
  guests: ["An"],
  paymentCode: 7,
  paymentStatus: "awaiting_payment",
  holdExpiresAt: "2099-10-01T00:30:00Z",
  amountVnd: 100000,
  payment,
};

const roster = { teams: [{ team: "a", capacity: 9, players: [] }, { team: "b", capacity: 9, players: [] }] };

describe("emptyRoster", () => {
  it("can start out cancelled when the match says so", () => {
    expect(emptyRoster(4, true).cancelled).toBe(true);
  });

  it("splits places like the API, with team A taking the odd one", () => {
    expect(emptyRoster(15)).toEqual({
      teams: [
        { team: "a", capacity: 8, players: [] },
        { team: "b", capacity: 7, players: [] },
      ],
      cancelled: false,
    });
  });
});

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
    expect(await slotsClient(respond(200, joinedBody)).mine("k7Qm2xPa")).toEqual(joinedPlace);
  });

  it("joins with a team and guest names", async () => {
    const fetchImpl = respond(201, joinedBody);

    const result = await slotsClient(fetchImpl).join("k7Qm2xPa", "b", ["An"]);

    expect(result).toEqual({ ok: true, place: joinedPlace });
    const [url, init] = fetchImpl.mock.calls[0];
    expect(url).toBe("/api/matches/k7Qm2xPa/slots");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ team: "b", guests: ["An"] });
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

  it("reports a transfer and returns the updated place", async () => {
    const fetchImpl = respond(200, { ...joinedBody, paymentStatus: "payment_reported", holdExpiresAt: null });

    const result = await slotsClient(fetchImpl).reportPayment("k7Qm2xPa");

    expect(result).toEqual({
      ok: true,
      place: { ...joinedPlace, paymentStatus: "payment_reported", holdExpiresAt: null },
    });
    const [url, init] = fetchImpl.mock.calls[0];
    expect(url).toBe("/api/matches/k7Qm2xPa/slots/mine/report-payment");
    expect(init.method).toBe("POST");
  });

  it("loads the host's payment list, or says the viewer is not the host", async () => {
    const parties = [{ paymentCode: 7, team: "a", holderName: "Long", guests: [], amountVnd: 50000, paymentStatus: "confirmed", holdExpiresAt: null }];

    expect(await slotsClient(respond(200, { parties })).hostParties("k7Qm2xPa")).toEqual({ status: "host", parties });
    expect(await slotsClient(respond(403, { error: "not_host" })).hostParties("k7Qm2xPa")).toEqual({ status: "notHost" });
    expect(await slotsClient(respond(401, { error: "unauthenticated" })).hostParties("k7Qm2xPa")).toEqual({
      status: "notHost",
    });
  });

  it("confirms or rejects a party as the host", async () => {
    const fetchImpl = respond(204);

    expect(await slotsClient(fetchImpl).hostAction("k7Qm2xPa", 7, "confirm")).toEqual({ ok: true });
    expect(fetchImpl.mock.calls[0][0]).toBe("/api/matches/k7Qm2xPa/payments/7/confirm");
    expect(fetchImpl.mock.calls[0][1].method).toBe("POST");
    expect(await slotsClient(respond(409, { error: "already_confirmed" })).hostAction("x", 7, "reject")).toEqual({
      ok: false,
      error: "already_confirmed",
    });
  });

  it("cancels a match as the host", async () => {
    const fetchImpl = respond(204);

    expect(await slotsClient(fetchImpl).cancelMatch("k7Qm2xPa")).toEqual({ ok: true });
    expect(fetchImpl.mock.calls[0][0]).toBe("/api/matches/k7Qm2xPa/cancel");
    expect(fetchImpl.mock.calls[0][1].method).toBe("POST");
    expect(await slotsClient(respond(409, { error: "match_started" })).cancelMatch("x")).toEqual({
      ok: false,
      error: "match_started",
    });
  });
});
