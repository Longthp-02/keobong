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

  it("tells hosts that a pasted message is sent to Anthropic to be read and is not stored", () => {
    const text = JSON.stringify(messages.legal.privacy);

    expect(text).toContain("Anthropic");
    expect(text).toContain("Chúng tôi không lưu nội dung này");
  });

  it("tells organisers what the Zalo bot stores and reads", () => {
    const text = JSON.stringify(messages.legal.privacy);

    expect(text).toContain("Bot DaGhep");
    expect(text).toContain("mã tài khoản Zalo");
  });

  it("tells organisers that reminder texts are stored until turned off", () => {
    const text = JSON.stringify(messages.legal.privacy);

    expect(text).toContain("lịch nhắc");
    expect(text).toContain("cho tới khi lịch nhắc bị tắt");
  });

  it("tells players that poll answers show their name to anyone with the link", () => {
    const text = JSON.stringify(messages.legal.privacy);

    expect(text).toContain("bảng điểm danh");
    expect(text).toContain("ai có link bảng đều xem được");
  });
});
