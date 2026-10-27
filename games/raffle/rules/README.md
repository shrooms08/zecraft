# zecraft-raffle

The Zecraft kit's example game: a provably fair raffle on shielded Zcash
testnet. It is one `Rules` implementation (`src/lib.rs`); everything else
(chain following, the timing rule, drand, the 2-of-3 FROST vault, keyholder
checks, verification) is the kit's.

| | |
|---|---|
| Memo | `0xFF \| "ZRFL" \| 1 \| round u32 \| tickets u16 \| sealed receiver (92)`, or text `zrfl1:` + base64url |
| Ticket price | `ticket_price_zat`, default 100,000 (0.001 TAZ) |
| Valid entry | in the window, this round, at least one ticket, value exactly `tickets × price` |
| Refund | another round, outside the window, zero tickets, wrong value, voided round: `value − entry_fee − output_fee` |
| Kept (carry) | undecodable memo, or a refund that cannot pay its two fees |
| Draw | entries ordered by (height, txid, action); entry *i* owns tickets [sᵢ, sᵢ+tᵢ); winner = `uniform_below("zecraft/raffle/v1", round, beacon, T)` |
| Prize | `Σ valid − entry fees + carry_in − output_fee`, one output, no rake |
| No tickets | everything carries over |

Binaries (`games/raffle/server`): `zecraft-raffle-engine`,
`zecraft-raffle-keyholder`, `zecraft-raffle-verify`. Players use
`zecraft-player buy`.

Tests: `tests/raffle.rs` (classification table, ticket boundaries, draw
known answer, winner payout, voided round, zero tickets, conservation
property test).
