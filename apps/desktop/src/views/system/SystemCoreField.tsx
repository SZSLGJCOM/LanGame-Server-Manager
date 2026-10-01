import { Canvas, useFrame, useThree } from "@react-three/fiber";
import type { MotionValue } from "motion/react";
import { Component, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import * as THREE from "three";
import { supportsWebGl2 } from "../../webgl-capabilities";
import type { ResourceState } from "../../domain/system-resources";

interface SystemCoreFieldProps {
  resourceState: ResourceState;
  load: number;
  loads: number[];
  activeIndex: number;
  reducedMotion: boolean;
  pointerX: MotionValue<number>;
  pointerY: MotionValue<number>;
}

interface CoreFieldSceneProps extends SystemCoreFieldProps {
  darkTheme: boolean;
}

interface TelemetryParticlesProps {
  load: number;
  reducedMotion: boolean;
  darkTheme: boolean;
}

class CoreFieldErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    return this.state.failed ? null : this.props.children;
  }
}

const VERTEX_SHADER = `
  out vec2 vUv;

  void main() {
    vUv = uv;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`;

const FRAGMENT_SHADER = `
  precision highp float;

  uniform float uTime;
  uniform float uAttention;
  uniform float uLoad;
  uniform float uDark;
  uniform vec3 uSignal;
  uniform vec2 uPointer;
  uniform vec2 uResolution;
  uniform vec4 uMainLoad;
  uniform vec4 uFocus;

  in vec2 vUv;
  out vec4 outColor;

  float hash21(vec2 point) {
    point = fract(point * vec2(123.34, 456.21));
    point += dot(point, point + 45.32);
    return fract(point.x * point.y);
  }

  float noise21(vec2 point) {
    vec2 cell = floor(point);
    vec2 local = fract(point);
    local = local * local * (3.0 - 2.0 * local);
    float a = hash21(cell);
    float b = hash21(cell + vec2(1.0, 0.0));
    float c = hash21(cell + vec2(0.0, 1.0));
    float d = hash21(cell + vec2(1.0, 1.0));
    return mix(mix(a, b, local.x), mix(c, d, local.x), local.y);
  }

  float fbm(vec2 point) {
    float value = 0.0;
    float amplitude = 0.5;
    mat2 rotation = mat2(0.8, -0.6, 0.6, 0.8);
    for (int octave = 0; octave < 4; octave += 1) {
      value += noise21(point) * amplitude;
      point = rotation * point * 2.03 + 7.17;
      amplitude *= 0.5;
    }
    return value;
  }

  float lineBand(float value, float center, float width) {
    return 1.0 - smoothstep(width, width * 2.1, abs(value - center));
  }

  float angleDistance(float first, float second) {
    return abs(atan(sin(first - second), cos(first - second)));
  }

  void main() {
    vec2 point = (vUv - 0.5) * 2.0;
    point.x *= uResolution.x / max(uResolution.y, 1.0);
    point += uPointer * 0.035;

    float radius = length(point);
    float angle = atan(point.y, point.x);
    float flowTime = uTime * (0.18 + uLoad * 0.18);
    float turbulence = fbm(point * 3.15 + vec2(cos(flowTime), sin(flowTime)) * 0.3);

    float spiralPhase = angle * 6.0 - radius * 23.0 + uTime * (0.52 + uLoad * 0.46) + turbulence * 3.2;
    float filament = pow(max(0.0, sin(spiralPhase) * 0.5 + 0.5), 12.0);
    filament *= (1.0 - smoothstep(0.2, 0.98, radius)) * (0.24 + turbulence * 0.76);

    float innerRing = lineBand(radius, 0.39 + sin(angle * 4.0 + flowTime) * 0.012, 0.006);
    float dataRing = lineBand(radius, 0.69 + turbulence * 0.018, 0.008);
    float outerRing = lineBand(radius, 0.91, 0.005);

    float angularGrid = pow(max(0.0, cos(angle * 24.0)), 48.0);
    float radialGrid = pow(max(0.0, cos(radius * 92.0 - turbulence)), 54.0);
    float grid = (angularGrid * 0.55 + radialGrid * 0.45) * (1.0 - smoothstep(0.34, 0.92, radius));

    float dialAngle = atan(point.x, point.y);
    vec4 lobes = vec4(
      1.0 - smoothstep(0.0, 0.86, angleDistance(dialAngle, 5.497787)),
      1.0 - smoothstep(0.0, 0.86, angleDistance(dialAngle, 0.785398)),
      1.0 - smoothstep(0.0, 0.86, angleDistance(dialAngle, 2.356194)),
      1.0 - smoothstep(0.0, 0.86, angleDistance(dialAngle, 3.926991))
    );
    lobes = pow(lobes, vec4(3.2));
    lobes *= (1.0 - smoothstep(0.48, 0.94, radius)) * smoothstep(0.22, 0.48, radius);
    vec4 lobeEnergy = lobes * (0.26 + uMainLoad * 0.84) * (0.58 + uFocus * 0.62);

    float core = exp(-radius * (7.2 - uLoad * 1.4));
    float breathing = 0.82 + sin(uTime * 1.35) * 0.08 * (1.0 - step(0.5, uDark));

    float lobeTotal = max(dot(lobeEnergy, vec4(1.0)), 0.001);

    float pulseCarrier = sin(radius * 44.0 - uTime * (0.85 + uLoad * 0.9) + turbulence * 2.4);
    float phaseSignal = pow(max(0.0, pulseCarrier), 20.0);
    phaseSignal *= smoothstep(0.24, 0.38, radius) * (1.0 - smoothstep(0.42, 0.91, radius));
    phaseSignal *= clamp(lobeTotal * 1.6, 0.18, 1.0);

    vec3 color = uSignal * filament * (0.42 + uLoad * 0.74 + uAttention * 0.18);
    color += uSignal * innerRing * 0.52;
    color += uSignal * dataRing * 0.38;
    color += uSignal * outerRing * 0.32;
    color += uSignal * grid * 0.13;
    color += uSignal * phaseSignal * 0.42;
    color += uSignal * core * 0.34 * breathing;
    color += uSignal * lobeTotal * (0.2 + filament * 0.38 + dataRing * 0.32);

    float circularMask = 1.0 - smoothstep(0.91, 1.0, radius);
    float alpha = filament * 0.3 + innerRing * 0.32 + dataRing * 0.22 + outerRing * 0.2;
    alpha += grid * 0.07 + phaseSignal * 0.2 + core * 0.18 + lobeTotal * 0.11;
    alpha *= circularMask * mix(0.52, 0.92, uDark);
    outColor = linearToOutputTexel(vec4(color, clamp(alpha, 0.0, 0.9)));
  }
`;

