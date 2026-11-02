const assert = require("node:assert/strict");
const fs = require("node:fs");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

require.extensions[".ts"] = (module, filename) => {
  module._compile(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), filename);
};
const { createAppPathSettingsDraft, createAppPathSelection, createAppPathMigration, updateAppPathDraft, appPathSettingsEqual } = require("../src/views/system/app-path-settings-draft.ts");

const configured = Object.freeze({
  games_root: "D:\\LanGame\\games",
  servers_root: "E:\\LanGame\\instances",
  archives_root: "E:\\LanGame\\instances\\.trash",
  steamcmd_root: "D:\\LanGame\\steamcmd"
});

test("changing a workspace drafts its default archives together without mutating persisted settings", () => {
  const draft = updateAppPathDraft(configured, "servers_root", " D:\\LanGame\\instances ");
  assert.deepEqual(draft, { ...configured, servers_root: "D:\\LanGame\\instances", archives_root: "D:\\LanGame\\instances\\.trash" });
  assert.equal(configured.servers_root, "E:\\LanGame\\instances");
  assert.equal(appPathSettingsEqual(draft, configured), false);
  assert.equal(appPathSettingsEqual(updateAppPathDraft(draft, "servers_root", configured.servers_root), configured), true);
});

test("a changed workspace always uses its canonical trash path with the chosen separator", () => {
  for (const [target, expected] of [
    ["D:\\Instances\\", "D:\\Instances\\.trash"], ["D:/Instances/", "D:/Instances/.trash"],
    ["/data/instances/", "/data/instances/.trash"], ["\\\\Host\\Share\\Instances\\", "\\\\Host\\Share\\Instances\\.trash"]
  ]) {
    for (const archives_root of ["e:/LANGAME/instances/.TRASH/", "E:\\Retained archives", "E:/LanGame/instances/recovery/custom"]) {
      const current = { ...configured, archives_root };
      const next = updateAppPathDraft(current, "servers_root", target);
      assert.equal(next.archives_root, expected);
      assert.equal(next.servers_root, target);
      assert.equal(next.games_root, current.games_root); assert.equal(next.steamcmd_root, current.steamcmd_root);
    }
  }
});

test("server-file and SteamCMD selections preserve every other effective location including external archives", () => {
  for (const key of ["games_root", "steamcmd_root"]) {
    const current = Object.freeze({ ...configured, archives_root: "E:\\Retained archives" });
    assert.deepEqual(createAppPathSelection(current, key, " D:\\Selected "), { ...current, [key]: "D:\\Selected" });
  }
});

test("selecting the existing workspace never relocates its external archives", () => {
  const current = Object.freeze({ ...configured, archives_root: "E:\\Retained archives" });
  for (const target of [configured.servers_root, " E:\\LanGame\\instances\\ ", "e:/LANGAME/instances", "E:/LanGame/./instances/../instances"]) {
    assert.deepEqual(createAppPathSelection(current, "servers_root", target), current);
  }
});

test("Windows long-path aliases of the current workspace preserve external archives", () => {
  for (const [ordinary, verbatim] of [
    ["E:\\LanGame\\instances", "\\\\?\\E:\\LanGame\\instances"],
    ["\\\\Host\\Share\\Instances", "\\\\?\\UNC\\host\\share\\instances"]
  ]) {
    for (const [servers_root, selected] of [[ordinary, verbatim], [verbatim, ordinary]]) {
      const current = Object.freeze({ ...configured, servers_root, archives_root: "E:\\External archives" });
      assert.deepEqual(createAppPathSelection(current, "servers_root", selected), current);
      const retry = { ...current, servers_root: selected, archives_root: "D:\\Ignored archive target", steamcmd_root: "D:\\New SteamCMD" };
      assert.deepEqual(createAppPathMigration(current, retry), { ...retry, archives_root: current.archives_root });
    }
  }
});

test("missing archives resolve to the effective default before editing another setting", () => {
  for (const archives_root of [undefined, "  "]) {
    const draft = createAppPathSettingsDraft({ ...configured, archives_root });
    assert.equal(draft.archives_root, "E:\\LanGame\\instances\\.trash");
    assert.equal(updateAppPathDraft(draft, "games_root", "D:\\Programs").archives_root, draft.archives_root);
    assert.equal(updateAppPathDraft(draft, "servers_root", "D:\\Instances").archives_root, "D:\\Instances\\.trash");
  }
});

test("Unix workspace identity remains case-sensitive while new targets use canonical archives", () => {
  const draft = { ...configured, servers_root: "/srv/instances", archives_root: "/external/Archives" };
  assert.deepEqual(updateAppPathDraft(draft, "servers_root", "/srv/instances/"), draft);
  assert.equal(updateAppPathDraft(draft, "servers_root", "/srv/Instances").archives_root, "/srv/Instances/.trash");
  assert.equal(updateAppPathDraft(draft, "servers_root", "/data/instances").archives_root, "/data/instances/.trash");
});

