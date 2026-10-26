import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import messages from "../messages/vi.json";
import { GroupJoin } from "./GroupJoin";

const t = messages.groups.join;
const ID = "0123456789ab";

describe("GroupJoin", () => {
  it("asks a signed-out visitor to sign in first", () => {
    render(<GroupJoin groupId={ID} initial={null} />);

    const link = screen.getByRole("link", { name: messages.auth.signInWithGoogle });
    expect(link.getAttribute("href")).toContain(encodeURIComponent(`/g/${ID}`));
    expect(screen.getByText(t.signInFirst)).toBeTruthy();
  });

  it("sends a request and says the organiser will decide in Zalo", async () => {
    const join = vi.fn().mockResolvedValue({ ok: true, membership: { status: "pending" } });
    render(<GroupJoin groupId={ID} initial={{ status: "none" }} join={join} />);

    fireEvent.click(screen.getByRole("button", { name: t.ask }));
    await waitFor(() => expect(screen.getByText(t.pending)).toBeTruthy());
    expect(screen.getByRole("status").textContent).toBe(t.sent);
    expect(join).toHaveBeenCalledWith(ID);
    expect(screen.queryByRole("button", { name: t.ask })).toBeNull();
  });

  it("shows a member the poll and the Zalo group", () => {
    render(
      <GroupJoin
        groupId={ID}
        initial={{ status: "member", groupLink: "https://zalo.me/g/abc123", pollPath: "/p/abcDEF12" }}
      />,
    );

    expect(screen.getByText(t.member)).toBeTruthy();
    expect(screen.getByRole("link", { name: t.openPoll }).getAttribute("href")).toBe("/p/abcDEF12");
    expect(screen.getByRole("link", { name: t.openZalo }).getAttribute("href")).toBe("https://zalo.me/g/abc123");
  });

  it("leaves out links a member does not have yet", () => {
    render(<GroupJoin groupId={ID} initial={{ status: "member", groupLink: null, pollPath: null }} />);

    expect(screen.queryByRole("link", { name: t.openPoll })).toBeNull();
    expect(screen.queryByRole("link", { name: t.openZalo })).toBeNull();
    expect(screen.getByText(t.noZaloLink)).toBeTruthy();
  });

  it("tells a declined player when they may ask again", () => {
    render(<GroupJoin groupId={ID} initial={{ status: "declined", askAgainAt: "2099-10-08T03:00:00Z" }} />);

    expect(screen.getByText(t.declined.replace("{date}", "08/10"))).toBeTruthy();
    expect(screen.queryByRole("button", { name: t.ask })).toBeNull();
  });

  it("offers to sign in again when the session ended", async () => {
    const join = vi.fn().mockResolvedValue({ ok: false, error: "signed_out" });
    render(<GroupJoin groupId={ID} initial={{ status: "none" }} join={join} />);

    fireEvent.click(screen.getByRole("button", { name: t.ask }));
    await waitFor(() => expect(screen.getByRole("link", { name: messages.auth.signInWithGoogle })).toBeTruthy());
  });

  it("explains a refused request", async () => {
    const join = vi.fn().mockResolvedValue({ ok: false, error: "too_many" });
    render(<GroupJoin groupId={ID} initial={{ status: "none" }} join={join} />);

    fireEvent.click(screen.getByRole("button", { name: t.ask }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(t.errors.too_many));
  });
});
