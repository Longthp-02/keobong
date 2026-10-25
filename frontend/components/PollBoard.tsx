"use client";

import { useState } from "react";
import { type AnswerPollResult, type PollAnswer, type PollView, browserAnswerPoll, signInUrl } from "../lib/api";
import messages from "../messages/vi.json";

const t = messages.poll;

type Props = {
  shareId: string;
  initial: PollView;
  answer?: (shareId: string, answers: PollAnswer[]) => Promise<AnswerPollResult>;
};

/** "T7 03/10" from a YYYY-MM-DD day. */
export function dayLabel(day: string): string {
  const [year, month, date] = day.split("-").map(Number);
  const weekday = new Date(Date.UTC(year, month - 1, date)).getUTCDay();
  return `${t.weekdays[weekday]} ${String(date).padStart(2, "0")}/${String(month).padStart(2, "0")}`;
}

function answerText(value: boolean | undefined): string {
  return value === undefined ? t.unanswered : value ? t.yes : t.no;
}

/** The attendance table everyone sees, and the signed-in player's yes/no for each day. */
export function PollBoard({ shareId, initial, answer = browserAnswerPoll }: Props) {
  const [poll, setPoll] = useState(initial);
  const [choices, setChoices] = useState<Record<string, boolean>>(initial.mine ?? {});
  const [state, setState] = useState<"idle" | "saving" | "saved" | "incomplete" | "closed" | "failed" | "signed_out">(
    "idle",
  );

  async function save() {
    if (poll.days.some((day) => choices[day] === undefined)) {
      setState("incomplete");
      return;
    }
    setState("saving");
    const result = await answer(
      shareId,
      poll.days.map((day) => ({ day, attending: choices[day] })),
    );
    if (result.ok) {
      setPoll(result.poll);
      setState("saved");
    } else {
      setState(result.error);
    }
  }

  const status = {
    idle: "",
    saving: "",
    saved: t.saved,
    incomplete: t.pickAll,
    closed: t.closed,
    failed: t.failed,
    signed_out: messages.create.errors.unauthenticated,
  }[state];
  const signedIn = poll.mine !== null;

  return (
    <section className="poll">
      <h1 className="create-form__title">{poll.title}</h1>
      {poll.playTime ? <p className="poll__time">{t.playTime.replace("{time}", poll.playTime)}</p> : null}
      {/* Focusable so keyboard users can scroll a wide table on phones. */}
      <div className="poll__scroll" role="region" aria-label={poll.title} tabIndex={0}>
        <table className="poll__table">
          <thead>
            <tr>
              <th scope="col">{t.playerColumn}</th>
              {poll.days.map((day) => (
                <th scope="col" key={day}>
                  {dayLabel(day)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {poll.players.length === 0 ? (
              <tr>
                <td colSpan={poll.days.length + 1} className="muted">
                  {t.empty}
                </td>
              </tr>
            ) : (
              poll.players.map((player, index) => (
                <tr key={index}>
                  <th scope="row">{player.name ?? messages.auth.anonymousName}</th>
                  {poll.days.map((day) => (
                    <td key={day} className={player.answers[day] ? "poll__yes" : undefined}>
                      {answerText(player.answers[day])}
                    </td>
                  ))}
                </tr>
              ))
            )}
          </tbody>
          <tfoot>
            <tr>
              <th scope="row">{t.countRow}</th>
              {poll.counts.map((count) => (
                <td key={count.day}>{count.yes}</td>
              ))}
            </tr>
          </tfoot>
        </table>
      </div>

      {poll.closed ? (
        <p className="muted">{t.closed}</p>
      ) : signedIn ? (
        <div className="poll__answer">
          <h2 className="poll__question">{t.yourAnswer}</h2>
          {poll.days.map((day) => (
            <fieldset key={day} className="poll__day">
              <legend>{dayLabel(day)}</legend>
              {[true, false].map((value) => (
                <label key={String(value)} className="poll__choice">
                  <input
                    type="radio"
                    name={`day-${day}`}
                    aria-label={`${dayLabel(day)} ${value ? t.yes : t.no}`}
                    checked={choices[day] === value}
                    onChange={() => setChoices({ ...choices, [day]: value })}
                  />
                  {value ? t.yes : t.no}
                </label>
              ))}
            </fieldset>
          ))}
          <button type="button" className="button-primary" onClick={save} disabled={state === "saving"}>
            {state === "saving" ? t.saving : t.save}
          </button>
        </div>
      ) : (
        <div className="sign-in">
          <h2 className="poll__question">{t.signInTitle}</h2>
          <p className="muted">{t.signInBody}</p>
          {/* A plain link: sign-in is a full-page redirect to Google, not a client navigation. */}
          <a href={signInUrl(`/p/${shareId}`)} className="button-primary">
            {messages.auth.signInWithGoogle}
          </a>
        </div>
      )}
      <p role="status" aria-live="polite">
        {status}
      </p>
    </section>
  );
}
