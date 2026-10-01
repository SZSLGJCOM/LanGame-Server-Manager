import type { MessageCatalog } from "./i18n-config";
import { ZH_CN_GAME_MESSAGES } from "./i18n/games/zh-cn";
import { ZH_CN_CORE_MESSAGES } from "./i18n-messages-zh-core";
import { ZH_CN_EXTRA_MESSAGES } from "./i18n-messages-zh-extra";
import { ZH_CN_SCHEMA_MESSAGES } from "./i18n-messages-zh-schema";
import { ZH_CN_SETTINGS_MESSAGES } from "./i18n-messages-zh-settings";
import { ZH_CN_UI_MESSAGES } from "./i18n-messages-zh-ui";

export const ZH_CN_MESSAGES: MessageCatalog = {
  ...ZH_CN_CORE_MESSAGES,
  ...ZH_CN_UI_MESSAGES,
  ...ZH_CN_GAME_MESSAGES,
  ...ZH_CN_EXTRA_MESSAGES,
  ...ZH_CN_SETTINGS_MESSAGES,
  ...ZH_CN_SCHEMA_MESSAGES
};
