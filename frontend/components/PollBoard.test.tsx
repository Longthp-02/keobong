import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { PollView } from "../lib/api";
import messages from "../messages/vi.json";
import { PollBoard } from "./PollBoard";

const t = messages.poll;

const poll: PollView = {
  title: "Ai chơi bóng rổ cuối tuần này?",
  days: ["2099-10-03", "2099-10-04"],
  playTime: "18:00",
  closesAt: "2099-10-04T17:00:00+00:00",
  closed: false,
  counts: [
    { day: "2099-10-03", yes: 1 },
    { day: "2099-10-04", yes: 0 },
  ],
  players: [{ name: "Nam", answers: { "2099-10-03": true, "2099-10-04": false } }],
  mine: null,
};

describe("PollBoard", () => {
  it("shows everyone's answers and the counts, with day labels", () => {
    render(<PollBoard shareId="abcDEF12" initial={poll} />);

    expect(screen.getByRole("heading", { name: poll.title })).toBeTruthy();
    expect(screen.getByText(t.playTime.replace("{time}", "18:00"))).toBeTruthy();
    const table = screen.getByRole("table");
    expect(within(table).getByText("T7 03/10")).toBeTruthy();
    expect(within(table).getByText("CN 04/10")).toBeTruthy();
    const nam = within(table).getByRole("row", { name: /Nam/ });
    expect(within(nam).getByText(t.yes)).toBeTruthy();
    expect(within(nam).getByText(t.no)).toBeTruthy();
    expect(within(table).getByRole("row", { name: new RegExp(t.countRow) }).textContent).toContain("1");
  });

  it("leaves out the play time for older polls without one", () => {
    render(<PollBoard shareId="abcDEF12" initial={{ ...poll, playTime: null }} />);

    expect(screen.queryByText(/18:00/)).toBeNull();
  });

  it("asks a signed-out visitor to sign in to answer", () => {
    render(<PollBoard shareId="abcDEF12" initial={poll} />);

    const link = screen.getByRole("link", { name: messages.auth.signInWithGoogle });
    expect(link.getAttribute("href")).toContain(encodeURIComponent("/p/abcDEF12"));
    expect(screen.queryByRole("button", { name: t.save })).toBeNull();
  });

  it("lets a signed-in player answer every day and shows the updated table", async () => {
    const updated: PollView = {
      ...poll,
      counts: [
        { day: "2099-10-03", yes: 2 },
        { day: "2099-10-04", yes: 0 },
      ],
      players: [...poll.players, { name: "Huy", answers: { "2099-10-03": true, "2099-10-04": false } }],
      mine: { "2099-10-03": true, "2099-10-04": false },
    };
    const answer = vi.fn().mockResolvedValue({ ok: true, poll: updated });
    render(<PollBoard shareId="abcDEF12" initial={{ ...poll, mine: {} }} answer={answer} />);

    fireEvent.click(screen.getByRole("button", { name: t.save }));
    expect(screen.getByRole("status").textContent).toBe(t.pickAll);
    expect(answer).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("radio", { name: `T7 03/10 ${t.yes}` }));
    fireEvent.click(screen.getByRole("radio", { name: `CN 04/10 ${t.no}` }));
    fireEvent.click(screen.getByRole("button", { name: t.save }));

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(t.saved));
    expect(answer).toHaveBeenCalledWith("abcDEF12", [
      { day: "2099-10-03", attending: true },
      { day: "2099-10-04", attending: false },
    ]);
    expect(screen.getByRole("row", { name: /Huy/ })).toBeTruthy();
  });

  it("starts from the player's saved answers", () => {
    render(<PollBoard shareId="abcDEF12" initial={{ ...poll, mine: { "2099-10-03": true } }} />);

    expect((screen.getByRole("radio", { name: `T7 03/10 ${t.yes}` }) as HTMLInputElement).checked).toBe(true);
    expect((screen.getByRole("radio", { name: `CN 04/10 ${t.yes}` }) as HTMLInputElement).checked).toBe(false);
  });

  it("says a closed poll takes no more answers", () => {
    render(<PollBoard shareId="abcDEF12" initial={{ ...poll, closed: true, mine: {} }} />);

    expect(screen.getByText(t.closed)).toBeTruthy();
    expect(screen.queryByRole("button", { name: t.save })).toBeNull();
  });

  it("explains a failed save", async () => {
    const answer = vi.fn().mockResolvedValue({ ok: false, error: "failed" });
    render(<PollBoard shareId="abcDEF12" initial={{ ...poll, mine: { "2099-10-03": true, "2099-10-04": true } }} answer={answer} />);

    fireEvent.click(screen.getByRole("button", { name: t.save }));

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(t.failed));
  });
});
