import {
  AnimatePresence,
  MotionConfig,
  motion,
  useMotionValue,
  useReducedMotion,
  useSpring
} from "motion/react";
import {
  useId,
  useMemo,
  type CSSProperties,
  type PointerEvent
} from "react";
import { SystemCoreField } from "./SystemCoreField";
import type { ResourceState } from "../../domain/system-resources";
import "./system-core-dial.css";

export type SystemCoreTone = "cpu" | "memory" | "disk" | "network";
export type SystemCoreTelemetryState = ResourceState;

export interface SystemCoreInteraction {
  activeTone: SystemCoreTone | null;
  pinnedTone: SystemCoreTone | null;
  onHoverChange: (tone: SystemCoreTone, active: boolean) => void;
  onFocusChange: (tone: SystemCoreTone, active: boolean) => void;
  onPinToggle: (tone: SystemCoreTone) => void;
}

export interface SystemCoreBand {
  tone: SystemCoreTone;
  label: string;
  value: string;
  unit?: string;
  detail: string;
  micro?: string;
  percent: number | null;
  start: number;
  end: number;
  telemetryState?: SystemCoreTelemetryState;
  telemetryLabel?: string;
}

interface SystemCoreDialProps {
  resourceLabel: string;
  stateLabel: string;
  sampleLabel: string;
  sampleStateLabel: string;
  hideIdleSampleState?: boolean;
  lockedLabel: string;
  rimCaption: string;
  operatingState: ResourceState;
  mainBands: SystemCoreBand[];
  channelBands: SystemCoreBand[];
  interaction: SystemCoreInteraction;
}

const SVG_CENTER = 500;

export const CORE_DIAL_SECTORS = [
  { start: 280, end: 350 },
  { start: 10, end: 80 },
  { start: 100, end: 170 },
  { start: 190, end: 260 }
] as const;

function clampPercent(value: number | null) {
  if (!Number.isFinite(value)) {
    return 0;
  }
  return Math.max(0, Math.min(100, value ?? 0));
}

function polarPoint(radius: number, angleDeg: number) {
  const angle = (angleDeg * Math.PI) / 180;
  return {
    x: SVG_CENTER + Math.sin(angle) * radius,
    y: SVG_CENTER - Math.cos(angle) * radius
  };
}

function arcPath(radius: number, start: number, end: number) {
  const first = polarPoint(radius, start);
  const last = polarPoint(radius, end);
  const largeArc = Math.abs(end - start) > 180 ? 1 : 0;
  return [
    `M ${first.x.toFixed(2)} ${first.y.toFixed(2)}`,
    `A ${radius} ${radius} 0 ${largeArc} 1 ${last.x.toFixed(2)} ${last.y.toFixed(2)}`
  ].join(" ");
}

function annularSectorPath(inner: number, outer: number, start: number, end: number) {
  const outerStart = polarPoint(outer, start);
  const outerEnd = polarPoint(outer, end);
  const innerEnd = polarPoint(inner, end);
  const innerStart = polarPoint(inner, start);
  const largeArc = Math.abs(end - start) > 180 ? 1 : 0;
  return [
    `M ${outerStart.x.toFixed(2)} ${outerStart.y.toFixed(2)}`,
    `A ${outer} ${outer} 0 ${largeArc} 1 ${outerEnd.x.toFixed(2)} ${outerEnd.y.toFixed(2)}`,
    `L ${innerEnd.x.toFixed(2)} ${innerEnd.y.toFixed(2)}`,
    `A ${inner} ${inner} 0 ${largeArc} 0 ${innerStart.x.toFixed(2)} ${innerStart.y.toFixed(2)}`,
    "Z"
  ].join(" ");
}

function bandProgressPoint(band: SystemCoreBand, radius: number) {
  const padding = 5;
  const angle = band.start + padding + (band.end - band.start - padding * 2) * (clampPercent(band.percent) / 100);
  return polarPoint(radius, angle);
}

