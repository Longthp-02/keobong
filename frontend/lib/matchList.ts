import messages from "../messages/vi.json";

const t = messages.list;
const TIME_ZONE = "Asia/Ho_Chi_Minh";
export const LIST_DAYS = 7;

const isoDate = new Intl.DateTimeFormat("en-CA", {
  timeZone: TIME_ZONE,
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
});

export type ListDay = {
  /** YYYY-MM-DD in Ho Chi Minh City, as the API expects. */
  date: string;
  label: string;
  sub: string;
};

/** Today and the next six days in Ho Chi Minh City, labelled for the day picker. */
export function listDays(now: Date): ListDay[] {
  const [year, month, day] = isoDate.format(now).split("-").map(Number);
  return Array.from({ length: LIST_DAYS }, (_, offset) => {
    // Calendar arithmetic in UTC: the date, not the instant, is what matters here.
    const date = new Date(Date.UTC(year, month - 1, day + offset));
    const weekday = date.getUTCDay();
    const dayMonth = `${date.getUTCDate()}/${date.getUTCMonth() + 1}`;
    const label = offset === 0 ? t.today : offset === 1 ? t.tomorrow : t.weekday[weekday];
    const sub = offset < 2 ? `${t.weekdayShort[weekday]} ${dayMonth}` : dayMonth;
    return { date: date.toISOString().slice(0, 10), label, sub };
  });
}

const oneDecimal = new Intl.NumberFormat("vi-VN", { maximumFractionDigits: 1 });

/** "850 m", "1,2 km", "12 km". */
export function formatDistance(metres: number): string {
  if (metres < 1000) {
    return `${Math.round(metres)} m`;
  }
  const km = metres / 1000;
  return `${km >= 10 ? Math.round(km) : oneDecimal.format(km)} km`;
}

/** Short price for a card badge: "50k", or "Miễn phí". */
export function formatShortVnd(amount: number): string {
  return amount > 0 ? `${Math.round(amount / 1000)}k` : t.free;
}
