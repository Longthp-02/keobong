import { describe, expect, it } from "vitest";
import { fill } from "./text";

describe("fill", () => {
  it("replaces named placeholders", () => {
    expect(fill("Còn {count} chỗ ở {team}", { count: 3, team: "Đội A" })).toBe("Còn 3 chỗ ở Đội A");
  });

  it("leaves unknown placeholders as they are", () => {
    expect(fill("{a} {b}", { a: "x" })).toBe("x {b}");
  });
});
