# Domain Context

## Vocabulary
- **Match** (Vietnamese slang "keo"): one scheduled game at a venue with a fixed number of slots.
- **Host**: the user who created the match and collects payment.
- **Slot**: one player position on team A or B.
- **Format**: 5-a-side, 7-a-side or 11-a-side.
- **Match type**: casual, competitive, beginner-friendly.
- **Level**: player skill from 1.0 (beginner) to 5.0 (semi-pro), step 0.5.
- **No-show**: a player who held a slot but did not attend.
- **VietQR**: Vietnamese interbank QR standard for bank transfers.

## Business Rules (confirmed)
- Free; the app never holds or moves money.
- Players pay the host directly via VietQR.
- Football only; three formats; two teams per match.
- A player takes a specific slot on a specific team.
- Matches declare a level range and a match type.
- Sign-in via Google or Zalo.

## Decisions AI Must Not Invent
- Slot capacity per format, price rounding, slot hold/expiry timing.
- Cancellation and refund etiquette (the app has no refunds; what does it show?).
- Who may mark no-shows and how disputes work.
- How levels are assigned and updated.
- Visibility of the host's bank details.
- Whether one account may take multiple slots.

## Open Questions
All items under "Decisions AI Must Not Invent" — TODO: verify with Long.
