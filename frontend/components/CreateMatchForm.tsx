"use client";

import { type FormEvent, useState } from "react";
import type { CreateMatchInput, MatchView, Venue } from "../lib/api";
import { formatVnd } from "../lib/format";
import {
  MAX_DURATION_HOURS,
  MAX_SLOT_COUNT,
  MAX_TOTAL_FEE_VND,
  MIN_SLOT_COUNT,
  defaultSlotCount,
  pricePerPlayerVnd,
} from "../lib/pricing";
import messages from "../messages/vi.json";

const t = messages.create;
const FORMATS: MatchView["format"][] = ["five_a_side", "seven_a_side", "eleven_a_side"];
const MATCH_TYPES: MatchView["matchType"][] = ["casual", "competitive", "beginner_friendly"];
const LEVELS = Array.from({ length: 9 }, (_, i) => 1 + i * 0.5);

type Props = {
  /** Venues the host can choose from. */
  venues: Venue[];
  /** Resolves with the rejected field, or undefined when the match was created (the caller navigates). */
  onSubmit: (input: CreateMatchInput) => Promise<{ field: string } | undefined>;
  /** Whether the host saved a payout account; a paid match needs one. */
  hasPayout?: boolean;
};

const PAYOUT_PAGE = `/account/payout?next=${encodeURIComponent("/create")}`;

/** Converts a date and time picked in Ho Chi Minh City (UTC+7, no DST) to a UTC ISO string. */
function toUtcIso(date: string, time: string): string {
  return new Date(`${date}T${time}:00+07:00`).toISOString();
}

function minutesOfDay(time: string): number {
  const [hours, minutes] = time.split(":").map(Number);
  return hours * 60 + minutes;
}

function parseWholeNumber(value: string): number | null {
  if (value.trim() === "") {
    return null;
  }
  const parsed = Number(value);
  return Number.isInteger(parsed) ? parsed : null;
}

/** Client-side checks for what the browser form cannot express; the API re-validates everything. */
function checkInputs(values: {
  venueId: string;
  date: string;
  startTime: string;
  endTime: string;
  totalFee: string;
  slotCount: string;
}): string | null {
  if (!values.venueId) return "venueId";
  if (!values.date || !values.startTime) return "startsAt";
  // Same-day matches only: the end must be after the start, within the limit.
  const duration = values.endTime ? minutesOfDay(values.endTime) - minutesOfDay(values.startTime) : 0;
  if (duration <= 0 || duration > MAX_DURATION_HOURS * 60) return "endsAt";
  const fee = parseWholeNumber(values.totalFee);
  if (fee === null || fee < 0 || fee > MAX_TOTAL_FEE_VND) return "totalFeeVnd";
  const slots = parseWholeNumber(values.slotCount);
  if (slots === null || slots < MIN_SLOT_COUNT || slots > MAX_SLOT_COUNT) return "slotCount";
  return null;
}

function errorMessage(field: string): string {
  const errors: Record<string, string> = t.errors;
  return errors[field] ?? t.errors.unknown;
}

