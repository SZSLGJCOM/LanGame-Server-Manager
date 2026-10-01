const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");
const compile = (module, filename) => module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
require.extensions[".ts"] = compile;
require.extensions[".tsx"] = compile;
const { formatDesktopError } = require("../src/desktop-error-message.ts");
const { formatLaunchValidationIssue } = require("../src/launch-validation-message.ts");
const { message, resolveUiMessage } = require("../src/app-ui.ts");
const { translate } = require("../src/i18n.tsx");
const catalogs = {
  "zh-CN": require("../src/i18n-messages-zh-cn.ts").ZH_CN_MESSAGES,
  "en-US": require("../src/i18n-messages.ts").EN_US_MESSAGES
};
const translator = (locale) => (key, params, fallback) => translate(locale, key, params, fallback, catalogs);
const creationErrors = [
  ["服务器程序尚未下载完成，请先在游戏库完成安装或校验，再创建实例；本次没有启动下载。", /download is incomplete.*No download was started/s, "commands_program_creation.rs"],
  ["本地服务器程序不完整，请先在游戏库安装或校验；已有文件已保留，本次没有启动下载。", /program is incomplete.*Existing files were preserved.*no download was started/s, "commands_program_creation.rs"],
  ["没有可导入的本地程序库目录；归档中的实例请先恢复，或使用已验证程序创建。", /no local library program directory.*Restore the archived instance.*verified program/s, "commands_program_creation.rs"],
  ["现有实例或归档中的程序缺少完整校验清单，或程序文件已修改；已有文件已保留，本次没有启动下载。可恢复原实例，或先在游戏库安装或校验服务器程序。", /instance or archive.*verification manifest.*Existing files were preserved.*no download was started.*Restore the original instance/s, "commands_program_creation.rs"],
  ["此游戏尚不支持共享服务器程序，请使用独立安装。", /does not support shared.*independent installation/, "commands_storage.rs"]
];

for (const [native, english, file] of creationErrors) {
  test(`program creation rejection localizes and preserves diagnostics: ${native}`, () => {
    assert.ok(fs.readFileSync(`${__dirname}/../src-tauri/src/${file}`, "utf8").includes(native), "fixture must match the native rejection");
    for (const suffix of ["", ". See app log: D:/User files/app.log", ". Could not write the diagnostic: access denied. Log path: D:/User files/app.log"]) {
      const raw = `${native}${suffix}`;
      for (const error of [raw, new Error(raw)]) {
        assert.equal(formatDesktopError(translator("zh-CN"), error), raw);
        const rendered = formatDesktopError(translator("en-US"), error);
        assert.match(rendered, english);
        assert.doesNotMatch(rendered, /[\u4e00-\u9fff]/);
        assert.ok(rendered.endsWith(suffix));
      }
      const activity = message("activity.createServerFailed", { message: raw });
      assert.match(resolveUiMessage(translator("en-US"), activity), english);
      assert.ok(resolveUiMessage(translator("zh-CN"), activity).includes(raw));
      assert.equal(activity.params.message, raw, "language changes must not replace the saved diagnostic");
    }
  });
}

test("program creation translation preserves Unicode paths and unknown diagnostic variants", () => {
  const raw = creationErrors[0][0];
  const suffix = ". See app log: D:/用户文件/应用.log";
  assert.ok(formatDesktopError(translator("en-US"), raw + suffix).endsWith(suffix));
  for (const unknown of [`${raw} Additional diagnostic`, `${raw}. Additional diagnostic`, `Other error: ${raw}`, "本地程序准备失败：原始原因"]) {
    for (const locale of ["zh-CN", "en-US"]) assert.equal(formatDesktopError(translator(locale), unknown), unknown);
  }
});

