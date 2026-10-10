import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import messages from "../messages/vi.json";
import MatchNotFound from "./m/[shareId]/not-found";
import NotFound from "./not-found";

describe("NotFound", () => {
  it("says the page does not exist, not that a match is missing", () => {
    render(<NotFound />);

    expect(screen.getByRole("heading", { name: messages.app.notFoundTitle })).toBeTruthy();
    expect(screen.queryByText(messages.match.notFoundTitle)).toBeNull();
    expect(screen.getByRole("link", { name: messages.app.backHome }).getAttribute("href")).toBe("/");
  });
});

describe("MatchNotFound", () => {
  it("says the match does not exist", () => {
    render(<MatchNotFound />);

    expect(screen.getByRole("heading", { name: messages.match.notFoundTitle })).toBeTruthy();
    expect(screen.getByText(messages.match.notFoundBody)).toBeTruthy();
  });
});
