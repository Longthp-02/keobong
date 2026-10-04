import { describe, expect, it, vi } from "vitest";
import { type CreateMatchInput, createMatch } from "./api";

const input: CreateMatchInput = {
  venueName: "SSA Sports Center",
  startsAt: "2099-10-10T11:30:00.000Z",
  endsAt: "2099-10-10T13:00:00.000Z",
  format: "seven_a_side",
  matchType: "casual",
  levelMin: 2.5,
  levelMax: 3.5,
  totalFeeVnd: 900000,
  slotCount: 18,
};

function respond(status: number, body: unknown) {
  return vi.fn().mockResolvedValue(
    new Response(JSON.stringify(body), {
      status,
      headers: { "content-type": "application/json" },
    }),
  );
}

describe("createMatch", () => {
  it("posts JSON and returns the created match", async () => {
    const created = { ...input, shareId: "k7Qm2xPa", pricePerPlayerVnd: 50000 };
    const fetchImpl = respond(201, created);

    const result = await createMatch(input, { baseUrl: "http://api", fetchImpl });

    expect(result).toEqual({ ok: true, match: created });
    const [url, init] = fetchImpl.mock.calls[0];
    expect(url).toBe("http://api/api/matches");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual(input);
  });

  it("returns the offending field on a validation error", async () => {
    const fetchImpl = respond(422, { error: "invalid_match", field: "startsAt" });

    await expect(createMatch(input, { baseUrl: "http://api", fetchImpl })).resolves.toEqual({
      ok: false,
      field: "startsAt",
    });
  });

  it("throws on any other failure instead of hiding it", async () => {
    const fetchImpl = respond(500, { error: "internal_error" });

    await expect(createMatch(input, { baseUrl: "http://api", fetchImpl })).rejects.toThrow(/500/);
  });
});
