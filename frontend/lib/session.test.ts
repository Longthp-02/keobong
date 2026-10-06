import { describe, expect, it, vi } from "vitest";
import { getBanks, getMe, getPayout, sessionCookieHeader, signInUrl } from "./api";

function respond(status: number, body: unknown) {
  return vi.fn().mockResolvedValue(
    new Response(JSON.stringify(body), {
      status,
      headers: { "content-type": "application/json" },
    }),
  );
}

describe("getMe", () => {
  it("forwards the browser's cookies and returns the signed-in user", async () => {
    const fetchImpl = respond(200, { displayName: "Long", avatarUrl: null });

    const me = await getMe("daghep_session=abc", { baseUrl: "http://api", fetchImpl });

    expect(me).toEqual({ displayName: "Long", avatarUrl: null });
    const [url, init] = fetchImpl.mock.calls[0];
    expect(url).toBe("http://api/api/me");
    expect(init.headers.cookie).toBe("daghep_session=abc");
    expect(init.cache).toBe("no-store");
  });

  it("returns null when not signed in", async () => {
    const fetchImpl = respond(401, { error: "unauthenticated" });

    expect(await getMe("", { baseUrl: "http://api", fetchImpl })).toBeNull();
  });

  it("does not call the API when there is no session cookie", async () => {
    const fetchImpl = vi.fn();

    expect(await getMe("other=1", { baseUrl: "http://api", fetchImpl })).toBeNull();
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("throws on other failures", async () => {
    const fetchImpl = respond(500, { error: "internal_error" });

    await expect(getMe("daghep_session=abc", { baseUrl: "http://api", fetchImpl })).rejects.toThrow(
      /500/,
    );
  });
});

describe("sessionCookieHeader", () => {
  it("forwards only the session cookie to the API", () => {
    expect(sessionCookieHeader("abc")).toBe("daghep_session=abc");
    expect(sessionCookieHeader(undefined)).toBe("");
  });
});

describe("signInUrl", () => {
  it("starts Google sign-in and comes back to the given page", () => {
    expect(signInUrl("/create")).toBe("/api/auth/google/start?next=%2Fcreate");
  });
});

describe("getPayout", () => {
  it("returns the signed-in user's payout account, or null when there is none", async () => {
    const account = { bankBin: "970436", accountNumber: "0123456789", accountName: "PHAM LONG" };
    const found = respond(200, account);

    expect(await getPayout("daghep_session=abc", { baseUrl: "http://api", fetchImpl: found })).toEqual(account);
    expect(found.mock.calls[0][0]).toBe("http://api/api/me/payout");
    expect(found.mock.calls[0][1].headers.cookie).toBe("daghep_session=abc");
    expect(found.mock.calls[0][1].cache).toBe("no-store");
    expect(await getPayout("daghep_session=abc", { baseUrl: "http://api", fetchImpl: respond(404, {}) })).toBeNull();
    expect(await getPayout("", { baseUrl: "http://api", fetchImpl: vi.fn() })).toBeNull();
  });
});

describe("getBanks", () => {
  it("returns the bank list", async () => {
    const banks = [{ bin: "970436", name: "Vietcombank" }];

    expect(await getBanks({ baseUrl: "http://api", fetchImpl: respond(200, { banks }) })).toEqual(banks);
  });
});