const fixtures = [
  ["steam_workshop_browse_unrecognized_response", {}, /Steam.*工坊结果不完整或无法识别/, /Steam returned incomplete or unrecognized/],
  ["steam_workshop_details_unrecognized_response", {}, /未能读取 Steam 对应该语言的条目详情/, /Steam did not return recognizable item details in the selected language/],
  ["steam_workshop_browse_unsupported_sort", { browse_kind: "collection", sort: "subscribers" }, /合集不支持按订阅数排序/, /collections do not support sorting by subscriber count/],
  ["workshop-collection-install", { reason: "client-only", item_id: "1365141672" }, /1365141672.*仅在客户端/, /1365141672.*runs on the client/],
  ["steam_workshop_browse_invalid_response", {}, /工坊列表返回内容异常.*重新启动管理器/, /Workshop catalog response is invalid.*restart the manager/],
  ["mod_runtime_unverified", { reason: "minecraft_loader" }, /无法确认.*Minecraft/, /no verified Minecraft loader/],
  ["mod_runtime_unverified", { reason: "bepinex" }, /BepInEx 缺失/, /BepInEx is missing/],
  ["mod_dependencies_unverified", { dependencies: ["Team-Core-1.2.3"] }, /依赖.*Team-Core-1.2.3/, /dependencies.*Team-Core-1.2.3/],
  ["mod_community_mismatch", { community: "valheim", package: "Team-Mod" }, /Team-Mod.*valheim/, /Team-Mod.*valheim/],
  ["assistant_request_interpretation_failed", {}, /暂时没能理解这条请求，请重试。尚未执行任何操作。/, /could not understand this request\. Please try again\. No operation was performed\./],
  ["instance_already_running", { instance_id: "server-1", reason: "active_run_record" }, /已在运行/, /already running/],
  ["instance_not_running", { instance_id: "server-1" }, /未在运行/, /not running/],
  ["module_in_use", { module_name: "Palworld", instances: ["Server A", "Server B"] }, /仍在运行/, /servers are running/],
  ["install_data_protected", { module_name: "Palworld", install_root: "D:/server", protected: [{ source: "saved game", path: "D:/server/save" }] }, /受保护数据/, /protected data/],
  ["install_replacement_not_empty", { module_name: "Palworld", install_root: "D:/server" }, /非空/, /not empty/],
  ["install_path_not_directory", { install_root: "D:/server" }, /不是目录/, /not a directory/]
];

test("protected-data errors explain unresolved declarations instead of showing only a count", () => {
  const diagnostic = "Module save template: {{paths.install_root}}";
  const payload = { code: "install_data_protected", module_name: "Example", install_root: "D:/server",
    protected: [{ source: diagnostic, path: null }], message: "Cannot separate save data" };
  for (const locale of ["zh-CN", "en-US"]) {
    assert.ok(formatDesktopError(translator(locale), payload).includes(diagnostic));
  }
});
for (const [code, fields, zh, en] of fixtures) {
  test(`${code} renders both languages through a persisted activity message`, () => {
    const payload = { code, ...fields, message: "Original diagnostic 原始诊断" };
    for (const error of [JSON.stringify(payload), new Error(JSON.stringify(payload)), payload]) {
      assert.match(formatDesktopError(translator("zh-CN"), error), zh);
      assert.match(formatDesktopError(translator("en-US"), error), en);
    }
    const activity = message("activity.startServerFailed", { message: JSON.stringify(payload) });
    assert.match(resolveUiMessage(translator("zh-CN"), activity), zh);
    const english = resolveUiMessage(translator("en-US"), activity);
    assert.match(english, en);
    assert.doesNotMatch(english, /[\u4e00-\u9fff]/);
    assert.equal(activity.params.message, JSON.stringify(payload));
  });
}

test("unsupported native working directories explain path compatibility without raw backend text", () => {
  for (const locale of ["zh-CN", "en-US"]) {
    for (const path of ["D:/long local path/server", "\\\\?\\D:\\reserved.\\server", "\\\\host\\share\\server"]) {
      const issue = { code: "launch_working_directory_incompatible", severity: "error", path,
        message: "UNLOCALIZED_NATIVE_MESSAGE", context: { ignored: "UNTRUSTED_CONTEXT_TEXT" } };
      const expected = locale === "zh-CN" ? /启动程序.*工作目录.*较短.*本地路径.*特殊目录名/
        : /launch program.*working directory.*shorter local path.*special directory names/;
      for (const rendered of [
        formatLaunchValidationIssue(issue, translator(locale), locale),
        formatLaunchValidationIssue(issue, undefined, locale),
        formatDesktopError(translator(locale), JSON.stringify({ code: "launch_preflight_failed", message: "UNLOCALIZED_NATIVE_ENVELOPE", issues: [issue] }))
      ]) {
        assert.match(rendered, expected);
        assert.ok(rendered.includes(path), "The incompatible saved path remains exact");
        assert.doesNotMatch(rendered, /UNLOCALIZED_NATIVE_MESSAGE|UNLOCALIZED_NATIVE_ENVELOPE|UNTRUSTED_CONTEXT_TEXT|Java|下载|重装|修复|不存在|创建目录|download|reinstall|repair|does not exist|create.*director/i);
      }
    }
  }
});

