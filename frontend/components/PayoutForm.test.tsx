import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import messages from "../messages/vi.json";
import { PayoutForm } from "./PayoutForm";

const t = messages.payout;
const banks = [
  { bin: "970436", name: "Vietcombank" },
  { bin: "970416", name: "ACB" },
];

function fill() {
  fireEvent.change(screen.getByLabelText(t.bank), { target: { value: "970416" } });
  fireEvent.change(screen.getByLabelText(t.accountNumber), { target: { value: " 257678859 " } });
  fireEvent.change(screen.getByLabelText(t.accountName), { target: { value: "Phạm Long" } });
  fireEvent.blur(screen.getByLabelText(t.accountName));
}

describe("PayoutForm", () => {
  it("converts the name as the host types and saves the account", async () => {
    const save = vi.fn().mockResolvedValue({ ok: true });
    render(<PayoutForm banks={banks} initial={null} next={null} save={save} />);
    fill();

    expect((screen.getByLabelText(t.accountName) as HTMLInputElement).value).toBe("PHAM LONG");
    fireEvent.click(screen.getByRole("button", { name: t.save }));

    expect(await screen.findByText(t.saved)).toBeTruthy();
    expect(save).toHaveBeenCalledWith({ bankBin: "970416", accountNumber: "257678859", accountName: "PHAM LONG" });
  });

  it("leaves the name alone while typing so Vietnamese keyboards keep working", () => {
    render(<PayoutForm banks={banks} initial={null} next={null} save={vi.fn()} />);

    fireEvent.change(screen.getByLabelText(t.accountName), { target: { value: "Phạm" } });

    expect((screen.getByLabelText(t.accountName) as HTMLInputElement).value).toBe("Phạm");
  });

  it("starts from the saved account", () => {
    render(
      <PayoutForm
        banks={banks}
        initial={{ bankBin: "970436", accountNumber: "0123456789", accountName: "PHAM LONG" }}
        next={null}
        save={vi.fn()}
      />,
    );

    expect((screen.getByLabelText(t.bank) as HTMLSelectElement).value).toBe("970436");
    expect((screen.getByLabelText(t.accountNumber) as HTMLInputElement).value).toBe("0123456789");
  });

  it("asks to choose again when the saved bank is no longer supported", () => {
    render(
      <PayoutForm
        banks={banks}
        initial={{ bankBin: "999999", accountNumber: "0123456789", accountName: "PHAM LONG" }}
        next={null}
        save={vi.fn()}
      />,
    );

    expect((screen.getByLabelText(t.bank) as HTMLSelectElement).value).toBe("");
    expect(screen.getByText(t.bankUnsupported)).toBeTruthy();
  });

  it("shows the field the server rejected", async () => {
    render(
      <PayoutForm banks={banks} initial={null} next={null} save={vi.fn().mockResolvedValue({ ok: false, error: "accountNumber" })} />,
    );
    fill();

    fireEvent.click(screen.getByRole("button", { name: t.save }));

    expect(await screen.findByRole("alert")).toHaveProperty("textContent", t.errors.accountNumber);
  });

  it("offers the way back to match creation after saving", async () => {
    render(<PayoutForm banks={banks} initial={null} next="/create" save={vi.fn().mockResolvedValue({ ok: true })} />);
    fill();

    fireEvent.click(screen.getByRole("button", { name: t.save }));

    const back = await screen.findByRole("link", { name: t.backToCreate });
    expect(back.getAttribute("href")).toBe("/create");
  });
});
