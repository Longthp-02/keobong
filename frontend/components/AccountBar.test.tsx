import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import messages from "../messages/vi.json";
import { AccountBar } from "./AccountBar";
import { SignInPrompt } from "./SignInPrompt";

const t = messages.auth;

describe("SignInPrompt", () => {
  it("links to Google sign-in and returns to the current page", () => {
    render(<SignInPrompt next="/create" />);

    const link = screen.getByRole("link", { name: t.signInWithGoogle });
    expect(link.getAttribute("href")).toBe("/api/auth/google/start?next=%2Fcreate");
  });
});

describe("AccountBar", () => {
  it("shows who is signed in and signs out with a POST", () => {
    render(<AccountBar me={{ displayName: "Long", avatarUrl: null }} />);

    expect(screen.getByText("Long")).toBeTruthy();
    const button = screen.getByRole("button", { name: t.signOut });
    const form = button.closest("form");
    expect(form?.getAttribute("method")).toBe("post");
    expect(form?.getAttribute("action")).toBe("/api/auth/logout");
  });

  it("falls back to a generic name when the account has none", () => {
    render(<AccountBar me={{ displayName: null, avatarUrl: null }} />);

    expect(screen.getByText(t.anonymousName)).toBeTruthy();
  });
});
