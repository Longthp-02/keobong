# Domain Context

## Vocabulary
- **Match** (Vietnamese players call a pickup game "da ghep"; avoid the word "keo", which also means betting odds): one scheduled game at a venue with a fixed number of slots.
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
- Joining holds a slot for 30 minutes; without host payment confirmation in that window, the slot is released.
- One account holds its own slot plus up to 2 named guests per match.
- Only the host marks no-shows; players may dispute; the admin (Long) resolves disputes.
- Host bank details / VietQR are visible only to current slot holders of that match.
- Level is self-assessed at sign-up; peer rating is post-MVP.

## Decisions AI Must Not Invent
- Slot capacity per format and price rounding.
- Cancellation etiquette for hosts and players (the app has no refunds; what does it show?).
- Who may host, and whether venues are a separate host type in MVP.

## Open Questions
All items under "Decisions AI Must Not Invent" — TODO: verify with Long.
