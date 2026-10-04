import { describe, expect, it } from "vitest";
import { apiRewrites } from "./rewrites";

describe("apiRewrites", () => {
  it("proxies /api to the backend so session cookies stay on the site's own domain", () => {
    expect(apiRewrites("https://api.example.run.app")).toEqual([
      { source: "/api/:path*", destination: "https://api.example.run.app/api/:path*" },
    ]);
  });

  it("ignores a trailing slash in the backend URL", () => {
    expect(apiRewrites("http://localhost:8080/")[0].destination).toBe("http://localhost:8080/api/:path*");
  });

  it("adds no proxy when no backend is configured", () => {
    expect(apiRewrites(undefined)).toEqual([]);
    expect(apiRewrites("")).toEqual([]);
  });
});
