const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { transpileTypeScript } = require("../scripts/typescript_source_tools.cjs");

const root = path.resolve(__dirname, "..", "..", "..");
const desktopRoot = path.join(root, "apps", "desktop");

require.extensions[".ts"] = function compileTypeScript(module, filename) {
  const source = fs.readFileSync(filename, "utf8");
  const outputText = transpileTypeScript(source, filename);
  module._compile(outputText, filename);
};

function readSource(...segments) {
  return fs.readFileSync(path.join(root, ...segments), "utf8");
}

function loadPortPresentation() {
  return require(path.join(desktopRoot, "src", "views", "settings", "instance-port-presentation.ts"));
}

function loadConnectivitySelection() {
  return require(path.join(desktopRoot, "src", "views", "settings", "instance-connectivity-selection.ts"));
}

test("ARK Evolved bundled port contract keeps peer equal to game plus one through either edit", () => {
  const manifest = spawnSync("python", ["-c", [
    "import json, pathlib, tomllib",
    "manifest = tomllib.loads(pathlib.Path('modules/arksurvivalevolved/module.toml').read_text(encoding='utf-8-sig'))",
    "print(json.dumps(manifest))"
  ].join("; ")], { cwd: root, encoding: "utf8" });
  assert.equal(manifest.status, 0, manifest.stderr || manifest.stdout);
  const module = JSON.parse(manifest.stdout);
  const { alignInstancePortGroups } = loadPortPresentation();
  let current = module.default_ports;
  for (const [name, port, expectedGame] of [["game", 28000, 28000], ["peer", 29001, 29000]]) {
    current = alignInstancePortGroups(current,
      current.map((binding) => binding.name === name ? { ...binding, port } : binding),
      module.runtime.port_groups ?? []);
    assert.equal(current.find((binding) => binding.name === "game").port, expectedGame);
    assert.equal(current.find((binding) => binding.name === "peer").port, expectedGame + 1);
    assert.equal(current.find((binding) => binding.name === "query").port, 27015);
    assert.equal(current.find((binding) => binding.name === "rcon").port, 27020);
  }
});

test("instance ports are partitioned by module role without changing the complete draft", () => {
  const { partitionInstancePorts } = loadPortPresentation();
  const ports = [
    { name: "game", protocol: "udp", port: 7777 },
    { name: "rcon", protocol: "tcp", port: 27020 },
    { name: "metrics", protocol: "tcp", port: 9090 }
  ];
  const roles = [
    { role: "player", port_names: ["game"] },
    { role: "service", port_names: ["rcon"] }
  ];

  const partitioned = partitionInstancePorts(ports, roles);

  assert.deepEqual(partitioned.player, [ports[0]]);
  assert.deepEqual(partitioned.service, [ports[1]]);
  assert.deepEqual(partitioned.unclassified, [ports[2]]);
  assert.deepEqual(ports.map((port) => port.name), ["game", "rcon", "metrics"]);
});

test("persisted network comparison normalizes bind and ignores port ordering", () => {
  const { networkDraftMatchesPersisted } = loadPortPresentation();
  const details = {
    summary: { bind_ip: "192.168.31.51" },
    ports: [
      { name: "game", protocol: "udp", port: 7777 },
      { name: "rcon", protocol: "tcp", port: 27020 }
    ]
  };

  assert.equal(
    networkDraftMatchesPersisted(details, " 192.168.31.51 ", [
      { name: "rcon", protocol: "TCP", port: 27020 },
      { name: "game", protocol: "UDP", port: 7777 }
    ]),
    true
  );
  assert.equal(networkDraftMatchesPersisted(details, "192.168.31.52", details.ports), false);
  assert.equal(
    networkDraftMatchesPersisted(details, "192.168.31.51", [
      { name: "game", protocol: "udp", port: 7778 },
      { name: "rcon", protocol: "tcp", port: 27020 }
    ]),
    false
  );
});

