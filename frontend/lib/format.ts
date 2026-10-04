const TIME_ZONE = "Asia/Ho_Chi_Minh";

const timeFormat = new Intl.DateTimeFormat("vi-VN", {
  timeZone: TIME_ZONE,
  hour: "2-digit",
  minute: "2-digit",
  hour12: false,
});

const dateFormat = new Intl.DateTimeFormat("vi-VN", {
  timeZone: TIME_ZONE,
  weekday: "long",
  day: "2-digit",
  month: "2-digit",
});

const vndFormat = new Intl.NumberFormat("vi-VN", { style: "currency", currency: "VND" });

export function formatTimeRange(startsAt: string, endsAt: string): string {
  return `${timeFormat.format(new Date(startsAt))} – ${timeFormat.format(new Date(endsAt))}`;
}

export function formatMatchDate(startsAt: string): string {
  return dateFormat.format(new Date(startsAt));
}

export function formatVnd(amount: number): string {
  return vndFormat.format(amount);
}

export function formatLevelRange(min: number, max: number): string {
  return `${min.toFixed(1)} – ${max.toFixed(1)}`;
}