test("preflight translation keeps file paths, ports, arguments and unknown diagnostics", () => {
  const issues = [
    { code: "launch_executable_missing", path: 'D:/服务端/"game".exe' },
    { code: "port_binding_unavailable", context: { port_name: "query", protocol: "UDP", address: "127.0.0.1:27015" } },
    { code: "managed_launch_option_conflict", context: { field: "custom_launch_flags" } },
    { code: "bind_ip_invalid", context: { bind_ip: "bad-address" } },
    { code: "launch_required_file_missing", context: { argument: "-jar" } }
  ].map((issue) => ({ severity: "error", message: "Original backend diagnostic", process_key: "main", display_name: "Main", ...issue }));
  const payload = JSON.stringify({ code: "launch_preflight_failed", message: "preflight failed", issues });
  const chinese = formatDesktopError(translator("zh-CN"), payload);
  for (const text of ['D:/服务端/"game".exe', "127.0.0.1:27015", "custom_launch_flags", "bad-address", "-jar"]) assert.ok(chinese.includes(text), text);
  assert.doesNotMatch(chinese, /Original backend|preflight failed|launch_executable_missing/);
  assert.match(formatDesktopError(translator("en-US"), payload), /Launch checks failed/);
  issues.push({ code: "future_check", severity: "error", message: "Unrecognized service response" });
  assert.match(formatDesktopError(translator("zh-CN"), { code: "launch_preflight_failed", message: "failed", issues }), /Unrecognized service response/);
});

test("unknown and malformed errors retain their original diagnostic without guessing from English text", () => {
  const values = ["Access denied", "{bad json", JSON.stringify({ code: "future_code", message: "details" }), JSON.stringify({ code: "launch_preflight_failed", message: "failed", issues: [{ code: 2 }] })];
  for (const value of values) assert.equal(formatDesktopError(translator("zh-CN"), new Error(value)), value);
});

test("collection errors distinguish each verified failure without claiming missing data", () => {
  const reasons = ["missing", "unresolved", "wrong-game", "unsupported", "client-only", "incomplete", "cycle", "empty-collection"];
  for (const locale of ["zh-CN", "en-US"]) {
    const rendered = reasons.map((reason) => formatDesktopError(translator(locale), new Error(JSON.stringify({
      code: "workshop-collection-install", message: "Technical diagnostic", reason, item_id: "1365141672"
    }))));
    assert.equal(new Set(rendered).size, reasons.length);
    for (const text of rendered) {
      assert.match(text, /1365141672/);
      assert.doesNotMatch(text, /Technical diagnostic|workshop-collection-install/);
    }
  }
  for (const fields of [{ reason: "future", item_id: "123" }, { reason: "missing", item_id: "" }]) {
    const raw = JSON.stringify({ code: "workshop-collection-install", message: "Keep original", ...fields });
    assert.equal(formatDesktopError(translator("zh-CN"), raw), raw);
  }
});

test("Workshop connection errors explain the failing service in both languages", () => {
  for (const [stage, origin, zhStage, enStage] of [
    ["browse", "https://steamcommunity.com", "创意工坊搜索", "Workshop search"],
    ["item_type", "https://steamcommunity.com", "工坊条目类型核验", "Workshop item type verification"],
    ["browse", "https://steamcommunity-a.akamaihd.net", "创意工坊搜索", "Workshop search"],
    ["item_type", "https://steamcommunity-a.akamaihd.net", "工坊条目类型核验", "Workshop item type verification"],
    ["details", "https://steamcommunity.com", "工坊条目详情查询", "Workshop item details"],
    ["details", "https://steamcommunity-a.akamaihd.net", "工坊条目详情查询", "Workshop item details"],
    ["details", "https://api.steamchina.com", "工坊条目详情查询", "Workshop item details"],
    ["collection", "https://api.steampowered.com", "工坊合集查询", "Workshop collection lookup"]
  ]) {
    const payload = JSON.stringify({ code: "steam_workshop_network_failed", message: "error sending request",
      stage, origin, reason: "connection", status: null });
    const zh = formatDesktopError(translator("zh-CN"), payload);
    const en = formatDesktopError(translator("en-US"), new Error(payload));
    assert.ok(zh.includes(zhStage));
    assert.ok(en.includes(enStage));
    for (const rendered of [zh, en]) {
      assert.ok(rendered.includes(origin));
      assert.ok(rendered.includes("SteamCMD"));
      assert.doesNotMatch(rendered, /error sending request|steam_workshop_network_failed/);
    }
    assert.match(zh, /代理/);
    assert.doesNotMatch(en, /[\u4e00-\u9fff]/);
    if (origin === "https://steamcommunity.com" || origin === "https://steamcommunity-a.akamaihd.net") {
      assert.match(zh, /Steam 社区或其官方 CDN/);
      assert.match(en, /Steam Community or its official CDN/);
      assert.doesNotMatch(zh, /公开 API/);
      assert.doesNotMatch(en, /public API/);
    } else {
      assert.match(zh, /公开 API/);
      assert.match(en, /public API/);
    }
  }
});

