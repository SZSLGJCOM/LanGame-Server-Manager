import { createElement } from "react";
import { SATISFACTORY_RULES, satisfactoryRuleCopyKey } from "../../../satisfactory-world-settings";
import { SatisfactoryAuthorizationSettings, SatisfactoryRoomSettings } from "../SatisfactoryRoomSettings";
import { SatisfactoryNativeOptionField } from "../SatisfactoryNativeOptionField";
import { SatisfactoryWorldCreation, SatisfactoryWorldRules } from "../SatisfactoryWorldRules";
import { SatisfactoryWorldSettingsProvider } from "../SatisfactoryWorldSettingsContext";
import type { ConfigurationSpecializedRendererProps, SettingsModuleDefinition } from "../module-types";
import type { ConfigurationFieldPresentationOverride } from "../settings-schema";

const OPTION_SECTIONS: Readonly<Record<string, string>> = {
  auto_pause_when_empty: "world", weather_preset: "world", network_quality: "network", send_gameplay_data: "advanced"
};
const PANELS = { room: SatisfactoryRoomSettings, access: SatisfactoryAuthorizationSettings,
  world_generation: SatisfactoryWorldCreation, creative_rules: SatisfactoryWorldRules };

const overrides: Readonly<Record<string, ConfigurationFieldPresentationOverride>> = Object.fromEntries(
  Object.entries(OPTION_SECTIONS).map(([key, sectionId]) => [key, {
    state: "specialized", owner: "configuration", sectionId, rendererId: `satisfactory-option-${key}`
  }])
);

export const satisfactoryNativePresentation: Pick<SettingsModuleDefinition,
  "workspaceProvider" | "fieldPresentationOverrides" | "specializedRenderers" | "getAdditionalPresentationFields"> = {
  workspaceProvider: SatisfactoryWorldSettingsProvider,
  fieldPresentationOverrides: overrides,
  specializedRenderers: Object.fromEntries([
    ...Object.entries(PANELS).map(([sectionId, Renderer]) => [`satisfactory-native-${sectionId}`, {
      kind: "module-addon" as const, sectionId, keepMounted: true,
      saveMode: sectionId === "room" || sectionId === "creative_rules" ? "native-settings" as const : "explicit" as const, Renderer
    }] as const),
    ...Object.entries(OPTION_SECTIONS).map(([fieldKey, sectionId]) => [`satisfactory-option-${fieldKey}`, {
      kind: "module-addon" as const, sectionId,
      Renderer: (props: ConfigurationSpecializedRendererProps) => createElement(SatisfactoryNativeOptionField, { ...props, fieldKey })
    }] as const)
  ]),
  getAdditionalPresentationFields: ({ t }) => Object.keys(PANELS).map((sectionId) => ({
    key: `native_${sectionId}`, title: t(`satisfactory.settings.native.panels.${sectionId}`), sectionId,
    sourceSurface: "runtime_api", description: t(`satisfactory.settings.native.panelHelp.${sectionId}`),
    presentation: { state: "specialized", owner: "configuration", sectionId,
      rendererId: `satisfactory-native-${sectionId}`, aliases: [
        ...SATISFACTORY_RULES.filter((rule) => sectionId === "world_generation" ? rule.scope === "creation"
          : sectionId === "creative_rules" ? rule.scope !== "creation" : false)
          .flatMap((rule) => [rule.key, t(`satisfactory.settings.native.rules.${satisfactoryRuleCopyKey(rule.key)}.title`)]),
        ...(sectionId === "room" ? ["server name", "session", "save", "join password", "服务器名称", "存档", "入服密码"] :
          sectionId === "access" ? ["admin password", "authorization", "administrator", "管理密码", "授权"] :
          sectionId === "world_generation" ? ["world seed", "resource purity", "starting location", "世界种子", "资源分布", "矿点纯度", "新世界"] :
          ["creative", "new player defaults", "创造模式", "新玩家默认规则"])
      ] }
  }))
};