test("legacy empty persisted ports display defaults without registering them", () => {
  const {
    materializeInstancePortEdit,
    networkDraftMatchesPersisted,
    resolveVisibleInstancePorts
  } = loadPortPresentation();
  const defaults = [
    { name: "game", protocol: "udp", port: 7777 },
    { name: "query", protocol: "udp", port: 27015 }
  ];
  const details = { summary: { bind_ip: "0.0.0.0" }, ports: [] };

  assert.deepEqual(resolveVisibleInstancePorts(details.ports, defaults), defaults);
  assert.equal(networkDraftMatchesPersisted(details, "0.0.0.0", []), true);
  assert.equal(networkDraftMatchesPersisted(details, "0.0.0.0", defaults), false);
  assert.deepEqual(
    materializeInstancePortEdit(details.ports, defaults, defaults[1], 27016),
    [defaults[0], { ...defaults[1], port: 27016 }]
  );
});

test("fixed-offset port groups follow edits from either member without splitting the block", () => {
  const { alignInstancePortGroups } = loadPortPresentation();
  const group = {
    id: "game_query",
    members: ["game", "query"],
    member_offsets: { game: 0, query: 1 }
  };
  const current = [
    { name: "game", protocol: "udp", port: 27015 },
    { name: "query", protocol: "udp", port: 27016 }
  ];

  const editedGame = alignInstancePortGroups(current, [
    { ...current[0], port: 28000 },
    current[1]
  ], [group]);
  assert.deepEqual(editedGame, [
    { name: "game", protocol: "udp", port: 28000 },
    { name: "query", protocol: "udp", port: 28001 }
  ]);

  const editedQuery = alignInstancePortGroups(editedGame, [
    editedGame[0],
    { ...editedGame[1], port: 29001 }
  ], [group]);
  assert.deepEqual(editedQuery, [
    { name: "game", protocol: "udp", port: 29000 },
    { name: "query", protocol: "udp", port: 29001 }
  ]);

  const clamped = alignInstancePortGroups(editedQuery, [
    { ...editedQuery[0], port: 65535 },
    editedQuery[1]
  ], [group]);
  assert.deepEqual(clamped, [
    { name: "game", protocol: "udp", port: 65534 },
    { name: "query", protocol: "udp", port: 65535 }
  ]);

  const lowerClamped = alignInstancePortGroups(current, [
    current[0],
    { ...current[1], port: 0 }
  ], [group]);
  assert.deepEqual(lowerClamped, [
    { name: "game", protocol: "udp", port: 1 },
    { name: "query", protocol: "udp", port: 2 }
  ]);
});

test("port groups without offsets keep their existing shared-number behavior", () => {
  const { alignInstancePortGroups } = loadPortPresentation();
  const current = [
    { name: "game", protocol: "udp", port: 7777 },
    { name: "game_tcp", protocol: "tcp", port: 7777 }
  ];

  assert.deepEqual(
    alignInstancePortGroups(current, [current[0], { ...current[1], port: 7000 }], [
      { id: "game_transport", members: ["game", "game_tcp"] }
    ]),
    [
      { name: "game", protocol: "udp", port: 7000 },
      { name: "game_tcp", protocol: "tcp", port: 7000 }
    ]
  );
});

test("persisted port acknowledgements advance the baseline without replacing a newer draft", () => {
  const {
    createInstancePortRegistrationState,
    instancePortRegistrationIsDirty,
    reduceInstancePortRegistrationState
  } = loadPortPresentation();

  const persistedInitial = [{ name: "game", protocol: "udp", port: 7777 }];
  const draftA = [{ name: "game", protocol: "udp", port: 7778 }];
  const draftB = [{ name: "game", protocol: "udp", port: 7779 }];
  let state = createInstancePortRegistrationState(persistedInitial);

  state = reduceInstancePortRegistrationState(state, {
    type: "draft-edited",
    ports: draftA,
    defaultPorts: [],
    portGroups: []
  });
  const submittedA = state.draftPorts;
  state = reduceInstancePortRegistrationState(state, {
    type: "draft-edited",
    ports: draftB,
    defaultPorts: [],
    portGroups: []
  });
  state = reduceInstancePortRegistrationState(state, {
    type: "baseline-received",
    ports: submittedA
  });

  assert.deepEqual(state.draftPorts, draftB);
  assert.equal(instancePortRegistrationIsDirty(state), true);

  state = reduceInstancePortRegistrationState(state, {
    type: "baseline-received",
    ports: draftB
  });

  assert.deepEqual(state.draftPorts, draftB);
  assert.equal(instancePortRegistrationIsDirty(state), false);
});