test("moving a workspace replaces its custom nested archive destination with the new default", () => {
  const draft = { ...configured, archives_root: "E:\\LanGame\\instances\\recovery\\custom-archives" };
  const next = updateAppPathDraft(draft, "servers_root", "D:\\Moved instances");
  assert.equal(next.archives_root, "D:\\Moved instances\\.trash");
  assert.equal(next.games_root, draft.games_root); assert.equal(next.steamcmd_root, draft.steamcmd_root);
});

test("changing server files never relocates other configured roles even when they are descendants", () => {
  const draft = Object.freeze({ games_root: "E:\\Managed", servers_root: "E:\\Managed\\instances",
    archives_root: "E:\\Managed\\instances\\archive", steamcmd_root: "E:\\Managed\\tools\\steamcmd" });
  const next = updateAppPathDraft(draft, "games_root", "D:/Destination/");
  assert.deepEqual(next, { ...draft, games_root: "D:/Destination/" });
  assert.deepEqual(updateAppPathDraft(next, "games_root", "F:/Final"), { ...draft, games_root: "F:/Final" });
});

test("moving a workspace carries its archives but leaves nested server files and SteamCMD independent", () => {
  const draft = Object.freeze({ games_root: "E:/Managed/games", servers_root: "E:/Managed",
    archives_root: "E:/Managed/recovery/archive", steamcmd_root: "E:/Managed/tools/steamcmd" });
  assert.deepEqual(updateAppPathDraft(draft, "servers_root", "D:/Instances"), {
    ...draft, servers_root: "D:/Instances", archives_root: "D:/Instances/.trash"
  });
});

test("selecting SteamCMD never relocates another configured role", () => {
  const draft = Object.freeze({ games_root: "E:/Managed/games", servers_root: "E:/Managed/instances",
    archives_root: "E:/Managed/archive", steamcmd_root: "E:/Managed" });
  assert.deepEqual(updateAppPathDraft(draft, "steamcmd_root", " D:/Selected "), { ...draft, steamcmd_root: "D:/Selected" });
});

test("workspace migration binds archives without depending on their old lexical ancestry", () => {
  for (const archives_root of ["e:/root", "E:/Root-other/archive", "E:/RootBackup/archive", "E:/Root/../external-archives"]) {
    const draft = { ...configured, servers_root: "E:/Root", archives_root };
    assert.deepEqual(updateAppPathDraft(draft, "servers_root", "D:/Root"), {
      ...draft, servers_root: "D:/Root", archives_root: "D:/Root/.trash"
    });
  }
});

test("UNC source archives also follow the new workspace regardless of the old share", () => {
  for (const archives_root of ["\\\\HOST\\SHARE\\LGSM\\Instances\\Archives", "//host/other/lgsm/Archives"]) {
    const draft = { ...configured, servers_root: "//host/share/lgsm/Instances", archives_root };
    assert.deepEqual(updateAppPathDraft(draft, "servers_root", "D:\\Moved"), {
      ...draft, servers_root: "D:\\Moved", archives_root: "D:\\Moved\\.trash"
    });
  }
});

test("new selections normalize effective settings before replacing the selected role", () => {
  const settings = { games_root: " D:/Games ", servers_root: " E:/Instances ", archives_root: undefined,
    steamcmd_root: " E:/SteamCMD " };
  assert.deepEqual(createAppPathSelection(settings, "games_root", " F:/Games "), {
    games_root: "F:/Games", servers_root: "E:/Instances", archives_root: "E:/Instances/.trash", steamcmd_root: "E:/SteamCMD"
  });
});

test("relocating a workspace binds every existing archive location to the new workspace trash", () => {
  for (const archives_root of ["E:/External archives", "E:/LanGame/instances/recovery/custom", "E:/LanGame/instances/.TRASH"]) {
    const current = Object.freeze({ ...configured, archives_root });
    assert.deepEqual(createAppPathSelection(current, "servers_root", " F:/Relocated instances/ "), {
      ...current, servers_root: "F:/Relocated instances/", archives_root: "F:/Relocated instances/.trash"
    });
    assert.equal(current.archives_root, archives_root);
  }
});


test("old failed or ready workspace inputs migrate their retained external archive to the new workspace", () => {
  const current = Object.freeze({ ...configured, archives_root: "E:/External archives" });
  for (const archives_root of [current.archives_root, "D:/Previous target/custom-archives"]) {
    const submitted = Object.freeze({ games_root: "F:/Selected games", servers_root: "D:/New instances/",
      archives_root, steamcmd_root: "F:/Selected steamcmd" });
    assert.deepEqual(createAppPathMigration(current, submitted), {
      ...submitted, archives_root: "D:/New instances/.trash"
    });
    assert.equal(submitted.archives_root, archives_root);
    assert.equal(current.archives_root, "E:/External archives");
  }
});

test("a retired archive-only retry cannot replace the currently effective archive path", () => {
  const current = Object.freeze({ ...configured, archives_root: "E:/External archives" });
  const submitted = Object.freeze({ ...current, archives_root: "D:/Obsolete standalone archive" });
  assert.deepEqual(createAppPathMigration(current, submitted), current);
  assert.equal(submitted.archives_root, "D:/Obsolete standalone archive");
});

