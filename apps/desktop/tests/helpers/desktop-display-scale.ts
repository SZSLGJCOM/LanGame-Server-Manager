import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";

// Observe the production native policy in the isolated acceptance profile,
// including renderer/browser recovery. There is no frontend zoom controller.
export async function verifyNativeDisplayScale() {
  const window = getCurrentWindow();
  const monitor = await currentMonitor();
  if (!monitor) throw new Error("Native display acceptance requires a monitor");
  const physical = await window.innerSize();
  const scale = Math.max(0.75, Math.min(monitor.size.width / 2560, monitor.size.height / 1440));
  // A preference from the removed manual control must not affect automatic
  // adaptation now or when the real native view is recreated later in the run.
  localStorage.setItem("langame.display-scale", "80");
  const deadline = performance.now() + 5000;
  while (Math.abs(devicePixelRatio - scale) >= 0.02
    || Math.abs(innerWidth * devicePixelRatio - physical.width) >= 3
    || Math.abs(innerHeight * devicePixelRatio - physical.height) >= 3) {
    if (performance.now() >= deadline) throw new Error(`Automatic native display scale did not settle: ${JSON.stringify({
      expected: scale, actual: devicePixelRatio, physical, viewport: [innerWidth, innerHeight],
    })}`);
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  console.log("DESKTOP_AUTO_DISPLAY", JSON.stringify({ scale, physical, viewport: [innerWidth, innerHeight] }));
}