test("a stale acknowledgement cannot roll back a locally authoritative port draft", () => {
  const {
    createInstancePortRegistrationState,
    instancePortRegistrationIsDirty,
    reduceInstancePortRegistrationState
  } = loadPortPresentation();
  const persistedInitial = [{ name: "game", protocol: "udp", port: 7777 }];
  const draftA = [{ name: "game", protocol: "udp", port: 7778 }];
  const draftB = [{ name: "game", protocol: "udp", port: 7779 }];
  let state = createInstancePortRegistrationState(persistedInitial);

  state = reduceInstancePortRegistrationState(state, {
    type: "draft-edited",
    ports: draftA,
    defaultPorts: [],
    portGroups: []
  });
  state = reduceInstancePortRegistrationState(state, {
    type: "draft-edited",
    ports: draftB,
    defaultPorts: [],
    portGroups: []
  });
  state = reduceInstancePortRegistrationState(state, {
    type: "baseline-received",
    ports: draftB
  });
  assert.equal(instancePortRegistrationIsDirty(state), false);

  state = reduceInstancePortRegistrationState(state, {
    type: "baseline-received",
    ports: draftA
  });

  assert.deepEqual(state.baselinePorts, draftA);
  assert.deepEqual(state.draftPorts, draftB);
  assert.equal(instancePortRegistrationIsDirty(state), true);
});

test("a clean modal-session draft stays stable when its server baseline changes externally", () => {
  const {
    createInstancePortRegistrationState,
    instancePortRegistrationIsDirty,
    networkDraftMatchesPersisted,
    reduceInstancePortRegistrationState
  } = loadPortPresentation();
  const sessionDraft = [{ name: "game", protocol: "udp", port: 7777 }];
  const externalBaseline = [{ name: "game", protocol: "udp", port: 7780 }];
  let state = createInstancePortRegistrationState(sessionDraft);

  state = reduceInstancePortRegistrationState(state, {
    type: "baseline-received",
    ports: externalBaseline
  });

  assert.deepEqual(state.baselinePorts, externalBaseline);
  assert.deepEqual(state.draftPorts, sessionDraft);
  assert.equal(instancePortRegistrationIsDirty(state), true);
  assert.equal(
    networkDraftMatchesPersisted(
      { summary: { bind_ip: "0.0.0.0" }, ports: externalBaseline },
      "0.0.0.0",
      state.draftPorts
    ),
    false
  );
});

test("legacy defaults materialize completely and keep later edits across an earlier acknowledgement", () => {
  const {
    createInstancePortRegistrationState,
    instancePortRegistrationIsDirty,
    materializeInstancePortEdit,
    reduceInstancePortRegistrationState
  } = loadPortPresentation();
  const defaults = [
    { name: "game", protocol: "udp", port: 7777 },
    { name: "query", protocol: "udp", port: 27015 }
  ];
  let state = createInstancePortRegistrationState([]);

  const draftA = materializeInstancePortEdit(state.draftPorts, defaults, defaults[0], 7778);
  state = reduceInstancePortRegistrationState(state, {
    type: "draft-edited",
    ports: draftA,
    defaultPorts: defaults,
    portGroups: []
  });
  const submittedA = state.draftPorts;
  const draftB = materializeInstancePortEdit(state.draftPorts, defaults, defaults[1], 27016);
  state = reduceInstancePortRegistrationState(state, {
    type: "draft-edited",
    ports: draftB,
    defaultPorts: defaults,
    portGroups: []
  });
  state = reduceInstancePortRegistrationState(state, {
    type: "baseline-received",
    ports: submittedA
  });

  assert.deepEqual(submittedA, [
    { name: "game", protocol: "udp", port: 7778 },
    { name: "query", protocol: "udp", port: 27015 }
  ]);
  assert.deepEqual(state.draftPorts, [
    { name: "game", protocol: "udp", port: 7778 },
    { name: "query", protocol: "udp", port: 27016 }
  ]);
  assert.equal(instancePortRegistrationIsDirty(state), true);

  state = reduceInstancePortRegistrationState(state, {
    type: "baseline-received",
    ports: state.draftPorts
  });
  assert.equal(instancePortRegistrationIsDirty(state), false);
});

