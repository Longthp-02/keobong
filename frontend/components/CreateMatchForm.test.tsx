import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import messages from "../messages/vi.json";
import { CreateMatchForm } from "./CreateMatchForm";

const t = messages.create;

function field(label: string) {
  return screen.getByLabelText(label) as HTMLInputElement;
}

function fillRequired() {
  fireEvent.change(field(t.venue), { target: { value: "SSA Sports Center" } });
  fireEvent.change(field(t.date), { target: { value: "2099-10-10" } });
  fireEvent.change(field(t.startTime), { target: { value: "18:30" } });
  fireEvent.change(field(t.endTime), { target: { value: "20:00" } });
  fireEvent.change(field(t.totalFee), { target: { value: "900000" } });
}

describe("CreateMatchForm", () => {
  it("defaults the slot count by format and previews the price per player", () => {
    render(<CreateMatchForm onSubmit={vi.fn()} />);
    fillRequired();

    expect(field(t.slotCount).value).toBe("18");
    expect(screen.getByTestId("price-per-player").textContent).toMatch(/50\.000/);

    fireEvent.click(screen.getByRole("radio", { name: messages.match.format.five_a_side }));

    expect(field(t.slotCount).value).toBe("14");
    expect(screen.getByTestId("price-per-player").textContent).toMatch(/65\.000/);
  });

  it("keeps a slot count the host typed when the format changes", () => {
    render(<CreateMatchForm onSubmit={vi.fn()} />);
    fireEvent.change(field(t.slotCount), { target: { value: "20" } });

    fireEvent.click(screen.getByRole("radio", { name: messages.match.format.eleven_a_side }));

    expect(field(t.slotCount).value).toBe("20");
  });

  it("submits times in Ho Chi Minh City converted to UTC", async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<CreateMatchForm onSubmit={onSubmit} />);
    fillRequired();

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit).toHaveBeenCalledWith({
      venueName: "SSA Sports Center",
      startsAt: "2099-10-10T11:30:00.000Z",
      endsAt: "2099-10-10T13:00:00.000Z",
      format: "seven_a_side",
      matchType: "casual",
      levelMin: 2.5,
      levelMax: 3.5,
      totalFeeVnd: 900000,
      slotCount: 18,
    });
  });

  it("shows the message for the field the server rejected", async () => {
    const onSubmit = vi.fn().mockResolvedValue({ field: "startsAt" });
    render(<CreateMatchForm onSubmit={onSubmit} />);
    fillRequired();

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.startsAt);
  });
});
