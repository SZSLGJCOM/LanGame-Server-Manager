import type { SettingsModuleDefinition, SettingsModuleFieldGroup } from "./module-types";

export function isRemoteConsoleField(fieldKey: string): boolean {
  return fieldKey.startsWith("rcon_") || fieldKey === "enable_rcon" ||
    fieldKey === "broadcast_rcon_to_ops" || fieldKey === "rconserver_game_log_buffer";
}

export function withConfigurationFieldGroups(definition: SettingsModuleDefinition): SettingsModuleDefinition {
  return {
    ...definition,
    buildFieldGroups(sectionId, fields, locale, t) {
      if (sectionId === "room") {
        return fields.length > 0 ? [{ id: "room", fields }] : [];
      }
      const remoteConsoleFields = sectionId === "network"
        ? fields.filter((field) => isRemoteConsoleField(field.key))
        : [];
      const moduleFields = fields.filter((field) => !remoteConsoleFields.includes(field));
      const moduleGroups = moduleFields.length > 0
        ? definition.buildFieldGroups?.(sectionId, moduleFields, locale, t)
          ?? [{ id: "default", fields: moduleFields }]
        : [];
      const remoteConsoleGroups: SettingsModuleFieldGroup[] = remoteConsoleFields.length > 0
        ? [{ id: "remote-console", title: "RCON", fields: remoteConsoleFields }]
        : [];
      return [...remoteConsoleGroups, ...moduleGroups];
    }
  };
}