test("Workshop HTTP responses retain their status without a connection diagnosis", () => {
  const payload = { code: "steam_workshop_network_failed", message: "rate limited",
    stage: "browse", origin: "https://steamcommunity.com", reason: "http", status: 429 };
  for (const locale of ["zh-CN", "en-US"]) {
    const rendered = formatDesktopError(translator(locale), payload);
    assert.match(rendered, /HTTP 429/);
    assert.doesNotMatch(rendered, /DNS|SteamCMD|rate limited/);
  }
});

test("Workshop throttling explains the actual cooldown in both languages", () => {
  const payload = { code: "steam_workshop_network_failed", message: "rate limited",
    stage: "item_type", origin: "https://steamcommunity-a.akamaihd.net", reason: "http", status: 429, retry_after_seconds: 120 };
  assert.match(formatDesktopError(translator("zh-CN"), payload), /Steam 暂时限制了请求.*HTTP 429.*等待 120 秒/);
  assert.match(formatDesktopError(translator("en-US"), payload), /temporarily limiting requests.*HTTP 429.*120 seconds/);
  for (const invalid of [-1, 1.5, "120", Number.MAX_SAFE_INTEGER + 1]) {
    assert.doesNotMatch(formatDesktopError(translator("en-US"), { ...payload, retry_after_seconds: invalid }), /Retry after/);
  }
});

test("Workshop error contracts reject unknown fields instead of guessing a diagnosis", () => {
  const payload = { code: "steam_workshop_network_failed", message: "original diagnostic",
    stage: "browse", origin: "https://steamcommunity.com", reason: "timeout", status: null };
  for (const change of [
    { stage: "future_operation" }, { origin: "https://unverified.example" },
    { origin: "https://steamstore-a.akamaihd.net" }, { origin: "https://steamcommunity-a.akamaihd.net.evil.example" },
    { reason: "unknown" }, { reason: "http", status: null }, { reason: "http", status: "429" }
  ]) {
    const error = JSON.stringify({ ...payload, ...change });
    assert.equal(formatDesktopError(translator("zh-CN"), error), error);
  }
});

test("assistant interpretation failures show the localized action boundary without internal model diagnostics", () => {
  const payload = JSON.stringify({ code: "assistant_request_interpretation_failed",
    message: "The assistant could not interpret this request. No operation was executed." });
  assert.equal(formatDesktopError(translator("zh-CN"), new Error(payload)), "暂时没能理解这条请求，请重试。尚未执行任何操作。");
  assert.equal(formatDesktopError(translator("en-US"), payload), "The assistant could not understand this request. Please try again. No operation was performed.");
  assert.doesNotMatch(formatDesktopError(translator("zh-CN"), payload), /assistant_request_interpretation_failed|missing required fields|code|message/);
});

test("the exact assistant conversation schema rejection explains the native and frontend version mismatch", () => {
  const diagnostic = "invalid args `input` for command `assistant_execute_operation`: unknown field `conversationMessages`, expected one of `settings`, `prompt`, `priorRequests`, `context`, `selectedInstanceId`, `selectedModuleId`";
  for (const error of [diagnostic, new Error(diagnostic)]) {
    assert.equal(formatDesktopError(translator("zh-CN"), error), "界面与后台程序版本不一致。请正常退出 LanGame，再通过启动入口重新打开。当前请求尚未执行。");
    assert.equal(formatDesktopError(translator("en-US"), error), "The interface and backend program versions do not match. Exit LanGame normally, then reopen it using the launcher. This request has not been executed.");
  }
  for (const unrelated of [
    diagnostic.replace("assistant_execute_operation", "assistant_confirm_operation"),
    diagnostic.replace("args `input`", "args `request`"),
    diagnostic.replace("unknown field `conversationMessages`", "unknown field `otherField`"),
    diagnostic.replace("unknown field", "missing field"),
    `Server output: ${diagnostic}`,
    "The assistant failed because conversationMessages was not recognized.",
    diagnostic.replace(/, expected one of .+$/, ", expected one of fields"),
  ]) {
    assert.equal(formatDesktopError(translator("zh-CN"), unrelated), unrelated);
    assert.equal(formatDesktopError(translator("en-US"), new Error(unrelated)), unrelated);
  }
});
