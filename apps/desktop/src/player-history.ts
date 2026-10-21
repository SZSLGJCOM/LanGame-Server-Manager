import { invokeOrMock } from "./api-transport";

export type PlayerHistoryKind = "join" | "leave" | "chat";
export interface PlayerHistoryEvent {
  id: number;
  run_id: number;
  process_key: string;
  kind: PlayerHistoryKind;
  player_name: string;
  account_id: string | null;
  message: string | null;
  log_time: string;
  estimated_at_unix_ms: number | null;
  duration_ms: number | null;
  session_ambiguous: boolean;
}
export interface PlayerHistoryQuery {
  instance_id: string;
  kind?: PlayerHistoryKind;
  search?: string;
  before_id?: number;
  since_unix_ms?: number;
  until_unix_ms?: number;
  limit?: number;
}
export interface PlayerHistoryPage {
  instance_id: string;
  supported: boolean;
  events: PlayerHistoryEvent[];
  next_before_id: number | null;
  collection_issue: string | null;
}
export const readInstancePlayerHistory = (input: PlayerHistoryQuery) =>
  invokeOrMock<PlayerHistoryPage>("read_instance_player_history", { input });
