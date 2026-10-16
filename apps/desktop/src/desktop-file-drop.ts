const MAX_DROPPED_FILES = 256;
const REPLY_TYPE = "lgsm:dropped-files";
const FRAME_NAME = "lgsm-file-drop";
const FRAME_PATH = "/lgsm-file-drop.html";

/** Negotiate a non-destructive operation using only data available during protected drag events. */
export function modDropEffect(transfer: Pick<DataTransfer, "effectAllowed" | "types" | "files" | "items">,
  acceptsReferences: boolean): "copy" | "link" | "none" {
  const types = new Set(Array.from(transfer.types, (type) => type.toLowerCase()));
  const hasFiles = types.has("files") || transfer.files.length > 0
    || Array.from(transfer.items).some((item) => item.kind === "file");
  const hasReferences = acceptsReferences && ["text/plain", "text/uri-list", "text/html"].some((type) => types.has(type));
  if (!hasFiles && !hasReferences) return "none";
  if (["copy", "copyLink", "copyMove", "all", "uninitialized"].includes(transfer.effectAllowed)) return "copy";
  // Browser links can also expose a virtual shortcut or image file. A link
  // operation consumes the reference, never copies or moves that file.
  if (hasReferences && ["link", "linkMove"].includes(transfer.effectAllowed)) return "link";
  // A move acknowledgement may remove source text or files. Importing Mods never authorizes that.
  return "none";
}

interface NativeFileDropBridge {
  postMessageWithAdditionalObjects(message: unknown, objects: ArrayLike<unknown>): void;
  addEventListener(type: "message", handler: (event: MessageEvent<unknown>) => void): void;
  removeEventListener(type: "message", handler: (event: MessageEvent<unknown>) => void): void;
}

type HandleItem = DataTransferItem & {
  getAsFileSystemHandle?: () => Promise<FileSystemHandle | null>;
};

export class DesktopFileDropError extends Error {
  constructor(readonly code: "unavailable" | "too_many", readonly stage?: "handles" | "frame" | "native_reply") {
    super(stage ? `${code} [${stage}]` : code);
  }
}

type FrameOwner = { owner: Window; frame: HTMLIFrameElement; ready: Promise<NativeFileDropBridge> };
let frameOwner: FrameOwner | null = null;

function frameBridge(): Promise<NativeFileDropBridge> {
  if (frameOwner?.owner === window && frameOwner.frame.isConnected) return frameOwner.ready;
  frameOwner?.frame.remove();
  const frame = document.createElement("iframe");
  frame.name = FRAME_NAME;
  frame.hidden = true;
  frame.tabIndex = -1;
  frame.setAttribute("aria-hidden", "true");
  const source = new URL(FRAME_PATH, window.location.href).href;
  const ready = new Promise<NativeFileDropBridge>((resolve, reject) => {
    let settled = false;
    const finish = (bridge?: NativeFileDropBridge) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      frame.removeEventListener("load", loaded);
      frame.removeEventListener("error", failed);
      if (bridge) resolve(bridge);
      else { frame.remove(); reject(new DesktopFileDropError("unavailable", "frame")); }
    };
    const failed = () => finish();
    const loaded = () => {
      try {
        const target = frame.contentWindow as (Window & { chrome?: { webview?: NativeFileDropBridge } }) | null;
        const bridge = target?.chrome?.webview;
        // A missing asset must not pass through a development SPA fallback.
        if (!target || target.location.href !== source || target.document.querySelector('meta[name="lgsm-file-drop"]')?.getAttribute("content") !== "1" ||
          !bridge?.postMessageWithAdditionalObjects) { finish(); return; }
        finish(bridge);
      } catch { finish(); }
    };
    const timer = setTimeout(failed, 5000);
    frame.addEventListener("load", loaded);
    frame.addEventListener("error", failed);
    frame.src = source;
    document.body.appendChild(frame);
  });
  frameOwner = { owner: window, frame, ready };
  return ready;
}

