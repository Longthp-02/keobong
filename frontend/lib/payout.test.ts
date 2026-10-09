import { describe, expect, it, vi } from "vitest";
import { canHostPaidMatches, savePayout, toAccountName } from "./payout";

function respond(status: number, body: unknown) {
  return vi.fn().mockResolvedValue(
    new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } }),
  );
}

describe("toAccountName", () => {
  it("writes the name the way banks show it: uppercase without diacritics", () => {
    expect(toAccountName("Phạm Trịnh Hoàng Long")).toBe("PHAM TRINH HOANG LONG");
    expect(toAccountName("Đặng thị Ánh")).toBe("DANG THI ANH");
  });

  it("drops digits and punctuation and collapses spaces", () => {
    expect(toAccountName("  Long  123 .,-  Phạm ")).toBe("LONG PHAM");
  });

  it("keeps a trailing space while the user is still typing", () => {
    expect(toAccountName("Long ", { typing: true })).toBe("LONG ");
  });
});

describe("savePayout", () => {
  const account = { bankBin: "970436", accountNumber: "0123456789", accountName: "PHAM LONG" };

  it("saves through the site's /api proxy", async () => {
    const fetchImpl = respond(200, account);

    expect(await savePayout(account, fetchImpl)).toEqual({ ok: true });
    const [url, init] = fetchImpl.mock.calls[0];
    expect(url).toBe("/api/me/payout");
    expect(init.method).toBe("PUT");
    expect(JSON.parse(init.body)).toEqual(account);
  });

  it("reports the rejected field", async () => {
    const fetchImpl = respond(422, { error: "invalid_payout_account", field: "accountNumber" });

    expect(await savePayout(account, fetchImpl)).toEqual({ ok: false, error: "accountNumber" });
  });

  it("reports an expired session and other failures", async () => {
    expect(await savePayout(account, respond(401, {}))).toEqual({ ok: false, error: "unauthenticated" });
    expect(await savePayout(account, respond(500, {}))).toEqual({ ok: false, error: "unexpected" });
  });
});

describe("canHostPaidMatches", () => {
  const banks = [{ bin: "970436", name: "Vietcombank" }];

  it("needs a payout account at a supported bank", () => {
    expect(canHostPaidMatches({ bankBin: "970436", accountNumber: "1", accountName: "A" }, banks)).toBe(true);
    expect(canHostPaidMatches({ bankBin: "999999", accountNumber: "1", accountName: "A" }, banks)).toBe(false);
    expect(canHostPaidMatches(null, banks)).toBe(false);
  });
});
