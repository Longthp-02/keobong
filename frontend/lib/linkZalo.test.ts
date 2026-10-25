import { describe, expect, it, vi } from "vitest";
import { browserLinkZalo } from "./api";

function respond(status: number) {
  return vi.fn().mockResolvedValue(new Response("{}", { status, headers: { "content-type": "application/json" } }));
}

describe("browserLinkZalo", () => {
  it("posts the code", async () => {
    const fetchImpl = respond(200);
    await expect(browserLinkZalo("abc", fetchImpl)).resolves.toBe("linked");
    expect(fetchImpl).toHaveBeenCalledWith(
      "/api/bot/link",
      expect.objectContaining({ method: "POST", body: JSON.stringify({ code: "abc" }) }),
    );
  });

  it.each([
    [401, "signed_out"],
    [410, "invalid"],
    [400, "invalid"],
    [500, "failed"],
    [404, "failed"],
  ])("maps status %i to %s", async (status, result) => {
    await expect(browserLinkZalo("abc", respond(status))).resolves.toBe(result);
  });

  it("treats a network failure as failed", async () => {
    await expect(browserLinkZalo("abc", vi.fn().mockRejectedValue(new TypeError("offline")))).resolves.toBe("failed");
  });
});
