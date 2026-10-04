"use client";

import { type FormEvent, useCallback, useEffect, useState } from "react";
import { signInUrl } from "../lib/api";
import {
  type MyPlace,
  type RosterView,
  type SlotError,
  type SlotsClient,
  type Team,
  type TeamView,
  slotsClient,
} from "../lib/slots";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";

const t = messages.slots;
const MAX_GUESTS = 2;
const defaultClient = slotsClient();

type Props = {
  shareId: string;
  startsAt: string;
  /** Server-rendered roster; refreshed from the API after every change. */
  initialRoster: RosterView;
  client?: SlotsClient;
};

function openPlaces(team: TeamView): number {
  return Math.max(0, team.capacity - team.players.length);
}

function Avatar({ name, url }: { name: string; url: string | null }) {
  if (url) {
    // Google avatars refuse requests that carry a referrer from other sites.
    return <img className="avatar" src={url} alt="" width={32} height={32} referrerPolicy="no-referrer" />;
  }
  return (
    <span className="avatar avatar--initial" aria-hidden="true">
      {name.trim().charAt(0).toUpperCase()}
    </span>
  );
}

function TeamColumn({ team }: { team: TeamView }) {
  const label = t.team[team.team];
  const open = openPlaces(team);
  return (
    <section className="team" aria-label={label}>
      <header className="team__header">
        <h3>{label}</h3>
        <span className="muted">{open > 0 ? fill(t.openPlaces, { count: open }) : t.full}</span>
      </header>
      <ul className="team__players">
        {team.players.map((player, index) => {
          const name = player.name ?? messages.auth.anonymousName;
          return (
            <li key={`${name}-${index}`} className="player">
              <Avatar name={name} url={player.avatarUrl} />
              <span className="player__name">
                {name}
                {player.isGuest ? (
                  <small className="muted">
                    {fill(t.guestOf, { name: player.guestOf ?? messages.auth.anonymousName })}
                  </small>
                ) : null}
              </span>
            </li>
          );
        })}
        {Array.from({ length: open }, (_, index) => (
          <li key={`open-${index}`} className="player player--open">
            {t.emptyPlace}
          </li>
        ))}
      </ul>
    </section>
  );
}

