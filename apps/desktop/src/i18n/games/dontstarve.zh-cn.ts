import type { MessageCatalog } from "../../i18n-config";
import { ZH_CN_DONT_STARVE_CORE_MESSAGES } from "./dontstarve.zh-cn.part1";
import { ZH_CN_DONT_STARVE_WORLD_MESSAGES } from "./dontstarve.zh-cn.part2";
import { ZH_CN_DONT_STARVE_WORKSPACE_MESSAGES } from "./dontstarve.zh-cn.part3";
import { ZH_CN_DONT_STARVE_WORLD_COPY_MESSAGES } from "./dontstarve.zh-cn.part4";
import { ZH_CN_DONT_STARVE_SHARD_MESSAGES } from "./dontstarve.zh-cn.part5";

export const ZH_CN_DONT_STARVE_MESSAGES: MessageCatalog = {
  ...ZH_CN_DONT_STARVE_CORE_MESSAGES,
  ...ZH_CN_DONT_STARVE_WORLD_MESSAGES,
  ...ZH_CN_DONT_STARVE_WORKSPACE_MESSAGES,
  ...ZH_CN_DONT_STARVE_WORLD_COPY_MESSAGES,
  ...ZH_CN_DONT_STARVE_SHARD_MESSAGES
};