function SystemCoreDialContent({
  resourceLabel,
  stateLabel,
  sampleLabel,
  sampleStateLabel,
  hideIdleSampleState = false,
  lockedLabel,
  rimCaption,
  operatingState,
  mainBands,
  channelBands,
  interaction
}: SystemCoreDialProps) {
  const reduceMotion = useReducedMotion() ?? false;
  const { activeTone, pinnedTone, onHoverChange, onFocusChange, onPinToggle } = interaction;
  const activeBand = mainBands.find((item) => item.tone === activeTone) ?? null;
  const coreLoads = mainBands.map((item) => clampPercent(item.percent) / 100);
  const peakLoad = Math.max(0, ...coreLoads);
  const activeIndex = mainBands.findIndex((item) => item.tone === activeTone);
  const rawId = useId();
  const idPrefix = useMemo(() => `system-core-${rawId.replace(/[^a-z0-9_-]/gi, "")}`, [rawId]);
  const ids = {
    disc: `${idPrefix}-disc`,
    center: `${idPrefix}-center`,
    caption: `${idPrefix}-caption`
  };
  const pointerX = useMotionValue(0);
  const pointerY = useMotionValue(0);
  const springX = useSpring(pointerX, { stiffness: 130, damping: 24, mass: 0.55 });
  const springY = useSpring(pointerY, { stiffness: 130, damping: 24, mass: 0.55 });

  const resetPointer = () => {
    pointerX.set(0);
    pointerY.set(0);
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (reduceMotion) {
      return;
    }
    const bounds = event.currentTarget.getBoundingClientRect();
    pointerX.set(((event.clientX - bounds.left) / bounds.width - 0.5) * 7);
    pointerY.set(((event.clientY - bounds.top) / bounds.height - 0.5) * 7);
  };

  return (
    <div
      className={`system-core-visual ${reduceMotion ? "is-reduced-motion" : ""}`.trim()}
      role="group"
      aria-label={`${resourceLabel}: ${stateLabel}. ${sampleLabel}`}
      onPointerMove={handlePointerMove}
      onPointerLeave={resetPointer}
    >
      <motion.div className="system-core-parallax" style={{ x: springX, y: springY }}>
        <SystemCoreField
          resourceState={operatingState}
          load={peakLoad}
          loads={coreLoads}
          activeIndex={activeIndex}
          reducedMotion={reduceMotion}
          pointerX={springX}
          pointerY={springY}
        />
        <motion.svg
          className="system-core-dial"
          viewBox="0 0 1000 1000"
          aria-hidden="true"
          focusable="false"
          initial={reduceMotion ? false : { opacity: 0, scale: 0.965 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ duration: 0.72, ease: [0.22, 1, 0.36, 1] }}
        >
          <defs>
            <radialGradient id={ids.disc} cx="50%" cy="44%" r="60%">
              <stop offset="0%" stopColor="var(--system-core-disc-stop-0)" />
              <stop offset="52%" stopColor="var(--system-core-disc-stop-1)" />
              <stop offset="82%" stopColor="var(--system-core-disc-stop-2)" />
              <stop offset="100%" stopColor="var(--system-core-disc-stop-3)" />
            </radialGradient>
            <radialGradient id={ids.center} cx="50%" cy="38%" r="64%">
              <stop offset="0%" stopColor="var(--system-core-center-stop-0)" />
              <stop offset="56%" stopColor="var(--system-core-center-stop-1)" />
              <stop offset="100%" stopColor="var(--system-core-center-stop-2)" />
            </radialGradient>
            <path id={ids.caption} d={arcPath(461, 300, 420)} />
          </defs>

          <circle className="system-core-disc" cx="500" cy="500" r="492" fill={`url(#${ids.disc})`} />
          <circle className="system-core-bezel" cx="500" cy="500" r="480" />
          <circle className="system-core-bezel system-core-bezel--inner" cx="500" cy="500" r="458" />

          <g className="system-core-calibration" aria-hidden="true">
            <circle className="system-core-calibration-ring is-outer" cx="500" cy="500" r="474" />
            <circle className="system-core-calibration-ring is-inner" cx="500" cy="500" r="312" />
            {Array.from({ length: 48 }, (_, index) => {
              const angle = index * 7.5;
              const major = index % 4 === 0;
              const start = polarPoint(major ? 457 : 466, angle);
              const end = polarPoint(major ? 487 : 480, angle);
              return (
                <motion.line
                  key={index}
                  className={`system-core-tick ${major ? "is-major" : ""}`.trim()}
                  x1={start.x}
                  y1={start.y}
                  x2={end.x}
                  y2={end.y}
                  initial={reduceMotion ? false : { opacity: 0 }}
                  animate={{ opacity: major ? 0.92 : 0.42 }}
                  transition={{ delay: reduceMotion ? 0 : 0.08 + index * 0.008, duration: 0.2 }}
                />
              );
            })}
          </g>

          <g className="system-core-band-layer">
            {mainBands.map((item, index) => {
              const channel = channelBands.find((candidate) => candidate.tone === item.tone);
              const style = { ["--band-accent" as string]: `var(--system-${item.tone})` } as CSSProperties;
              const mainArc = arcPath(432, item.start + 5, item.end - 5);
              const channelArc = arcPath(291, item.start + 5, item.end - 5);
              const marker = channel ? bandProgressPoint(channel, 291) : polarPoint(291, item.start + 5);
              const dimmed = Boolean(activeTone && activeTone !== item.tone);
              const active = activeTone === item.tone;
              return (
                <motion.g
                  key={item.tone}
                  className={`system-core-band system-core-band--${item.tone} ${active ? "is-active" : ""}`.trim()}
                  style={style}
                  animate={{ opacity: dimmed ? 0.34 : 1 }}
                  transition={{ duration: reduceMotion ? 0 : 0.18 }}
                >
                  <motion.path
                    className="system-core-sector"
                    d={annularSectorPath(322, 450, item.start, item.end)}
                    initial={reduceMotion ? false : { opacity: 0 }}
                    animate={{ opacity: active ? 1 : 0.78 }}
                    transition={{ delay: reduceMotion ? 0 : 0.12 + index * 0.07, duration: 0.46 }}
                  />
                  <path className="system-core-band-track" d={mainArc} />
                  {item.percent !== null ? <motion.path
                    className="system-core-band-progress"
                    d={mainArc}
                    initial={reduceMotion ? false : { pathLength: 0, opacity: 0 }}
                    animate={{ pathLength: clampPercent(item.percent) / 100, opacity: 1 }}
                    transition={reduceMotion ? { duration: 0 } : {
                      pathLength: { type: "spring", stiffness: 72, damping: 18, mass: 0.75, delay: index * 0.055 },
                      opacity: { duration: 0.2 }
                    }}
                  /> : null}
                  <path className="system-core-channel-track" d={channelArc} />
                  {channel && channel.percent !== null ? (
                    <>
                      <motion.path
                        className="system-core-channel-progress"
                        d={channelArc}
                        initial={reduceMotion ? false : { pathLength: 0, opacity: 0 }}
                        animate={{ pathLength: clampPercent(channel.percent) / 100, opacity: 0.88 }}
                        transition={reduceMotion ? { duration: 0 } : {
                          pathLength: { duration: 0.64, delay: 0.22 + index * 0.05, ease: [0.22, 1, 0.36, 1] },
                          opacity: { duration: 0.2 }
                        }}
                      />
                      <motion.circle
                        className="system-core-channel-node"
                        cx={marker.x}
                        cy={marker.y}
                        r={active ? 7 : 5}
                        initial={false}
                        animate={{ cx: marker.x, cy: marker.y, r: active ? 7 : 5 }}
                        transition={reduceMotion
                          ? { duration: 0 }
                          : { type: "spring", stiffness: 95, damping: 20 }}
                      />
                    </>
                  ) : null}
                </motion.g>
              );
            })}
          </g>

          <AnimatePresence initial={false}>
            {activeBand ? (
              <motion.g
                key={activeBand.tone}
                className="system-core-phase-lock"
                style={{ ["--band-accent" as string]: `var(--system-${activeBand.tone})` } as CSSProperties}
                initial={reduceMotion ? false : { opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: reduceMotion ? 0 : 0.16 }}
                aria-hidden="true"
              >
                <motion.path
                  className="system-core-phase-halo"
                  d={arcPath(468, activeBand.start + 7, activeBand.end - 7)}
                  initial={reduceMotion ? false : { pathLength: 0, pathOffset: 0.5 }}
                  animate={{ pathLength: 1, pathOffset: 0 }}
                  transition={reduceMotion ? { duration: 0 } : { duration: 0.46, ease: [0.22, 1, 0.36, 1] }}
                />
                <motion.path
                  className="system-core-phase-rail"
                  d={arcPath(468, activeBand.start + 7, activeBand.end - 7)}
                  initial={reduceMotion ? false : { pathLength: 0 }}
                  animate={{ pathLength: 1 }}
                  transition={reduceMotion ? { duration: 0 } : { duration: 0.34, ease: [0.22, 1, 0.36, 1] }}
                />
                <motion.circle
                  className="system-core-phase-node"
                  cx={polarPoint(468, activeBand.end - 7).x}
                  cy={polarPoint(468, activeBand.end - 7).y}
                  initial={reduceMotion ? false : { r: 0 }}
                  animate={{ r: 6 }}
                  transition={reduceMotion ? { duration: 0 } : { delay: 0.2, type: "spring", stiffness: 180, damping: 18 }}
                />
              </motion.g>
            ) : null}
          </AnimatePresence>

          <g className="system-core-reactor" aria-hidden="true">
            <circle className="system-core-reactor-shell" cx="500" cy="500" r="222" fill={`url(#${ids.center})`} />
            <circle className="system-core-reactor-orbit is-outer" cx="500" cy="500" r="207" />
            <circle className="system-core-reactor-orbit is-inner" cx="500" cy="500" r="178" />
            <path className="system-core-reactor-bracket" d={arcPath(194, 304, 342)} />
            <path className="system-core-reactor-bracket" d={arcPath(194, 34, 72)} />
            <path className="system-core-reactor-bracket" d={arcPath(194, 124, 162)} />
            <path className="system-core-reactor-bracket" d={arcPath(194, 214, 252)} />
          </g>

          <text className="system-core-rim-caption">
            <textPath href={`#${ids.caption}`} startOffset="50%" textAnchor="middle">
              {rimCaption}
            </textPath>
          </text>
        </motion.svg>

        {mainBands.map((item, index) => {
          const channel = channelBands.find((candidate) => candidate.tone === item.tone);
          const style = {
            ["--band-accent" as string]: `var(--system-${item.tone})`,
            ["--core-value-length" as string]: Math.max(1, item.value.length)
          } as CSSProperties;
          const selected = pinnedTone === item.tone;
          const telemetryState = item.telemetryState ?? "unknown";
          const telemetryLabel = item.telemetryLabel ?? sampleLabel;
          return (
            <motion.button
              key={item.tone}
              type="button"
              className={`system-core-metric system-core-metric--${item.tone}`}
              data-position={index}
              style={style}
              aria-label={`${item.label}: ${item.value}${item.unit ?? ""}. ${telemetryLabel}. ${item.detail}${channel ? `. ${channel.label}: ${channel.value}${channel.unit ?? ""}` : ""}`}
              aria-pressed={selected}
              onPointerEnter={() => onHoverChange(item.tone, true)}
              onPointerLeave={() => onHoverChange(item.tone, false)}
              onFocus={(event) => {
                if (event.currentTarget.matches(":focus-visible")) {
                  onFocusChange(item.tone, true);
                }
              }}
              onBlur={() => onFocusChange(item.tone, false)}
              onClick={() => onPinToggle(item.tone)}
              initial={reduceMotion ? false : { opacity: 0, y: index < 2 ? -10 : 10 }}
              animate={{
                opacity: activeTone && activeTone !== item.tone ? 0.46 : 1,
                y: 0,
                scale: selected ? 1.035 : 1
              }}
              whileHover={reduceMotion ? undefined : { scale: 1.035 }}
              whileFocus={reduceMotion ? undefined : { scale: 1.025 }}
              transition={{ duration: reduceMotion ? 0 : 0.2 }}
            >
              <span className="system-core-metric-label">{item.label}</span>
              <span className="system-core-metric-value" data-wide-unit={Boolean(item.unit && item.unit.length > 1)}>
                <motion.strong key={`${item.tone}-${item.value}`} initial={reduceMotion ? false : { opacity: 0.42, y: 4 }} animate={{ opacity: 1, y: 0 }}>
                  {item.value}
                </motion.strong>
                {item.unit ? <em>{item.unit}</em> : null}
              </span>
              <span className={`system-core-metric-live is-${telemetryState}`}><i /> {telemetryLabel}</span>
            </motion.button>
          );
        })}

        <motion.div
          className={`system-core-center ${activeBand ? "is-focused" : ""} is-${activeBand?.telemetryState ?? operatingState}`.trim()}
          style={{ ["--core-value-length" as string]: Math.max(1, activeBand?.value.length ?? stateLabel.length) } as CSSProperties}
          initial={reduceMotion ? false : { opacity: 0, scale: 0.9 }}
          animate={{ opacity: 1, scale: 1 }}
          transition={{ delay: reduceMotion ? 0 : 0.3, duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
          aria-hidden="true"
        >
          <span className="system-core-center-kicker">{activeBand?.label || resourceLabel}</span>
          <span className={`system-core-center-value ${activeBand ? "" : "is-resource-state"}`.trim()} data-wide-unit={Boolean(activeBand?.unit && activeBand.unit.length > 1)}>
            {activeBand ? (
              <motion.strong key={`${activeBand.tone}-${activeBand.value}`} initial={reduceMotion ? false : { opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }}>
                {activeBand.value}
              </motion.strong>
            ) : (
              <strong>{stateLabel}</strong>
            )}
            {activeBand?.unit ? <em>{activeBand.unit}</em> : null}
          </span>
          {activeBand || (!hideIdleSampleState && operatingState !== "normal") ? <span className="system-core-center-state">
            <i />
            {pinnedTone ? lockedLabel : activeBand?.telemetryLabel || sampleStateLabel}
          </span> : null}
        </motion.div>
      </motion.div>
    </div>
  );
}

export function SystemCoreDial(props: SystemCoreDialProps) {
  return (
    <MotionConfig reducedMotion="user">
      <SystemCoreDialContent {...props} />
    </MotionConfig>
  );
}