export function TeamSlots({ shareId, startsAt, initialRoster, client = defaultClient }: Props) {
  const [roster, setRoster] = useState(initialRoster);
  const [mine, setMine] = useState<MyPlace | null>(null);
  // Decided after hydration so server and client render the same markup.
  const [started, setStarted] = useState(false);
  const [team, setTeam] = useState<Team | null>(null);
  const [guests, setGuests] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);

  const refreshRoster = useCallback(async () => {
    try {
      setRoster(await client.roster(shareId));
    } catch {
      // Keep showing the last roster; the next action or page load retries.
    }
  }, [client, shareId]);

  const loadMine = useCallback(() => {
    setLoadFailed(false);
    client.mine(shareId).then(setMine, () => setLoadFailed(true));
  }, [client, shareId]);

  useEffect(() => {
    setStarted(Date.parse(startsAt) <= Date.now());
    loadMine();
    // The page itself is cached for up to 30 seconds; show who has joined since.
    void refreshRoster();
  }, [startsAt, loadMine, refreshRoster]);

  const partySize = 1 + guests.length;
  const fits = (candidate: TeamView) => openPlaces(candidate) >= partySize;
  const chosenView = roster.teams.find((candidate) => candidate.team === team && fits(candidate));
  const chosen = chosenView?.team ?? roster.teams.find(fits)?.team ?? null;

  function showError(code: SlotError) {
    setError(t.errors[code]);
    // Move the UI to the state the server reported.
    if (code === "unauthenticated") {
      setMine({ status: "signedOut" });
    }
    if (code === "match_started") {
      setStarted(true);
    }
  }

  async function join(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!chosen) {
      return;
    }
    const names = guests.map((name) => name.trim());
    if (names.some((name) => name.length === 0 || name.length > 40)) {
      showError("guests");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await client.join(shareId, chosen, names);
      if (result.ok) {
        setMine(result.place);
        setGuests([]);
      } else {
        showError(result.error);
        if (result.error === "already_joined") {
          setMine(await client.mine(shareId));
        }
      }
      await refreshRoster();
    } catch {
      showError("unexpected");
    } finally {
      setBusy(false);
    }
  }

  async function leave() {
    setBusy(true);
    setError(null);
    try {
      const result = await client.leave(shareId);
      if (result.ok) {
        setMine({ status: "out" });
      } else {
        showError(result.error);
      }
      await refreshRoster();
    } catch {
      showError("unexpected");
    } finally {
      setBusy(false);
    }
  }

  function actions() {
    if (started) {
      return <p className="muted">{t.started}</p>;
    }
    if (loadFailed) {
      return (
        <button type="button" className="button-secondary" onClick={loadMine}>
          {t.retry}
        </button>
      );
    }
    if (!mine) {
      return null;
    }
    if (mine.status === "signedOut") {
      return (
        <a href={signInUrl(`/m/${shareId}`)} className="button-primary">
          {t.signInToJoin}
        </a>
      );
    }
    if (mine.status === "in") {
      return (
        <div className="my-place">
          <p>
            <strong>{fill(t.joined, { team: t.team[mine.team] })}</strong>
          </p>
          {mine.guests.length > 0 ? <p className="muted">{fill(t.withGuests, { names: mine.guests.join(", ") })}</p> : null}
          <button type="button" className="button-secondary" onClick={leave} disabled={busy}>
            {busy ? t.leaving : t.leave}
          </button>
        </div>
      );
    }
    return (
      <form className="join-form" onSubmit={join} noValidate>
        <fieldset className="field">
          <legend>{t.chooseTeam}</legend>
          <div className="segmented segmented--two">
            {roster.teams.map((candidate) => (
              <label key={candidate.team} className="segmented__option">
                <input
                  type="radio"
                  name="team"
                  value={candidate.team}
                  checked={chosen === candidate.team}
                  disabled={!fits(candidate)}
                  onChange={() => setTeam(candidate.team)}
                />
                <span>{t.team[candidate.team]}</span>
              </label>
            ))}
          </div>
        </fieldset>
        <fieldset className="field">
          <legend>{t.guestsLabel}</legend>
          {guests.map((name, index) => (
            <div key={index} className="guest-row">
              <input
                type="text"
                value={name}
                maxLength={40}
                placeholder={t.guestPlaceholder}
                aria-label={`${t.guestPlaceholder} ${index + 1}`}
                onChange={(e) => setGuests(guests.map((g, i) => (i === index ? e.target.value : g)))}
              />
              <button
                type="button"
                className="link-button"
                onClick={() => setGuests(guests.filter((_, i) => i !== index))}
              >
                {t.removeGuest}
              </button>
            </div>
          ))}
          {guests.length < MAX_GUESTS ? (
            <button type="button" className="link-button" onClick={() => setGuests([...guests, ""])}>
              {t.addGuest}
            </button>
          ) : null}
        </fieldset>
        <button type="submit" className="button-primary" disabled={!chosen || busy}>
          {busy ? t.joining : chosen ? fill(t.join, { team: t.team[chosen] }) : t.full}
        </button>
      </form>
    );
  }

  return (
    <section className="team-slots" aria-labelledby="team-slots-title">
      <h2 id="team-slots-title" className="team-slots__title">
        {t.title}
      </h2>
      <div className="team-slots__teams">
        {roster.teams.map((candidate) => (
          <TeamColumn key={candidate.team} team={candidate} />
        ))}
      </div>
      {error ? (
        <p className="form-error" role="alert">
          {error}
        </p>
      ) : null}
      {actions()}
    </section>
  );
}
