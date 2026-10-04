import messages from "../messages/vi.json";
import type { MatchView } from "../lib/api";
import { formatLevelRange, formatMatchDate, formatTimeRange, formatVnd } from "../lib/format";

const t = messages.match;

export function MatchCard({ match }: { match: MatchView }) {
  return (
    <article className="match-card">
      <header className="match-card__hero">
        <div className="match-card__tags">
          <span className="tag tag--dark">{t.matchType[match.matchType]}</span>
          <span className="tag">{t.format[match.format]}</span>
        </div>
        <p className="match-card__date">{formatMatchDate(match.startsAt)}</p>
        <p className="match-card__time">{formatTimeRange(match.startsAt, match.endsAt)}</p>
        <h1 className="match-card__venue">{match.venueName}</h1>
        <p className="match-card__price">
          <span>{t.pricePerPlayer}</span>
          <strong data-testid="match-price">{formatVnd(match.pricePerPlayerVnd)}</strong>
        </p>
      </header>
      <dl className="match-card__stats">
        <div>
          <dt>{t.level}</dt>
          <dd>{formatLevelRange(match.levelMin, match.levelMax)}</dd>
        </div>
        <div>
          <dt>{t.totalFee}</dt>
          <dd>{formatVnd(match.totalFeeVnd)}</dd>
        </div>
        <div>
          <dt>{t.slots}</dt>
          <dd>{`${match.slotCount} ${t.slotsUnit}`}</dd>
        </div>
      </dl>
    </article>
  );
}