/** Capture items during drop: the browser protects DataTransfer after the event returns. */
export async function droppedFilePaths(transfer: DataTransfer, signal: AbortSignal): Promise<string[]> {
  const files = Array.from(transfer.files);
  const items = Array.from(transfer.items).filter((item) => item.kind === "file") as HandleItem[];
  if (Math.max(files.length, items.length) > MAX_DROPPED_FILES) throw new DesktopFileDropError("too_many");
  if (!files.length && !items.length) return [];
  if (signal.aborted) throw new DOMException("Drop cancelled", "AbortError");

  // Existing non-WebView hosts may expose a native path directly. WebView2 does not.
  const direct = files.map((file) => (file as File & { path?: string }).path);
  if (files.length && direct.every((path): path is string => typeof path === "string" && path.length > 0)) return direct;
  const bridge = (window as Window & { chrome?: { webview?: NativeFileDropBridge } }).chrome?.webview;
  if (!bridge?.postMessageWithAdditionalObjects) throw new DesktopFileDropError("unavailable");

  // WebView2 can recover a dropped File's original file or directory path.
  // Prefer it over FileSystemAccess promises, which can remain pending in a
  // native drop. Capture a handle synchronously only when no File is available.
  const objects = items.length ? items.map((item) => {
    const file = item.getAsFile();
    if (file) return Promise.resolve(file);
    if (item.getAsFileSystemHandle) return item.getAsFileSystemHandle();
    return Promise.resolve(null);
  }) : files.map((file) => Promise.resolve(file));
  const requestId = crypto.randomUUID();
  return new Promise((resolve, reject) => {
    let settled = false;
    let stage: "handles" | "frame" | "native_reply" = "handles";
    const finish = (paths?: string[], error?: Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal.removeEventListener("abort", abort);
      bridge.removeEventListener("message", receive);
      if (error) reject(error); else resolve(paths ?? []);
    };
    const abort = () => finish(undefined, new DOMException("Drop cancelled", "AbortError"));
    const receive = (event: MessageEvent<unknown>) => {
      const reply = event.data;
      if (!reply || typeof reply !== "object" || !("type" in reply) || reply.type !== REPLY_TYPE ||
        !("requestId" in reply) || reply.requestId !== requestId) return;
      if (!("paths" in reply) || !Array.isArray(reply.paths) || !reply.paths.length || reply.paths.length > MAX_DROPPED_FILES ||
        !reply.paths.every((path): path is string => typeof path === "string" && path.length > 0) ||
        ("error" in reply && reply.error != null)) {
        finish(undefined, new DesktopFileDropError("unavailable", "native_reply"));
      } else finish(reply.paths);
    };
    const timer = setTimeout(() => finish(undefined, new DesktopFileDropError("unavailable", stage)), 5000);
    signal.addEventListener("abort", abort, { once: true });
    bridge.addEventListener("message", receive);
    Promise.all(objects).then(async (resolved) => {
      if (settled) return;
      if (resolved.some((object) => !object)) throw new DesktopFileDropError("unavailable", "handles");
      stage = "frame";
      const sender = await frameBridge();
      if (settled) return;
      stage = "native_reply";
      // Keep the original browser objects. The dedicated frame has its own
      // WebMessageReceived channel, independent of Tauri's top-level IPC.
      sender.postMessageWithAdditionalObjects({ type: "lgsm:resolve-dropped-files", requestId }, resolved);
    }).catch(() => finish(undefined, new DesktopFileDropError("unavailable", stage)));
  });
}

/** Unhandled URL/file drops must never navigate away from the desktop application. */
export function preventUnhandledDropNavigation(): () => void {
  const prevent = (event: DragEvent) => {
    if (event.defaultPrevented) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "none";
  };
  window.addEventListener("dragover", prevent);
  window.addEventListener("drop", prevent);
  return () => {
    window.removeEventListener("dragover", prevent);
    window.removeEventListener("drop", prevent);
  };
}