test("SteamCMD retries preserve the current external archive while keeping all other submitted targets", () => {
  const current = Object.freeze({ ...configured, archives_root: "E:/Current external archives" });
  const submitted = Object.freeze({ ...configured, games_root: "F:/Requested games", steamcmd_root: "F:/Requested steamcmd" });
  assert.deepEqual(createAppPathMigration(current, submitted), { ...submitted, archives_root: current.archives_root });
  assert.equal(submitted.archives_root, configured.archives_root);
});

test("retry normalization preserves the submitted workspace spelling when it is the same directory", () => {
  const current = { ...configured, archives_root: " E:/Current external archives " };
  const submitted = { games_root: " F:/Requested games ", servers_root: " e:/LANGAME/instances/ ",
    archives_root: "D:/Ignored archives", steamcmd_root: " F:/Requested steamcmd " };
  assert.deepEqual(createAppPathMigration(current, submitted), {
    games_root: "F:/Requested games", servers_root: "e:/LANGAME/instances/",
    archives_root: "E:/Current external archives", steamcmd_root: "F:/Requested steamcmd"
  });
  const missing = { ...current, archives_root: undefined };
  assert.equal(createAppPathMigration(missing, submitted).archives_root, "E:\\LanGame\\instances\\.trash");
});

test("new selections and repeated retry normalization retain the same full migration targets", () => {
  for (const key of ["games_root", "servers_root", "steamcmd_root"]) {
    const selected = createAppPathSelection(configured, key, " F:/Selected target/ ");
    const normalized = createAppPathMigration(configured, selected);
    assert.deepEqual(normalized, selected);
    assert.deepEqual(createAppPathMigration(configured, normalized), normalized);
  }
});

test("selecting a drive or share root resolves each role into its managed LanGame folder", () => {
  const folders = { games_root: "server-files", servers_root: "instances", steamcmd_root: "cmd/steamcmd" };
  for (const selected of [" D:\\ ", "d:/", "\\\\?\\D:\\", "\\\\Host\\Share", "//host/share/", "\\\\?\\UNC\\Host\\Share\\"]) {
    const root = selected.trim();
    const separator = root.includes("\\") ? "\\" : "/";
    for (const [key, folder] of Object.entries(folders)) {
      const target = `${root.replace(/[\\/]+$/, "")}${separator}LanGame${separator}${folder.replaceAll("/", separator)}`;
      const current = { ...configured, archives_root: "E:\\External archives" };
      const next = createAppPathSelection(current, key, selected);
      assert.deepEqual(next, { ...current, [key]: target,
        archives_root: key === "servers_root" ? `${target}${separator}.trash` : current.archives_root });
      assert.deepEqual(createAppPathMigration(current, next), next);
    }
  }
});

test("a drive selection that resolves to the current role is a no-op despite path spelling", () => {
  const current = Object.freeze({ ...configured, games_root: "d:/LanGame\\server-files", servers_root: "d:/LanGame\\instances",
    archives_root: "D:\\External archives", steamcmd_root: "d:/LanGame\\cmd\\steamcmd" });
  for (const key of ["games_root", "servers_root", "steamcmd_root"]) {
    assert.deepEqual(createAppPathSelection(current, key, "D:\\"), current);
    assert.deepEqual(createAppPathMigration(current, { ...current, [key]: "D:\\" }), current);
  }
});

test("retrying an old volume-root failure resolves all selected roles before binding archives", () => {
  const current = Object.freeze({ ...configured, archives_root: "E:\\External archives" });
  const submitted = Object.freeze({ games_root: "D:\\", servers_root: "F:\\", archives_root: "F:\\.trash", steamcmd_root: "G:/" });
  assert.deepEqual(createAppPathMigration(current, submitted), {
    games_root: "D:\\LanGame\\server-files", servers_root: "F:\\LanGame\\instances",
    archives_root: "F:\\LanGame\\instances\\.trash", steamcmd_root: "G:/LanGame/cmd/steamcmd"
  });
  assert.equal(submitted.servers_root, "F:\\");
});

test("root selection resolution preserves specific folders and leaves malformed paths for native validation", () => {
  for (const selected of ["D:\\Chosen", "D:/LanGame", "\\\\Host\\Share\\Chosen", "D:", "relative", "\\", "\\\\server",
    "\\\\.\\PhysicalDrive0", "\\\\?\\GLOBALROOT\\Device", "D:\\Parent\\..\\", "D:\\."]) {
    assert.equal(createAppPathSelection(configured, "steamcmd_root", selected).steamcmd_root, selected);
  }
});

test("retry conversion never rewrites an unchanged configured volume root", () => {
  const current = { ...configured, games_root: "D:\\" };
  const submitted = { ...current, steamcmd_root: "F:\\Tools" };
  assert.deepEqual(createAppPathMigration(current, submitted), submitted);
  assert.deepEqual(createAppPathSettingsDraft(current), current);
});
