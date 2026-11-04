import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { PublicGroup } from "../lib/api";
import messages from "../messages/vi.json";
import { GroupList } from "./GroupList";

const t = messages.groups;

const lakeview: PublicGroup = {
  id: "0123456789ab",
  title: "Ai chơi bóng rổ cuối tuần này?",
  sport: "basketball",
  format: "3x3",
  venue: { name: "Sân Lakeview", address: "Sân bóng rổ Lakeview" },
  weekdays: [0, 6],
  playTime: "18:00",
  week: { players: 8, yes: [{ day: "2099-10-03", yes: 6 }] },
};

describe("GroupList", () => {
  it("shows each group's sport, venue, days, time and players this week", () => {
    render(<GroupList groups={[lakeview]} />);

    expect(screen.getByRole("heading", { name: t.title })).toBeTruthy();
    const card = screen.getByRole("listitem");
    expect(within(card).getByText("🏀 Bóng rổ · 3x3 · Sân Lakeview")).toBeTruthy();
    expect(within(card).getByText("Sân bóng rổ Lakeview")).toBeTruthy();
    expect(within(card).getByText("T7, CN · 18:00")).toBeTruthy();
    expect(within(card).getByText("8 người điểm danh tuần này")).toBeTruthy();
  });

  it("tells members how to answer in the Zalo group", () => {
    render(<GroupList groups={[lakeview, { ...lakeview, weekdays: [1] }]} />);

    const [weekend, monday] = screen.getAllByRole("listitem");
    expect(within(weekend).getByText(/@Bot DaGhep T7 có, CN không/)).toBeTruthy();
    expect(within(monday).getByText(/@Bot DaGhep T2 có/)).toBeTruthy();
  });

  it("says the week's poll is coming when none is open, and works without a venue", () => {
    render(
      <GroupList
        groups={[{ ...lakeview, sport: null, format: null, venue: null, playTime: null, week: null, weekdays: [1] }]}
      />,
    );

    const card = screen.getByRole("listitem");
    expect(within(card).getByText(lakeview.title)).toBeTruthy();
    expect(within(card).getByText("T2")).toBeTruthy();
    expect(within(card).getByText(t.noPollYet)).toBeTruthy();
  });

  it("links each group to its page, where people ask to join", () => {
    render(<GroupList groups={[lakeview]} />);

    const link = screen.getByRole("link", { name: t.openLabel.replace("{title}", lakeview.title) });
    expect(link.textContent).toBe(t.open);
    expect(link.getAttribute("href")).toBe("/g/0123456789ab");
  });

  it("lists upcoming group games with a link to answer", () => {
    render(
      <GroupList
        groups={[]}
        games={[
          {
            title: "Bóng rổ vui vẻ hôm nay",
            sport: "basketball",
            format: null,
            venue: { name: "Sân Lakeview", address: "Sân bóng rổ Lakeview" },
            days: [{ day: "2099-10-01", yes: 3 }],
            playTime: "19:00",
            pollPath: "/p/abcDEF12",
          },
        ]}
      />,
    );

    expect(screen.getByRole("heading", { name: t.gamesTitle })).toBeTruthy();
    const card = screen.getByRole("listitem");
    expect(within(card).getByText("🏀 Bóng rổ · Sân Lakeview")).toBeTruthy();
    expect(within(card).getByText("Bóng rổ vui vẻ hôm nay")).toBeTruthy();
    expect(within(card).getByText("T5 01/10 · 19:00")).toBeTruthy();
    expect(within(card).getByText(t.gameYes.replace("{n}", "3"))).toBeTruthy();
    const link = within(card).getByRole("link", { name: t.gameOpenLabel.replace("{title}", "Bóng rổ vui vẻ hôm nay") });
    expect(link.getAttribute("href")).toBe("/p/abcDEF12");
  });

  it("renders nothing without groups or games", () => {
    const { container } = render(<GroupList groups={[]} games={[]} />);
    expect(container.innerHTML).toBe("");
  });
});
