import { describe, expect, it, vi } from "vitest";
import { browserOpenMatches } from "./api";
import { formatDistance, formatShortVnd, listDays } from "./matchList";
import messages from "../messages/vi.json";

const t = messages.list;

describe("listDays", () => {
  it("lists today and the next six days in Ho Chi Minh City", () => {
    // 23:30 UTC on 2 October is already 06:30 on Saturday 3 October in Ho Chi Minh City.
    const days = listDays(new Date("2099-10-02T23:30:00Z"));

    expect(days.map((d) => d.date)).toEqual([
      "2099-10-03",
      "2099-10-04",
      "2099-10-05",
      "2099-10-06",
      "2099-10-07",
      "2099-10-08",
      "2099-10-09",
    ]);
    expect(days[0]).toEqual({ date: "2099-10-03", label: t.today, sub: `${t.weekdayShort[6]} 3/10` });
    expect(days[1]).toEqual({ date: "2099-10-04", label: t.tomorrow, sub: `${t.weekdayShort[0]} 4/10` });
    expect(days[2]).toEqual({ date: "2099-10-05", label: t.weekday[1], sub: "5/10" });
  });
});

describe("formatDistance", () => {
  it("uses metres up close and kilometres with a decimal comma further away", () => {
    expect(formatDistance(8)).toBe("8 m");
    expect(formatDistance(950)).toBe("950 m");
    expect(formatDistance(1234)).toBe("1,2 km");
    expect(formatDistance(12_400)).toBe("12 km");
  });
});

describe("formatShortVnd", () => {
  it("shortens prices for the card badge", () => {
    expect(formatShortVnd(50_000)).toBe("50k");
    expect(formatShortVnd(65_500)).toBe("66k");
    expect(formatShortVnd(0)).toBe(t.free);
  });
});

describe("browserOpenMatches", () => {
  function respond(status: number, body: unknown) {
    return vi.fn().mockResolvedValue(new Response(JSON.stringify(body), { status }));
  }

  it("asks the site's API with the filters and a coarse position", async () => {
    const fetchImpl = respond(200, { matches: [], nextCursor: null });

    await browserOpenMatches(
      { date: "2099-10-03", type: "casual", near: { lat: 10.8069381, lng: 106.7388129 }, cursor: "1.abcdefgh" },
      fetchImpl,
    );

    const [url, init] = fetchImpl.mock.calls[0];
    // About 100 m of precision is enough for distances and reveals less.
    expect(url).toBe("/api/matches?date=2099-10-03&type=casual&lat=10.807&lng=106.739&cursor=1.abcdefgh");
    expect(init.cache).toBe("no-store");
  });

  it("leaves out filters that are not set", async () => {
    const fetchImpl = respond(200, { matches: [], nextCursor: null });

    await browserOpenMatches({ date: "2099-10-03" }, fetchImpl);

    expect(fetchImpl.mock.calls[0][0]).toBe("/api/matches?date=2099-10-03");
  });

  it("throws when the list cannot be loaded", async () => {
    await expect(browserOpenMatches({ date: "2099-10-03" }, respond(500, {}))).rejects.toThrow();
  });
});
