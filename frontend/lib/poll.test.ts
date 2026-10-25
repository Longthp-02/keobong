import { describe, expect, it, vi } from "vitest";
import { type PollView, browserAnswerPoll, getPoll } from "./api";

const poll: PollView = {
  title: "Ai chơi bóng rổ cuối tuần này?",
  days: ["2099-10-03", "2099-10-04"],
  closesAt: "2099-10-04T17:00:00+00:00",
  closed: false,
  counts: [
    { day: "2099-10-03", yes: 1 },
    { day: "2099-10-04", yes: 0 },
  ],
  players: [{ name: "Nam", answers: { "2099-10-03": true } }],
  mine: null,
};

function respond(status: number, body: unknown = {}) {
  return vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
}

describe("getPoll", () => {
  it("forwards the session and is never cached, since it shows the viewer's answers", async () => {
    const fetchImpl = respond(200, poll);
    await expect(
      getPoll("abcDEF12", { baseUrl: "http://api", fetchImpl, cookie: "daghep_session=s" }),
    ).resolves.toEqual(poll);
    expect(fetchImpl).toHaveBeenCalledWith(
      "http://api/api/polls/abcDEF12",
      expect.objectContaining({ cache: "no-store", headers: expect.objectContaining({ cookie: "daghep_session=s" }) }),
    );
  });

  it("returns null for an unknown poll and throws on other failures", async () => {
    await expect(getPoll("x", { baseUrl: "http://api", fetchImpl: respond(404) })).resolves.toBeNull();
    await expect(getPoll("x", { baseUrl: "http://api", fetchImpl: respond(500) })).rejects.toThrow();
  });
});

describe("browserAnswerPoll", () => {
  it("puts the answers and returns the updated poll", async () => {
    const fetchImpl = respond(200, poll);
    const answers = [{ day: "2099-10-03", attending: true }];
    await expect(browserAnswerPoll("abcDEF12", answers, fetchImpl)).resolves.toEqual({ ok: true, poll });
    expect(fetchImpl).toHaveBeenCalledWith(
      "/api/polls/abcDEF12/answers",
      expect.objectContaining({ method: "PUT", body: JSON.stringify({ answers }) }),
    );
  });

  it.each([
    [401, "signed_out"],
    [409, "closed"],
    [400, "failed"],
    [500, "failed"],
  ])("maps status %i to %s", async (status, error) => {
    await expect(browserAnswerPoll("x", [], respond(status))).resolves.toEqual({ ok: false, error });
  });

  it("treats a network failure as failed", async () => {
    await expect(browserAnswerPoll("x", [], vi.fn().mockRejectedValue(new TypeError("offline")))).resolves.toEqual({
      ok: false,
      error: "failed",
    });
  });
});
