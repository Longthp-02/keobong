import { afterEach, describe, expect, it, vi } from "vitest";
import { apiBaseUrl, isApiConfigured } from "./api";

afterEach(() => {
  vi.unstubAllEnvs();
});

describe("apiBaseUrl", () => {
  it("uses API_BASE_URL when set", () => {
    vi.stubEnv("API_BASE_URL", "https://api.daghep.vn");
    expect(apiBaseUrl()).toBe("https://api.daghep.vn");
  });

  it("falls back to localhost in development", () => {
    vi.stubEnv("API_BASE_URL", "");
    vi.stubEnv("NODE_ENV", "development");
    expect(apiBaseUrl()).toBe("http://localhost:8080");
  });

  it("fails loudly in production when API_BASE_URL is missing", () => {
    vi.stubEnv("API_BASE_URL", "");
    vi.stubEnv("NODE_ENV", "production");
    expect(() => apiBaseUrl()).toThrow(/API_BASE_URL/);
  });
});

describe("isApiConfigured", () => {
  it("is false in production without API_BASE_URL so match creation stays hidden", () => {
    vi.stubEnv("API_BASE_URL", "");
    vi.stubEnv("NODE_ENV", "production");
    expect(isApiConfigured()).toBe(false);
  });

  it("is true in production once API_BASE_URL is set", () => {
    vi.stubEnv("API_BASE_URL", "https://api.daghep.vn");
    vi.stubEnv("NODE_ENV", "production");
    expect(isApiConfigured()).toBe(true);
  });

  it("is true in development", () => {
    vi.stubEnv("API_BASE_URL", "");
    vi.stubEnv("NODE_ENV", "development");
    expect(isApiConfigured()).toBe(true);
  });
});