test("clean port drafts adopt newly registered or removed map bindings", () => {
  const { createInstancePortRegistrationState, reduceInstancePortRegistrationState, instancePortRegistrationIsDirty } = loadPortPresentation();
  const main = { name: "game", protocol: "udp", port: 7777 };
  const map = { name: "map-scorched-game", protocol: "udp", port: 7787 };
  let state = createInstancePortRegistrationState([main]);
  state = reduceInstancePortRegistrationState(state, { type: "baseline-received", ports: [main, map] });
  assert.deepEqual(state.draftPorts, [main, map]);
  assert.equal(instancePortRegistrationIsDirty(state), false);
  state = reduceInstancePortRegistrationState(state, { type: "baseline-received", ports: [main] });
  assert.deepEqual(state.draftPorts, [main]);
  assert.equal(instancePortRegistrationIsDirty(state), false);
});

test("map topology acknowledgements preserve existing port edits and merge authoritative map identities", () => {
  const { createInstancePortRegistrationState, reduceInstancePortRegistrationState, instancePortRegistrationIsDirty } = loadPortPresentation();
  const main = { name: "game", protocol: "udp", port: 7777 };
  const removed = { name: "map-scorched-rcon", protocol: "tcp", port: 27030 };
  const retained = { name: "map-center-rcon", protocol: "tcp", port: 27040 };
  const added = { name: "map-aberration-rcon", protocol: "tcp", port: 27050 };
  let state = createInstancePortRegistrationState([main, removed, retained]);
  state = reduceInstancePortRegistrationState(state, { type: "draft-edited", ports: [
    { ...main, port: 7797 }, { ...removed, port: 28030 }, { ...retained, port: 28040 }
  ], defaultPorts: [], portGroups: [] });
  state = reduceInstancePortRegistrationState(state, { type: "baseline-received", ports: [main, retained, added] });
  assert.deepEqual(state.draftPorts, [{ ...main, port: 7797 }, { ...retained, port: 28040 }, added]);
  assert.equal(instancePortRegistrationIsDirty(state), true);
});

test("non-strict bind support disables bind independently from port editing", () => {
  const { instanceNetworkControlState } = loadPortPresentation();

  assert.deepEqual(instanceNetworkControlState(false, false), {
    bindDisabled: true,
    portsDisabled: false
  });
  assert.deepEqual(instanceNetworkControlState(false, true), {
    bindDisabled: false,
    portsDisabled: false
  });
  assert.deepEqual(instanceNetworkControlState(true, true), {
    bindDisabled: true,
    portsDisabled: true
  });
});

test("an active runtime owns the launch-effective network settings", () => {
  const { instanceRuntimeOwnsNetworkSettings } = loadPortPresentation();

  assert.equal(
    instanceRuntimeOwnsNetworkSettings({ summary: { status: "Stopped" }, active_run: null }),
    false
  );
  for (const status of ["Starting", "Running", "Stopping"]) {
    assert.equal(
      instanceRuntimeOwnsNetworkSettings({ summary: { status }, active_run: null }),
      true,
      status
    );
  }
  assert.equal(
    instanceRuntimeOwnsNetworkSettings({ summary: { status: "Error" }, active_run: { run_id: 7 } }),
    true
  );
});

test("only the primary player join port requires a non-zero value", () => {
  const { minimumInstancePortValue } = loadPortPresentation();

  assert.equal(minimumInstancePortValue("player", true), 1);
  assert.equal(minimumInstancePortValue("player", false), 0);
  assert.equal(minimumInstancePortValue("service", true), 0);
  assert.equal(minimumInstancePortValue(null, true), 0);
  assert.equal(minimumInstancePortValue(null, true, true), 1);
  assert.equal(minimumInstancePortValue("service", false, true), 1);
});

test("player join address follows the selected network route", () => {
  const { resolveSelectedJoinEndpoint } = loadConnectivitySelection();
  const endpoints = [
    { address: "26.10.0.8", endpoint: "26.10.0.8:7777", kind: "overlay", label: "Radmin VPN" },
    { address: "192.168.31.51", endpoint: "192.168.31.51:7777", kind: "lan", label: "LAN" }
  ];

  assert.equal(resolveSelectedJoinEndpoint(endpoints, "192.168.31.51"), endpoints[1]);
  assert.equal(resolveSelectedJoinEndpoint(endpoints, "missing"), endpoints[0]);
  assert.equal(resolveSelectedJoinEndpoint([], "192.168.31.51"), null);
});

