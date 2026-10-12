import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import messages from "../messages/vi.json";
import RootLayout from "./layout";

// The real component only injects its script in production builds, so a marker
// stands in for it to show that every page includes the page-view counter.
vi.mock("@vercel/analytics/next", () => ({
  Analytics: () => <span data-testid="page-view-analytics" />,
}));

describe("RootLayout", () => {
  it("counts page views on every page", () => {
    const html = renderToStaticMarkup(
      <RootLayout>
        <p>content</p>
      </RootLayout>,
    );

    expect(html).toContain('data-testid="page-view-analytics"');
  });
});

describe("privacy policy", () => {
  it("tells visitors that anonymous page-view statistics are collected", () => {
    const text = JSON.stringify(messages.legal.privacy);

    expect(text).toContain("Vercel Web Analytics");
  });
});
