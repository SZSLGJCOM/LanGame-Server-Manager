const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

function harness(onRename) {
  const state = [];
  const refs = [];
  const effects = [];
  let stateIndex = 0;
  let refIndex = 0;
  let effectIndex = 0;
  const node = (type, props) => ({ type, props });
  const dependencies = {
    "react/jsx-runtime": { jsx: node, jsxs: node },
    react: {
      useId: () => "rename-help",
      useRef: (initial) => refs[refIndex++] ??= { current: initial },
      useState: (initial) => {
        const index = stateIndex++;
        if (!(index in state)) state[index] = initial;
        return [state[index], (value) => { state[index] = value; }];
      },
      useEffect: (callback, deps) => {
        const index = effectIndex++;
        if (!effects[index] || deps.some((value, position) => effects[index].deps[position] !== value)) {
          effects[index] = { callback, deps, pending: true };
        }
      }
    },
    "../i18n": { useI18n: () => ({ t: (key) => key }) },
    "../app-state": { describeError: (error) => error.message },
    "./backup-rename-action.css": {},
    "./ActivityNotice": { ActivityNotice: "ActivityNotice" }
  };
  const exports = {};
  const filename = path.join(__dirname, "../src/components/BackupRenameAction.tsx");
  vm.runInNewContext(transpileTypeScript(fs.readFileSync(filename, "utf8"), filename), {
    exports,
    require: (id) => {
      assert.ok(Object.hasOwn(dependencies, id), `Unexpected dependency ${id}`);
      return dependencies[id];
    }
  }, { filename });
  const props = { name: "Original backup", onRename };
  return {
    props,
    render() {
      stateIndex = refIndex = effectIndex = 0;
      return exports.BackupRenameAction(props);
    },
    effects() {
      for (const effect of effects) {
        if (effect.pending) { effect.pending = false; effect.callback(); }
      }
    }
  };
}

function find(tree, type) {
  if (tree?.type === type) return tree;
  const children = tree?.props?.children;
  for (const child of Array.isArray(children) ? children.flat(Infinity) : [children]) {
    if (!child || typeof child !== "object") continue;
    const found = find(child, type);
    if (found) return found;
  }
}

const submit = (form) => form.props.onSubmit({ preventDefault() {} });
const settle = () => new Promise((resolve) => setImmediate(resolve));

test("backup rename opens inline, focuses the draft and cancels without a mutation", () => {
  const view = harness(() => assert.fail("cancel must not rename a backup"));
  const trigger = view.render();
  let focused = 0;
  let selected = 0;
  trigger.props.ref.current = { focus() { focused += 1; } };
  trigger.props.onClick();
  let tree = view.render();
  const input = find(tree, "input");
  input.props.ref.current = { focus() { focused += 1; }, select() { selected += 1; } };
  view.effects();
  assert.equal(input.props.value, "Original backup");
  assert.equal(focused, 1);
  assert.equal(selected, 1);
  input.props.onChange({ target: { value: "Unsaved name" } });
  tree = view.render();
  let prevented = false;
  tree.props.onKeyDown({ key: "Escape", nativeEvent: { isComposing: false },
    preventDefault() { prevented = true; }, stopPropagation() {} });
  tree = view.render();
  view.effects();
  assert.equal(prevented, true);
  assert.equal(tree.type, "button");
  assert.equal(focused, 2);
  view.props.name = "Refreshed name";
  view.render().props.onClick();
  assert.equal(find(view.render(), "input").props.value, "Refreshed name");
});

test("rename submits once, disables edits during the request and closes on success", async () => {
  let complete;
  const received = [];
  const view = harness((value) => {
    received.push(value);
    return new Promise((resolve) => { complete = resolve; });
  });
  view.render().props.onClick();
  find(view.render(), "input").props.onChange({ target: { value: "New name" } });
  let tree = view.render();
  submit(tree);
  submit(tree);
  tree = view.render();
  assert.deepEqual(received, ["New name"]);
  assert.equal(tree.props["aria-busy"], true);
  assert.equal(find(tree, "input").props.disabled, true);
  tree.props.onKeyDown({ key: "Escape", nativeEvent: { isComposing: false }, preventDefault() {}, stopPropagation() {} });
  assert.equal(view.render().type, "form", "pending requests cannot be cancelled locally");
  complete(true);
  await settle();
  assert.equal(view.render().type, "button");
});

test("failed rename keeps the draft for retry and renders unexpected errors", async () => {
  let attempts = 0;
  const view = harness(async () => {
    attempts += 1;
    if (attempts === 1) return false;
    throw new Error("Backup name could not be saved");
  });
  view.render().props.onClick();
  find(view.render(), "input").props.onChange({ target: { value: "Retain this draft" } });
  submit(view.render());
  await settle();
  let tree = view.render();
  assert.equal(find(tree, "input").props.value, "Retain this draft");
  assert.equal(tree.props["aria-busy"], false);
  submit(tree);
  await settle();
  tree = view.render();
  const error = tree.props.children.find((child) => child?.type === "ActivityNotice" && child.props.tone === "error");
  assert.equal(error.props.children, "Backup name could not be saved");
  assert.equal(find(tree, "input").props.value, "Retain this draft");
});

test("IME confirmation cannot submit the backup name", () => {
  const view = harness(() => assert.fail("composition must not save"));
  view.render().props.onClick();
  let prevented = false;
  view.render().props.onKeyDown({ key: "Enter", nativeEvent: { isComposing: true }, preventDefault() { prevented = true; } });
  assert.equal(prevented, true);
});

test("archived backup rename cannot enter editing or invoke its callback", async () => {
  const view = harness(() => assert.fail("archive must not rename"));
  view.props.disabled = true;
  const trigger = view.render();
  assert.equal(trigger.props.disabled, true);
  trigger.props.onClick();
  assert.equal(view.render().type, "button");
  delete view.props.onRename;
  view.props.disabled = false;
  assert.equal(view.render().props.disabled, true);
  view.render().props.onClick();
  assert.equal(view.render().type, "button");
});