import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import messages from "../messages/vi.json";
import { BotLinkForm } from "./BotLinkForm";

const t = messages.botLink;

describe("BotLinkForm", () => {
  it("links only after the user confirms, then says what to do next", async () => {
    const link = vi.fn().mockResolvedValue("linked");
    render(<BotLinkForm code="abc_DEF-123" accountName="Long" link={link} />);

    expect(link).not.toHaveBeenCalled();
    expect(screen.getByText(/Long/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(t.done));
    expect(link).toHaveBeenCalledWith("abc_DEF-123");
    expect(screen.queryByRole("button", { name: t.submit })).toBeNull();
  });

  it("explains an expired or used link", async () => {
    const link = vi.fn().mockResolvedValue("invalid");
    render(<BotLinkForm code="abc" accountName={null} link={link} />);

    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(t.invalid));
  });

  it("lets the user try again after a failure", async () => {
    const link = vi.fn().mockResolvedValueOnce("failed").mockResolvedValueOnce("linked");
    render(<BotLinkForm code="abc" accountName={null} link={link} />);

    fireEvent.click(screen.getByRole("button", { name: t.submit }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(t.failed));
    fireEvent.click(screen.getByRole("button", { name: t.submit }));

    await waitFor(() => expect(screen.getByRole("status").textContent).toBe(t.done));
  });
});