export function CreateMatchForm({ venues, onSubmit, hasPayout = true }: Props) {
  const [venueId, setVenueId] = useState("");
  const venue = venues.find((candidate) => candidate.id === venueId) ?? null;
  const [date, setDate] = useState("");
  const [startTime, setStartTime] = useState("");
  const [endTime, setEndTime] = useState("");
  const [format, setFormat] = useState<MatchView["format"]>("seven_a_side");
  const [matchType, setMatchType] = useState<MatchView["matchType"]>("casual");
  const [levelMin, setLevelMin] = useState(2.5);
  const [levelMax, setLevelMax] = useState(3.5);
  const [totalFee, setTotalFee] = useState("");
  const [slotCount, setSlotCount] = useState(String(defaultSlotCount("seven_a_side")));
  const [slotsTouched, setSlotsTouched] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [errorField, setErrorField] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const fee = parseWholeNumber(totalFee);
  const needsPayout = !hasPayout && fee !== null && fee > 0;
  const slots = parseWholeNumber(slotCount);
  const price = fee === null || slots === null ? null : pricePerPlayerVnd(fee, slots);

  function chooseFormat(next: MatchView["format"]) {
    setFormat(next);
    if (!slotsTouched) {
      setSlotCount(String(defaultSlotCount(next)));
    }
  }

  function showError(field: string | null, message: string) {
    setErrorField(field);
    setError(message);
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const invalid =
      checkInputs({ venueId, date, startTime, endTime, totalFee, slotCount }) ?? (needsPayout ? "payout" : null);
    if (invalid) {
      showError(invalid, errorMessage(invalid));
      return;
    }
    setError(null);
    setErrorField(null);
    setSubmitting(true);
    try {
      const rejected = await onSubmit({
        venueId,
        venueName: venue?.name ?? "",
        startsAt: toUtcIso(date, startTime),
        endsAt: toUtcIso(date, endTime),
        format,
        matchType,
        levelMin,
        levelMax,
        totalFeeVnd: Number(totalFee),
        slotCount: Number(slotCount),
      });
      if (rejected) {
        showError(rejected.field, errorMessage(rejected.field));
        setSubmitting(false);
      }
      // On success the caller navigates away; staying disabled prevents a duplicate match.
    } catch {
      showError(null, t.unexpectedError);
      setSubmitting(false);
    }
  }

  const invalidProps = (field: string) =>
    errorField === field ? { "aria-invalid": true, "aria-describedby": "create-form-error" } : {};

  return (
    <form className="create-form" onSubmit={handleSubmit} noValidate>
      <h1 className="create-form__title">{t.title}</h1>

      <div className="field">
        <label htmlFor="create-venue">{t.venue}</label>
        <select
          id="create-venue"
          value={venueId}
          onChange={(e) => setVenueId(e.target.value)}
          {...invalidProps("venueId")}
          required
        >
          <option value="">{t.venuePlaceholder}</option>
          {venues.map((option) => (
            <option key={option.id} value={option.id}>
              {option.name}
            </option>
          ))}
        </select>
        {venue ? <span className="field-hint">{venue.address}</span> : null}
        {venues.length === 0 ? <span className="form-error">{t.venuesUnavailable}</span> : null}
        <span className="field-hint">{t.venueMissing}</span>
      </div>

      <label className="field">
        <span>{t.date}</span>
        <input
          type="date"
          value={date}
          onChange={(e) => setDate(e.target.value)}
          required
          {...invalidProps("startsAt")}
        />
      </label>

      <div className="field-row">
        <label className="field">
          <span>{t.startTime}</span>
          <input
            type="time"
            value={startTime}
            onChange={(e) => setStartTime(e.target.value)}
            required
            {...invalidProps("startsAt")}
          />
        </label>
        <label className="field">
          <span>{t.endTime}</span>
          <input
            type="time"
            value={endTime}
            onChange={(e) => setEndTime(e.target.value)}
            required
            {...invalidProps("endsAt")}
          />
        </label>
      </div>

      <fieldset className="field">
        <legend>{t.format}</legend>
        <div className="segmented">
          {FORMATS.map((option) => (
            <label key={option} className="segmented__option">
              <input
                type="radio"
                name="format"
                value={option}
                checked={format === option}
                onChange={() => chooseFormat(option)}
              />
              <span>{messages.match.format[option]}</span>
            </label>
          ))}
        </div>
      </fieldset>

      <fieldset className="field">
        <legend>{t.matchType}</legend>
        <div className="chips">
          {MATCH_TYPES.map((option) => (
            <label key={option} className="chip">
              <input
                type="radio"
                name="matchType"
                value={option}
                checked={matchType === option}
                onChange={() => setMatchType(option)}
              />
              <span>{messages.match.matchType[option]}</span>
            </label>
          ))}
        </div>
      </fieldset>

      <div className="field-row">
        <label className="field">
          <span>{t.levelMin}</span>
          <select value={levelMin} onChange={(e) => setLevelMin(Number(e.target.value))}>
            {LEVELS.map((level) => (
              <option key={level} value={level}>
                {level.toFixed(1)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{t.levelMax}</span>
          <select value={levelMax} onChange={(e) => setLevelMax(Number(e.target.value))}>
            {LEVELS.map((level) => (
              <option key={level} value={level}>
                {level.toFixed(1)}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="field-row">
        <label className="field">
          <span>{t.totalFee}</span>
          <input
            type="number"
            inputMode="numeric"
            min={0}
            step={1000}
            value={totalFee}
            onChange={(e) => setTotalFee(e.target.value)}
            required
            {...invalidProps("totalFeeVnd")}
          />
        </label>
        <label className="field">
          <span>{t.slotCount}</span>
          <input
            type="number"
            inputMode="numeric"
            min={MIN_SLOT_COUNT}
            max={MAX_SLOT_COUNT}
            value={slotCount}
            onChange={(e) => {
              setSlotsTouched(true);
              setSlotCount(e.target.value);
            }}
            required
            {...invalidProps("slotCount")}
          />
        </label>
      </div>
      <p className="muted field-hint">{t.slotHint}</p>

      {needsPayout ? (
        <p className="notice">
          {t.payoutNeeded} <a href={PAYOUT_PAGE}>{t.payoutLink}</a>
        </p>
      ) : null}
      <p className="price-preview" aria-live="polite">
        <span>{t.pricePerPlayer}</span>
        <strong data-testid="price-per-player">{price === null ? "—" : formatVnd(price)}</strong>
      </p>

      {error ? (
        <p className="form-error" role="alert" id="create-form-error">
          {error}
        </p>
      ) : null}

      <button type="submit" className="button-primary" disabled={submitting}>
        {submitting ? t.submitting : t.submit}
      </button>
    </form>
  );
}