test("the unified configuration workspace mounts infrastructure only from built-in sections", () => {
  const source = readSource("apps", "desktop", "src", "views", "settings", "ConfigurationWorkspace.tsx");

  assert.match(source, /<GuidedSettingsForm[\s\S]{0,300}?selectedSectionId=\{selectedSectionId\}/);
  assert.match(source, /activeNode\.builtInEditor === "instance-network"[\s\S]{0,200}?<InstanceConnectionSettingsPanel/);
  assert.doesNotMatch(source, /instance-runtime|InstanceRuntimeSettingsPanel/);
  assert.equal(source.match(/<InstanceConnectionSettingsPanel/g)?.length ?? 0, 1);
  assert.match(source, /<InstanceConnectionSettingsPanel[\s\S]{0,500}?disabled=\{editorDisabled\}/);
  assert.doesNotMatch(source, /RoomSettingsPanel|BindAddressSelect/);
});

test("network panel keeps persisted endpoint and independent disabled contracts", () => {
  const networkPanel = readSource("apps", "desktop", "src", "views", "settings", "InstanceConnectionSettingsPanel.tsx");
  const listenSelect = readSource("apps", "desktop", "src", "views", "settings", "ListenAddressSelect.tsx");
  const joinSelect = readSource("apps", "desktop", "src", "views", "settings", "PlayerJoinAddressSelect.tsx");
  const portFields = readSource("apps", "desktop", "src", "views", "settings", "InstancePortFields.tsx");
  const portHook = readSource("apps", "desktop", "src", "views", "settings", "useInstancePortRegistration.ts");

  assert.match(networkPanel, /partitionInstancePorts\(visiblePorts,/);
  assert.match(networkPanel, /networkDraftMatchesPersisted\(props\.details, props\.bindIp, props\.ports\)/);
  assert.match(networkPanel, /settings\.network\.playerPorts/);
  assert.match(networkPanel, /instanceRuntimeOwnsNetworkSettings\(props\.details\)/);
  assert.match(networkPanel, /settings\.network\.runtimeLock/);
  assert.match(networkPanel, /localPortDraftDirty/);
  assert.match(networkPanel, /copyDisabled=\{!networkPersisted \|\| localPortDraftDirty\}/);
  assert.match(networkPanel, /supportsStrictBindAddress \? \([\s\S]{0,300}?<ListenAddressSelect/);
  assert.match(networkPanel, /disabled=\{controlState\.bindDisabled\}/);
  assert.match(networkPanel, /disabled=\{controlState\.portsDisabled\}/);
  assert.doesNotMatch(networkPanel, /bindIpUnsupported|bindIpStrictHint|form-note/);
  assert.doesNotMatch(listenSelect, /buildJoinEndpoint|navigator\.clipboard|copyDisabledHint/);
  assert.match(joinSelect, /buildShareEndpoints\(props\.details, props\.candidates,/);
  assert.match(joinSelect, /resolveSelectedJoinEndpoint\(endpoints, selectedAddress\)/);
  assert.match(joinSelect, /navigator\.clipboard\.writeText\(selectedEndpoint\.endpoint\)/);
  assert.match(joinSelect, /window\.localStorage\.setItem/);
  assert.match(portFields, /onPortDraftDirtyChange/);
  assert.match(joinSelect, /setCopyState\("idle"\)/);
  assert.doesNotMatch(portHook, /initialDefaultPorts/);
  assert.doesNotMatch(portHook, /registeredPorts\.length > 0 \? registeredPorts : defaultPorts/);
});

test("opening infrastructure settings has no render-time change callback", () => {
  const retiredRoomPanelPath = path.join(desktopRoot, "src", "views", "settings", "RoomSettingsPanel.tsx");
  const listenSelect = readSource("apps", "desktop", "src", "views", "settings", "ListenAddressSelect.tsx");

  assert.equal(fs.existsSync(retiredRoomPanelPath), false);
  assert.doesNotMatch(listenSelect, /useEffect\([\s\S]{0,300}?props\.onChange/);
});
