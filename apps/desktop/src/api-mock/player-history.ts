import type { PlayerHistoryPage, PlayerHistoryQuery } from "../player-history";

/** The development preview has no real server log ingestion. */
export function mockPlayerHistory(input: PlayerHistoryQuery): PlayerHistoryPage {
  return { instance_id: input.instance_id, supported: true, events: [], next_before_id: null, collection_issue: null };
}
