import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { browserMe } from "../lib/api";
import messages from "../messages/vi.json";
import { BrowserAccountBar } from "./BrowserAccountBar";

const t = messages.auth;

function respond(status: number, body: unknown) {
  return vi.fn().mockResolvedValue(
    new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }),
  );
}

describe("browserMe", () => {
  it("asks the site's own API without caching", async () => {
    const fetchImpl = respond(200, { displayName: "Long", avatarUrl: null });

    expect(await browserMe(fetchImpl)).toEqual({ displayName: "Long", avatarUrl: null });
    const [url, init] = fetchImpl.mock.calls[0];
    expect(url).toBe("/api/me");
    expect(init.cache).toBe("no-store");
  });

  it("returns null when signed out", async () => {
    expect(await browserMe(respond(401, { error: "unauthenticated" }))).toBeNull();
  });

  it("throws on other failures", async () => {
    await expect(browserMe(respond(500, {}))).rejects.toThrow();
  });
});

describe("BrowserAccountBar", () => {
  it("shows the signed-in user with a sign-out button", async () => {
    render(<BrowserAccountBar loadMe={vi.fn().mockResolvedValue({ displayName: "Long", avatarUrl: null })} />);

    expect(await screen.findByText("Long")).toBeTruthy();
    expect(screen.getByRole("button", { name: t.signOut })).toBeTruthy();
  });

  it("shows nothing when signed out", async () => {
    const loadMe = vi.fn().mockResolvedValue(null);
    const { container } = render(<BrowserAccountBar loadMe={loadMe} />);

    await waitFor(() => expect(loadMe).toHaveBeenCalled());
    expect(container.innerHTML).toBe("");
  });

  it("shows nothing when the API fails", async () => {
    const loadMe = vi.fn().mockRejectedValue(new Error("down"));
    const { container } = render(<BrowserAccountBar loadMe={loadMe} />);

    await waitFor(() => expect(loadMe).toHaveBeenCalled());
    expect(container.innerHTML).toBe("");
  });
});
