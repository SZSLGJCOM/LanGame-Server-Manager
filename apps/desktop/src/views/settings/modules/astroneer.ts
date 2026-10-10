import type { SettingsModuleDefinition } from "../module-types";
import { AstroneerSaveSelect } from "../AstroneerSaveSelect";
import { createSurvivalDedicatedSettingsDefinition } from "./survival-dedicated";

const ASTRONEER_FIELD_GROUPS = [
  { id: "network", sectionId: "network", keys: ["public_ip", "console_password"] },
  { id: "access", sectionId: "access", keys: ["deny_unlisted_players", "server_owner_display_name", "owner_guid"] },
  {
    id: "world",
    sectionId: "world",
    keys: [
      "load_auto_save",
      "auto_save_interval_seconds",
      "backup_save_interval_seconds",
      "disable_server_travel"
    ]
  },
  {
    id: "performance",
    sectionId: "performance",
    keys: [
      "max_server_framerate",
      "max_server_idle_framerate",
      "wait_for_players_before_shutdown",
      "player_activity_timeout_seconds"
    ]
  },
  { id: "advanced", sectionId: "advanced", keys: ["verbose_player_properties", "extra_launch_args"] }
] as const;

const astroneerBaseSettingsDefinition = createSurvivalDedicatedSettingsDefinition(
  "astroneer",
  ["network", "access", "world", "performance", "advanced"],
  ASTRONEER_FIELD_GROUPS
);

export const astroneerSettingsDefinition: SettingsModuleDefinition = {
  id: "astroneer",
  fieldPresentationOverrides: {
    active_save_file_name: {
      state: "specialized", owner: "configuration", sectionId: "room", rendererId: "astroneer-save-select"
    }
  },
  specializedRenderers: {
    "astroneer-save-select": { kind: "module-addon", sectionId: "room", keepMounted: true, Renderer: AstroneerSaveSelect }
  },
  getSections: astroneerBaseSettingsDefinition.getSections,
  buildFieldGroups: astroneerBaseSettingsDefinition.buildFieldGroups,
  getFieldCopy(key, t, locale) {
    if (key === "active_save_file_name") return {
      title: t("settings.schema.astroneer.active_save_file_name.title", undefined, "Startup Save"),
      description: t("settings.schema.astroneer.active_save_file_name.description", undefined,
        "Choose an existing save to use on the next start with automatic loading enabled. Custom Game rules stay with each save.")
    };
    return astroneerBaseSettingsDefinition.getFieldCopy?.(key, t, locale);
  }
};