function useDarkTheme() {
  const readTheme = () => typeof document !== "undefined" && document.documentElement.dataset.theme === "dark";
  const [darkTheme, setDarkTheme] = useState(readTheme);

  useEffect(() => {
    const root = document.documentElement;
    const observer = new MutationObserver(() => setDarkTheme(readTheme()));
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);

  return darkTheme;
}

function readSignalColor() {
  return getComputedStyle(document.documentElement).getPropertyValue("--shell-muted").trim();
}

function useDocumentVisible() {
  const [visible, setVisible] = useState(() => typeof document === "undefined" || document.visibilityState !== "hidden");

  useEffect(() => {
    const handleVisibility = () => setVisible(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", handleVisibility);
    return () => document.removeEventListener("visibilitychange", handleVisibility);
  }, []);

  return visible;
}

function CoreFieldCamera() {
  const camera = useThree((state) => state.camera);
  const width = useThree((state) => state.size.width);
  const height = useThree((state) => state.size.height);
  const invalidate = useThree((state) => state.invalidate);

  useLayoutEffect(() => {
    if (!(camera instanceof THREE.OrthographicCamera)) {
      return;
    }
    camera.zoom = Math.max(1, Math.min(width, height) / 2);
    camera.updateProjectionMatrix();
    invalidate();
  }, [camera, height, invalidate, width]);

  return null;
}

function CoreFieldWake({ active }: { active: boolean }) {
  const invalidate = useThree((state) => state.invalidate);

  useEffect(() => {
    if (active) {
      invalidate();
    }
  }, [active, invalidate]);

  return null;
}

function attentionIntensity(state: ResourceState) {
  return state === "critical" ? 1 : state === "watch" ? 0.5 : 0;
}

function CoreFieldPlane({
  resourceState,
  load,
  loads,
  activeIndex,
  reducedMotion,
  darkTheme,
  pointerX,
  pointerY
}: CoreFieldSceneProps) {
  const materialRef = useRef<THREE.ShaderMaterial>(null);
  const invalidate = useThree((state) => state.invalidate);
  const initialLoads = [0, 1, 2, 3].map((index) => THREE.MathUtils.clamp(loads[index] ?? 0, 0, 1));
  const uniforms = useMemo(() => ({
    uTime: { value: 0 },
    uAttention: { value: attentionIntensity(resourceState) },
    uLoad: { value: load },
    uDark: { value: darkTheme ? 1 : 0 },
    uSignal: { value: new THREE.Color() },
    uPointer: { value: new THREE.Vector2() },
    uResolution: { value: new THREE.Vector2(1, 1) },
    uMainLoad: { value: new THREE.Vector4(initialLoads[0], initialLoads[1], initialLoads[2], initialLoads[3]) },
    uFocus: { value: new THREE.Vector4(0.72, 0.72, 0.72, 0.72) }
  }), []);

  useLayoutEffect(() => {
    uniforms.uSignal.value.set(readSignalColor());
    invalidate();
  }, [darkTheme, invalidate, uniforms]);

  useEffect(() => {
    invalidate();
  }, [activeIndex, darkTheme, resourceState, invalidate, load, loads, reducedMotion]);

  useFrame((state) => {
    const material = materialRef.current;
    if (!material) {
      return;
    }
    const nextAttention = attentionIntensity(resourceState);
    const nextLoad = THREE.MathUtils.clamp(load, 0, 1);
    const nextMainLoad = [0, 1, 2, 3].map((index) => THREE.MathUtils.clamp(loads[index] ?? 0, 0, 1));
    const nextFocus = [0, 1, 2, 3].map((index) => activeIndex < 0 ? 0.72 : index === activeIndex ? 1 : 0.18);
    const blend = reducedMotion ? 1 : 0.06;
    const focusBlend = reducedMotion ? 1 : 0.08;
    material.uniforms.uTime.value = reducedMotion ? 0 : performance.now() * 0.001;
    material.uniforms.uAttention.value = THREE.MathUtils.lerp(material.uniforms.uAttention.value, nextAttention, reducedMotion ? 1 : 0.035);
    material.uniforms.uLoad.value = THREE.MathUtils.lerp(material.uniforms.uLoad.value, nextLoad, reducedMotion ? 1 : 0.035);
    material.uniforms.uDark.value = THREE.MathUtils.lerp(material.uniforms.uDark.value, darkTheme ? 1 : 0, reducedMotion ? 1 : 0.08);
    const nextPointerX = THREE.MathUtils.clamp(pointerX.get() / 3.5, -1, 1);
    const nextPointerY = THREE.MathUtils.clamp(-pointerY.get() / 3.5, -1, 1);
    const pointerUniform = material.uniforms.uPointer.value;
    const pointerBlend = reducedMotion ? 1 : 0.035;
    pointerUniform.set(
      THREE.MathUtils.lerp(pointerUniform.x, nextPointerX, pointerBlend),
      THREE.MathUtils.lerp(pointerUniform.y, nextPointerY, pointerBlend)
    );
    material.uniforms.uResolution.value.set(state.size.width, state.size.height);
    const mainLoadUniform = material.uniforms.uMainLoad.value;
    mainLoadUniform.set(
      THREE.MathUtils.lerp(mainLoadUniform.x, nextMainLoad[0], blend),
      THREE.MathUtils.lerp(mainLoadUniform.y, nextMainLoad[1], blend),
      THREE.MathUtils.lerp(mainLoadUniform.z, nextMainLoad[2], blend),
      THREE.MathUtils.lerp(mainLoadUniform.w, nextMainLoad[3], blend)
    );
    const focusUniform = material.uniforms.uFocus.value;
    focusUniform.set(
      THREE.MathUtils.lerp(focusUniform.x, nextFocus[0], focusBlend),
      THREE.MathUtils.lerp(focusUniform.y, nextFocus[1], focusBlend),
      THREE.MathUtils.lerp(focusUniform.z, nextFocus[2], focusBlend),
      THREE.MathUtils.lerp(focusUniform.w, nextFocus[3], focusBlend)
    );
  });

  return (
    <mesh frustumCulled={false}>
      <planeGeometry args={[2, 2]} />
      <shaderMaterial
        ref={materialRef}
        uniforms={uniforms}
        vertexShader={VERTEX_SHADER}
        fragmentShader={FRAGMENT_SHADER}
        glslVersion={THREE.GLSL3}
        transparent
        depthWrite={false}
        depthTest={false}
        toneMapped={false}
      />
    </mesh>
  );
}

function TelemetryParticles({ load, reducedMotion, darkTheme }: TelemetryParticlesProps) {
  const pointsRef = useRef<THREE.Points<THREE.BufferGeometry, THREE.PointsMaterial>>(null);
  const positions = useMemo(() => {
    const count = 160;
    const positionData = new Float32Array(count * 3);
    for (let index = 0; index < count; index += 1) {
      const seed = Math.abs(Math.sin(index * 91.733 + 0.17));
      const angle = index * 2.399963 + seed * 0.75;
      const radius = 0.24 + seed * 0.69;
      positionData[index * 3] = Math.cos(angle) * radius;
      positionData[index * 3 + 1] = Math.sin(angle) * radius;
      positionData[index * 3 + 2] = 0.03 + (seed - 0.5) * 0.08;
    }
    return positionData;
  }, []);

  useLayoutEffect(() => {
    pointsRef.current?.material.color.set(readSignalColor());
  }, [darkTheme]);

  useFrame((_, delta) => {
    const points = pointsRef.current;
    if (!points || reducedMotion) {
      return;
    }
    points.rotation.z -= delta * (0.025 + load * 0.035);
    const pulse = 1 + Math.sin(performance.now() * 0.001 * 0.8) * 0.012 * load;
    points.scale.setScalar(pulse);
  });

  return (
    <points ref={pointsRef}>
      <bufferGeometry>
        <bufferAttribute attach="attributes-position" args={[positions, 3]} />
      </bufferGeometry>
      <pointsMaterial
        size={1.25}
        sizeAttenuation
        transparent
        opacity={darkTheme ? 0.56 : 0.28}
        blending={THREE.AdditiveBlending}
        depthWrite={false}
      />
    </points>
  );
}

function CoreFieldScene(props: CoreFieldSceneProps) {
  return (
    <>
      <CoreFieldPlane {...props} />
      <TelemetryParticles load={props.load} reducedMotion={props.reducedMotion} darkTheme={props.darkTheme} />
    </>
  );
}

export function SystemCoreField({
  resourceState,
  load,
  loads,
  activeIndex,
  reducedMotion,
  pointerX,
  pointerY
}: SystemCoreFieldProps) {
  const [supported] = useState(supportsWebGl2);
  const darkTheme = useDarkTheme();
  const documentVisible = useDocumentVisible();
  if (!supported) {
    return null;
  }

  return (
    <div className="system-core-field" aria-hidden="true">
      <CoreFieldErrorBoundary>
        <Canvas
          orthographic
          flat
          fallback={null}
          camera={{ position: [0, 0, 1], zoom: 1 }}
          dpr={[1, 1.5]}
          frameloop={reducedMotion || !documentVisible ? "demand" : "always"}
          gl={{
            alpha: true,
            antialias: false,
            depth: false,
            stencil: false,
            premultipliedAlpha: false,
            powerPreference: "high-performance"
          }}
          onCreated={({ gl }) => {
            gl.setClearColor(0x000000, 0);
            gl.outputColorSpace = THREE.SRGBColorSpace;
            gl.domElement.addEventListener("webglcontextlost", (event) => {
              event.preventDefault();
            });
          }}
        >
          <CoreFieldCamera />
          <CoreFieldWake active={!reducedMotion && documentVisible} />
          <CoreFieldScene
            resourceState={resourceState}
            load={load}
            loads={loads}
            activeIndex={activeIndex}
            reducedMotion={reducedMotion}
            darkTheme={darkTheme}
            pointerX={pointerX}
            pointerY={pointerY}
          />
        </Canvas>
      </CoreFieldErrorBoundary>
    </div>
  );
}
