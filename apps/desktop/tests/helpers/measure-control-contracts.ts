export interface ControlMeasurement { name: string; height: number; radius: number; fontSize: number; lineHeight: number; expectedHeight: number; expectedLineHeight: number; }
export function measureControl(name: string, element: HTMLElement, expectedHeight: number, role: "field" | "action" = "field"): ControlMeasurement {
  const style = getComputedStyle(element);
  return { name, height: element.getBoundingClientRect().height, radius: parseFloat(style.borderTopLeftRadius),
    fontSize: parseFloat(style.fontSize), lineHeight: parseFloat(style.lineHeight), expectedHeight, expectedLineHeight: role === "action" ? 16.25 : 18.2 };
}
export function controlContractViolations(measurements: ControlMeasurement[]): string[] {
  return measurements.flatMap((value) => {
    const violations: string[] = [];
    if (![value.height, value.radius, value.fontSize, value.lineHeight].every(Number.isFinite)) {
      return [`${value.name}: computed control metrics must be finite`];
    }
    if (Math.abs(value.height - value.expectedHeight) > 0.5) violations.push(`${value.name}: height ${value.height}, expected ${value.expectedHeight}`);
    if (Math.abs(value.radius - 8) > 0.1) violations.push(`${value.name}: radius ${value.radius}, expected 8`);
    if (Math.abs(value.fontSize - 13) > 0.1) violations.push(`${value.name}: font ${value.fontSize}, expected 13`);
    if (Math.abs(value.lineHeight - value.expectedLineHeight) > 0.1) violations.push(`${value.name}: line height ${value.lineHeight}, expected ${value.expectedLineHeight}`);
    return violations;
  });
}
