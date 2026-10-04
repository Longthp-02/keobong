"use client";

import { type FormEvent, useState } from "react";
import type { CreateMatchInput, MatchView } from "../lib/api";
import { formatVnd } from "../lib/format";
import {
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
  /** Resolves with the rejected field, or undefined when the match was created (the caller navigates). */
  onSubmit: (input: CreateMatchInput) => Promise<{ field: string } | undefined>;
};

/** Converts a date and time picked in Ho Chi Minh City (UTC+7, no DST) to a UTC ISO string. */
function toUtcIso(date: string, time: string, addDays = 0): string {
  const instant = new Date(`${date}T${time}:00+07:00`);
  instant.setUTCDate(instant.getUTCDate() + addDays);
  return instant.toISOString();
}

/** An end time before the start time means the match ends after midnight. */
function endsNextDay(startTime: string, endTime: string): boolean {
  return Boolean(startTime && endTime) && endTime < startTime;
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
  date: string;
  startTime: string;
  endTime: string;
  totalFee: string;
  slotCount: string;
}): string | null {
  if (!values.date || !values.startTime) return "startsAt";
  if (!values.endTime || values.endTime === values.startTime) return "endsAt";
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

export function CreateMatchForm({ onSubmit }: Props) {
  const [venueName, setVenueName] = useState("");
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
  const slots = parseWholeNumber(slotCount);
  const price = fee === null || slots === null ? null : pricePerPlayerVnd(fee, slots);
  const nextDay = endsNextDay(startTime, endTime);

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
    const invalid = checkInputs({ date, startTime, endTime, totalFee, slotCount });
    if (invalid) {
      showError(invalid, errorMessage(invalid));
      return;
    }
    setError(null);
    setErrorField(null);
    setSubmitting(true);
    try {
      const rejected = await onSubmit({
        venueName,
        startsAt: toUtcIso(date, startTime),
        endsAt: toUtcIso(date, endTime, nextDay ? 1 : 0),
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

      <label className="field">
        <span>{t.venue}</span>
        <input
          type="text"
          value={venueName}
          maxLength={120}
          placeholder={t.venuePlaceholder}
          onChange={(e) => setVenueName(e.target.value)}
          {...invalidProps("venueName")}
          required
        />
      </label>

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
      {nextDay ? <p className="muted field-hint">{t.nextDay}</p> : null}

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
