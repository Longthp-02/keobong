import { describe, expect, it, vi } from "vitest";
import { type PublicGroup, getGroups } from "./api";

const group: PublicGroup = {
  title: "Ai chơi bóng rổ cuối tuần này?",
  sport: "basketball",
  format: "3x3",
  venue: { name: "Sân Lakeview", address: "Sân bóng rổ Lakeview" },
  weekdays: [0, 6],
  playTime: "18:00",
  week: { players: 8, yes: [{ day: "2099-10-03", yes: 6 }] },
};

function respond(status: number, body: unknown = {}) {
  return vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
}

describe("getGroups", () => {
  it("reads the public groups, refreshed every minute", async () => {
    const fetchImpl = respond(200, { groups: [group] });
    await expect(getGroups({ baseUrl: "http://api", fetchImpl })).resolves.toEqual([group]);
    expect(fetchImpl).toHaveBeenCalledWith(
      "http://api/api/groups",
      expect.objectContaining({ next: { revalidate: 60 } }),
    );
  });

  it("shows no groups rather than failing the home page", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    await expect(getGroups({ baseUrl: "http://api", fetchImpl: respond(500) })).resolves.toEqual([]);
    await expect(
      getGroups({ baseUrl: "http://api", fetchImpl: vi.fn().mockRejectedValue(new TypeError("offline")) }),
    ).resolves.toEqual([]);
    expect(error).toHaveBeenCalled();
    error.mockRestore();
  });
});
