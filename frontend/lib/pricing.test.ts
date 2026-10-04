import { describe, expect, it } from "vitest";
import { defaultSlotCount, pricePerPlayerVnd } from "./pricing";

describe("pricing rules (must match the backend)", () => {
  it("defaults include substitutes", () => {
    expect(defaultSlotCount("five_a_side")).toBe(14);
    expect(defaultSlotCount("seven_a_side")).toBe(18);
    expect(defaultSlotCount("eleven_a_side")).toBe(28);
  });

  it("rounds the price per player up to the next 1,000 VND", () => {
    expect(pricePerPlayerVnd(900_000, 18)).toBe(50_000);
    expect(pricePerPlayerVnd(900_000, 14)).toBe(65_000);
    expect(pricePerPlayerVnd(1, 10)).toBe(1_000);
    expect(pricePerPlayerVnd(0, 10)).toBe(0);
  });

  it("returns null when the inputs cannot produce a price", () => {
    expect(pricePerPlayerVnd(Number.NaN, 18)).toBeNull();
    expect(pricePerPlayerVnd(900_000, 0)).toBeNull();
    expect(pricePerPlayerVnd(-5, 10)).toBeNull();
  });
});
