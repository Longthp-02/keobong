import { describe, expect, it, vi } from "vitest";
import { browserReadDraft, type MatchDraft } from "./api";

const draft: MatchDraft = {
  venueId: "ssa-amitie",
  date: "2099-10-01",
  startTime: "19:00",
  endTime: null,
  format: "seven_a_side",
  matchType: null,
  levelMin: null,
  levelMax: null,
  slotCount: 3,
  totalFeeVnd: 180000,
};

function respond(status: number, body: unknown) {
  return vi
    .fn()
    .mockResolvedValue(new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }));
}

describe("browserReadDraft", () => {
  it("posts the message and returns the draft", async () => {
    const fetchImpl = respond(200, { draft });

    await expect(browserReadDraft("tối nay 19h SSA", fetchImpl)).resolves.toEqual({ ok: true, draft });
    expect(fetchImpl).toHaveBeenCalledWith(
      "/api/match-drafts",
      expect.objectContaining({ method: "POST", body: JSON.stringify({ text: "tối nay 19h SSA" }) }),
    );
  });

  it.each([
    [401, { error: "unauthenticated" }, "signed_out"],
    [422, { error: "invalid_text" }, "invalid_text"],
    [429, { error: "draft_limit", scope: "user" }, "limit_user"],
    [429, { error: "draft_limit", scope: "service" }, "limit_service"],
    [503, { error: "drafts_unavailable" }, "unavailable"],
    [502, { error: "reader_failed" }, "failed"],
  ])("maps status %i %j to %s", async (status, body, error) => {
    await expect(browserReadDraft("kèo", respond(status, body))).resolves.toEqual({ ok: false, error });
  });

  it("treats a network failure as a failed read", async () => {
    const fetchImpl = vi.fn().mockRejectedValue(new TypeError("offline"));
    await expect(browserReadDraft("kèo", fetchImpl)).resolves.toEqual({ ok: false, error: "failed" });
  });
});
