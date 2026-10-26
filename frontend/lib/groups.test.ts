import { describe, expect, it, vi } from "vitest";
import { type GroupView, type PublicGroup, browserJoinGroup, getGroup, getGroups } from "./api";

const group: PublicGroup = {
  id: "0123456789ab",
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
      expect.objectContaining({ next: { revalidate: 60 }, signal: expect.any(AbortSignal) }),
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

describe("getGroup", () => {
  const view: GroupView = { ...group, membership: { status: "none" } };

  it("forwards the session and is never cached, since it shows the viewer's standing", async () => {
    const fetchImpl = respond(200, view);
    await expect(
      getGroup("0123456789ab", { baseUrl: "http://api", fetchImpl, cookie: "daghep_session=s" }),
    ).resolves.toEqual(view);
    expect(fetchImpl).toHaveBeenCalledWith(
      "http://api/api/groups/0123456789ab",
      expect.objectContaining({ cache: "no-store", headers: expect.objectContaining({ cookie: "daghep_session=s" }) }),
    );
  });

  it("returns null for an unknown group and throws on other errors", async () => {
    await expect(getGroup("x", { baseUrl: "http://api", fetchImpl: respond(404) })).resolves.toBeNull();
    await expect(getGroup("x", { baseUrl: "http://api", fetchImpl: respond(500) })).rejects.toThrow();
  });
});

describe("browserJoinGroup", () => {
  it("returns the new standing", async () => {
    const fetchImpl = respond(200, { membership: { status: "pending" } });
    await expect(browserJoinGroup("0123456789ab", fetchImpl)).resolves.toEqual({
      ok: true,
      membership: { status: "pending" },
    });
    expect(fetchImpl).toHaveBeenCalledWith("/api/groups/0123456789ab/join", expect.objectContaining({ method: "POST" }));
  });

  it("names each refusal", async () => {
    const cases: [number, unknown, string][] = [
      [401, {}, "signed_out"],
      [409, { error: "too_soon", askAgainAt: "2099-10-08T00:00:00Z" }, "too_soon"],
      [409, { error: "group_busy" }, "group_busy"],
      [429, { error: "too_many_requests" }, "too_many"],
      [500, {}, "failed"],
    ];
    for (const [status, body, error] of cases) {
      await expect(browserJoinGroup("0123456789ab", respond(status, body))).resolves.toMatchObject({ ok: false, error });
    }
    await expect(browserJoinGroup("0123456789ab", vi.fn().mockRejectedValue(new TypeError("offline")))).resolves.toEqual({
      ok: false,
      error: "failed",
    });
  });
});
