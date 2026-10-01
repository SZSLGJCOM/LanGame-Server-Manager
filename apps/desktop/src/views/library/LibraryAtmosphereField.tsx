import { Canvas, useFrame, useThree } from "@react-three/fiber";
import type { MotionValue } from "motion/react";
import { useReducedMotion } from "motion/react";
import { Component, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import * as THREE from "three";
import { supportsWebGl2 } from "../../webgl-capabilities";
import { getModuleArtTheme, resolveModuleMediaFallbackSources, type ModuleArtTheme } from "../../module-art";
import { loadMediaImage } from "../../load-media-image";
import { useI18n, type LocaleCode } from "../../i18n";

interface LibraryAtmosphereFieldProps {
  moduleId: string;
  moduleName: string;
  mediaKey: string;
  imageSrc: string | null;
  mode: "catalog" | "detail";
  pointerX: MotionValue<number>;
  pointerY: MotionValue<number>;
  pointerActivity: MotionValue<number>;
}

interface LibraryAtmosphereSceneProps extends LibraryAtmosphereFieldProps {
  locale: LocaleCode;
  darkTheme: boolean;
  documentVisible: boolean;
  reducedMotion: boolean;
}

interface LoadedTexture {
  texture: THREE.Texture;
  size: THREE.Vector2;
  key: string;
}

const TRANSITION_DURATION_MS = 620;
const POINTER_SETTLE_MS = 480;
const MOTIF_INDEX: Record<ModuleArtTheme["motif"], number> = {
  embers: 0,
  grid: 1,
  forest: 2,
  signal: 3,
  hazard: 4,
  night: 5
};

class LibraryAtmosphereFieldBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
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
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;

const FRAGMENT_SHADER = `
  precision highp float;

  uniform sampler2D uCurrentTexture;
  uniform sampler2D uNextTexture;
  uniform vec2 uCurrentSize;
  uniform vec2 uNextSize;
  uniform vec2 uResolution;
  uniform vec2 uPointer;
  uniform vec3 uCurrentAccent;
  uniform vec3 uNextAccent;
  uniform vec3 uCurrentSurface;
  uniform vec3 uNextSurface;
  uniform float uTransition;
  uniform float uTime;
  uniform float uPointerActivity;
  uniform float uDark;
  uniform float uMode;
  uniform float uMotif;

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

  vec2 coverUv(vec2 uv, vec2 imageSize, vec2 viewportSize) {
    float imageAspect = imageSize.x / max(imageSize.y, 1.0);
    float viewportAspect = viewportSize.x / max(viewportSize.y, 1.0);
    if (imageAspect > viewportAspect) {
      uv.x = (uv.x - 0.5) * viewportAspect / imageAspect + 0.5;
    } else {
      uv.y = (uv.y - 0.5) * imageAspect / viewportAspect + 0.5;
    }
    return uv;
  }

  float line(float value, float width) {
    return 1.0 - smoothstep(width, width * 2.1, abs(value));
  }

  float motifWeight(float index) {
    return max(1.0 - abs(uMotif - index), 0.0);
  }

  void main() {
    vec2 aspect = vec2(uResolution.x / max(uResolution.y, 1.0), 1.0);
    vec2 pointerUv = uPointer * 0.5 + 0.5;
    vec2 pointerDelta = (vUv - pointerUv) * aspect;
    float pointerDistance = length(pointerDelta);
    float lens = exp(-pointerDistance * pointerDistance * 16.0) * uPointerActivity;
    vec2 lensDirection = normalize(pointerDelta + vec2(0.0001));

    float transitionNoise = fbm(vUv * vec2(5.2, 3.6) + vec2(uTime * 0.035, -uTime * 0.021));
    float transitionSignal = transitionNoise * 0.62 + vUv.x * 0.23 + (1.0 - vUv.y) * 0.15;
    float transitionMask = smoothstep(transitionSignal - 0.13, transitionSignal + 0.13, uTransition);
    float transitionEdge = 1.0 - smoothstep(0.0, 0.06, abs(transitionSignal - uTransition));
    transitionEdge *= step(0.002, uTransition) * step(uTransition, 0.998);

    vec2 currentUv = coverUv(vUv + lensDirection * lens * 0.006, uCurrentSize, uResolution);
    vec2 nextUv = coverUv(vUv - lensDirection * lens * 0.008, uNextSize, uResolution);
    float displacement = (transitionNoise - 0.5) * transitionEdge * 0.018;
    currentUv += vec2(displacement, -displacement * 0.45);
    nextUv -= vec2(displacement * 0.72, displacement * 0.35);

    vec3 currentImage = texture(uCurrentTexture, clamp(currentUv, 0.001, 0.999)).rgb;
    vec3 nextImage = texture(uNextTexture, clamp(nextUv, 0.001, 0.999)).rgb;
    vec3 image = mix(currentImage, nextImage, transitionMask);
    vec3 accent = mix(uCurrentAccent, uNextAccent, transitionMask);
    vec3 surface = mix(uCurrentSurface, uNextSurface, transitionMask);

    float luminance = dot(image, vec3(0.2126, 0.7152, 0.0722));
    image = mix(vec3(luminance), image, mix(0.72, 0.94, uDark));

    vec2 fieldPoint = (vUv - 0.5) * aspect;
    float radius = length(fieldPoint);
    float fieldNoise = fbm(fieldPoint * 3.4 + vec2(uTime * 0.026, -uTime * 0.018));
    vec2 microScale = vec2(28.0, 16.0);
    vec2 microCellId = floor(vUv * microScale);
    vec2 microCell = fract(vUv * microScale + vec2(fieldNoise * 0.32, 0.0)) - 0.5;
    float cellSeed = hash21(microCellId);
    float cellGate = smoothstep(0.62, 0.9, cellSeed);
    float gridLines = max(
      line(abs(microCell.x) - 0.46, 0.018),
      line(abs(microCell.y) - 0.46, 0.018)
    ) * cellGate;
    float contour = line(fract((fieldNoise + fieldPoint.y * 0.1) * 11.0) - 0.5, 0.022);
    float signal = line(microCell.y, 0.026)
      * (1.0 - smoothstep(0.08, 0.38, abs(microCell.x)))
      * cellGate;
    float hazard = line(abs(microCell.y) - (0.1 + abs(microCell.x) * 0.38), 0.022)
      * step(0.72, cellSeed);
    float ember = pow(max(fieldNoise - 0.58, 0.0), 3.0) * 5.2;
    float night = pow(max(0.0, cellSeed - 0.93), 4.0) * 18.0;
    float circuitry =
      ember * motifWeight(0.0)
      + gridLines * motifWeight(1.0)
      + contour * motifWeight(2.0)
      + signal * motifWeight(3.0)
      + hazard * motifWeight(4.0)
      + night * motifWeight(5.0);
    circuitry *= 0.008 + lens * 0.11 + transitionEdge * 0.1;
    float lensGrain = pow(max(
      noise21((vUv - pointerUv) * vec2(180.0, 104.0) + vec2(uTime * 0.18, -uTime * 0.11)) - 0.56,
      0.0
    ), 3.0) * lens * 5.5;

    float vignette = 1.0 - smoothstep(0.26, 1.08, radius);
    float edgeFade = smoothstep(0.0, 0.12, vUv.y) * smoothstep(0.0, 0.16, 1.0 - vUv.y);
    float imageStrength = mix(0.2, 0.42, uDark) * mix(0.82, 1.0, uMode);
    vec3 color = mix(surface, image, imageStrength + luminance * 0.08);
    color += accent * circuitry;
    color += accent * lensGrain * 0.22;
    color += accent * transitionEdge * (0.08 + fieldNoise * 0.12);
    color += image * lens * 0.08;

    float alpha = mix(0.28, 0.6, uDark) * (0.68 + vignette * 0.32);
    alpha += circuitry * 0.34 + lensGrain * 0.12 + transitionEdge * 0.12;
    alpha *= edgeFade;
    outColor = linearToOutputTexel(vec4(color, clamp(alpha, 0.0, 0.82)));
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

function useDocumentVisible() {
  const [visible, setVisible] = useState(() => typeof document === "undefined" || document.visibilityState !== "hidden");

  useEffect(() => {
    const handleVisibility = () => setVisible(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", handleVisibility);
    return () => document.removeEventListener("visibilitychange", handleVisibility);
  }, []);

  return visible;
}

function configureTexture(texture: THREE.Texture, key: string): LoadedTexture {
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.wrapS = THREE.ClampToEdgeWrapping;
  texture.wrapT = THREE.ClampToEdgeWrapping;
  texture.magFilter = THREE.LinearFilter;
  texture.minFilter = THREE.LinearFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  const image = texture.image as { naturalWidth?: number; naturalHeight?: number; width?: number; height?: number } | undefined;
  return {
    texture,
    size: new THREE.Vector2(image?.naturalWidth ?? image?.width ?? 1, image?.naturalHeight ?? image?.height ?? 1),
    key
  };
}

function createNeutralTexture() {
  const texture = new THREE.DataTexture(new Uint8Array([4, 7, 10, 255]), 1, 1, THREE.RGBAFormat);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.needsUpdate = true;
  return texture;
}

function AtmospherePlane(props: LibraryAtmosphereSceneProps) {
  const materialRef = useRef<THREE.ShaderMaterial>(null);
  const currentTextureRef = useRef<LoadedTexture | null>(null);
  const nextTextureRef = useRef<LoadedTexture | null>(null);
  const generationRef = useRef(0);
  const transitionStartedAtRef = useRef(0);
  const transitioningRef = useRef(false);
  const activeUntilRef = useRef(0);
  const neutralTexture = useMemo(createNeutralTexture, []);
  const invalidate = useThree((state) => state.invalidate);
  const theme = getModuleArtTheme(props.moduleId, props.moduleName);
  const nextAccent = useMemo(() => new THREE.Color(theme.accent), [theme.accent]);
  const nextSurface = useMemo(() => new THREE.Color(theme.surface), [theme.surface]);
  const uniforms = useMemo(() => ({
    uCurrentTexture: { value: neutralTexture },
    uNextTexture: { value: neutralTexture },
    uCurrentSize: { value: new THREE.Vector2(1, 1) },
    uNextSize: { value: new THREE.Vector2(1, 1) },
    uResolution: { value: new THREE.Vector2(1, 1) },
    uPointer: { value: new THREE.Vector2() },
    uCurrentAccent: { value: nextAccent.clone() },
    uNextAccent: { value: nextAccent.clone() },
    uCurrentSurface: { value: nextSurface.clone() },
    uNextSurface: { value: nextSurface.clone() },
    uTransition: { value: 1 },
    uTime: { value: 0 },
    uPointerActivity: { value: 0 },
    uDark: { value: props.darkTheme ? 1 : 0 },
    uMode: { value: props.mode === "detail" ? 1 : 0 },
    uMotif: { value: MOTIF_INDEX[theme.motif] }
  }), []);

  useEffect(() => {
    const wake = () => {
      if (!props.documentVisible || props.reducedMotion) {
        return;
      }
      activeUntilRef.current = performance.now() + POINTER_SETTLE_MS;
      invalidate();
    };
    const unsubscribeX = props.pointerX.on("change", wake);
    const unsubscribeY = props.pointerY.on("change", wake);
    const unsubscribeActivity = props.pointerActivity.on("change", wake);
    return () => {
      unsubscribeX();
      unsubscribeY();
      unsubscribeActivity();
    };
  }, [invalidate, props.documentVisible, props.pointerActivity, props.pointerX, props.pointerY, props.reducedMotion]);

  useEffect(() => {
    const material = materialRef.current;
    if (!material) {
      return;
    }
    material.uniforms.uMotif.value = MOTIF_INDEX[theme.motif];
    material.uniforms.uMode.value = props.mode === "detail" ? 1 : 0;
    material.uniforms.uDark.value = props.darkTheme ? 1 : 0;
    activeUntilRef.current = performance.now() + TRANSITION_DURATION_MS;
    invalidate();
  }, [invalidate, nextAccent, nextSurface, props.darkTheme, props.mode, theme.motif]);

  useEffect(() => {
    const generation = ++generationRef.current;
    const targetKey = `${props.moduleId}:${props.mediaKey}`;

    const install = (loaded: LoadedTexture) => {
      if (generation !== generationRef.current) {
        loaded.texture.dispose();
        return;
      }
      const material = materialRef.current;
      if (!material) {
        loaded.texture.dispose();
        return;
      }

      if (!currentTextureRef.current || props.reducedMotion) {
        currentTextureRef.current?.texture.dispose();
        nextTextureRef.current?.texture.dispose();
        currentTextureRef.current = loaded;
        nextTextureRef.current = null;
        transitioningRef.current = false;
        material.uniforms.uCurrentTexture.value = loaded.texture;
        material.uniforms.uNextTexture.value = loaded.texture;
        material.uniforms.uCurrentSize.value.copy(loaded.size);
        material.uniforms.uNextSize.value.copy(loaded.size);
        material.uniforms.uCurrentAccent.value.copy(nextAccent);
        material.uniforms.uNextAccent.value.copy(nextAccent);
        material.uniforms.uCurrentSurface.value.copy(nextSurface);
        material.uniforms.uNextSurface.value.copy(nextSurface);
        material.uniforms.uTransition.value = 1;
        invalidate();
        return;
      }

      if (transitioningRef.current && nextTextureRef.current) {
        const elapsed = performance.now() - transitionStartedAtRef.current;
        const keepNext = elapsed >= TRANSITION_DURATION_MS * 0.5;
        const base = keepNext ? nextTextureRef.current : currentTextureRef.current;
        const discarded = keepNext ? currentTextureRef.current : nextTextureRef.current;
        if (keepNext) {
          material.uniforms.uCurrentAccent.value.copy(material.uniforms.uNextAccent.value);
          material.uniforms.uCurrentSurface.value.copy(material.uniforms.uNextSurface.value);
        }
        discarded?.texture.dispose();
        currentTextureRef.current = base;
        nextTextureRef.current = null;
      }

      const current = currentTextureRef.current;
      nextTextureRef.current = loaded;
      material.uniforms.uCurrentTexture.value = current?.texture ?? neutralTexture;
      material.uniforms.uCurrentSize.value.copy(current?.size ?? new THREE.Vector2(1, 1));
      material.uniforms.uNextTexture.value = loaded.texture;
      material.uniforms.uNextSize.value.copy(loaded.size);
      material.uniforms.uNextAccent.value.copy(nextAccent);
      material.uniforms.uNextSurface.value.copy(nextSurface);
      material.uniforms.uTransition.value = 0;
      transitionStartedAtRef.current = performance.now();
      transitioningRef.current = true;
      activeUntilRef.current = transitionStartedAtRef.current + TRANSITION_DURATION_MS;
      invalidate();
    };

    const imageSources = resolveModuleMediaFallbackSources(props.moduleId, props.imageSrc);
    const cancelImage = loadMediaImage(imageSources, (image, source) => {
      const texture = new THREE.Texture(image);
      texture.needsUpdate = true;
      install(configureTexture(texture, `${targetKey}:${source}`));
    }, () => install(configureTexture(neutralTexture.clone(), `${targetKey}:neutral`)), props.locale);
    return () => {
      cancelImage();
      generationRef.current += 1;
    };
  }, [invalidate, neutralTexture, nextAccent, nextSurface, props.imageSrc, props.mediaKey, props.moduleId, props.reducedMotion, props.locale]);

  useEffect(() => {
    if (props.documentVisible) {
      invalidate();
    }
  }, [invalidate, props.documentVisible, props.reducedMotion]);

  useEffect(() => () => {
    generationRef.current += 1;
    currentTextureRef.current?.texture.dispose();
    nextTextureRef.current?.texture.dispose();
    currentTextureRef.current = null;
    nextTextureRef.current = null;
  }, [neutralTexture]);

  useFrame((state) => {
    const material = materialRef.current;
    if (!material || !props.documentVisible) {
      return;
    }

    const now = performance.now();
    const targetPointerX = THREE.MathUtils.clamp(props.pointerX.get(), -1, 1);
    const targetPointerY = THREE.MathUtils.clamp(props.pointerY.get(), -1, 1);
    const targetActivity = THREE.MathUtils.clamp(props.pointerActivity.get(), 0, 1);
    const pointerBlend = props.reducedMotion ? 1 : 0.16;
    material.uniforms.uPointer.value.set(
      THREE.MathUtils.lerp(material.uniforms.uPointer.value.x, targetPointerX, pointerBlend),
      THREE.MathUtils.lerp(material.uniforms.uPointer.value.y, targetPointerY, pointerBlend)
    );
    material.uniforms.uPointerActivity.value = THREE.MathUtils.lerp(
      material.uniforms.uPointerActivity.value,
      targetActivity,
      pointerBlend
    );
    material.uniforms.uResolution.value.set(state.size.width, state.size.height);
    material.uniforms.uTime.value = props.reducedMotion ? 0 : performance.now() * 0.001;
    material.uniforms.uDark.value = THREE.MathUtils.lerp(
      material.uniforms.uDark.value,
      props.darkTheme ? 1 : 0,
      props.reducedMotion ? 1 : 0.12
    );

    if (transitioningRef.current) {
      const progress = props.reducedMotion
        ? 1
        : THREE.MathUtils.clamp((now - transitionStartedAtRef.current) / TRANSITION_DURATION_MS, 0, 1);
      material.uniforms.uTransition.value = progress;
      if (progress >= 1) {
        const previous = currentTextureRef.current;
        const current = nextTextureRef.current;
        if (current) {
          currentTextureRef.current = current;
          nextTextureRef.current = null;
          material.uniforms.uCurrentTexture.value = current.texture;
          material.uniforms.uNextTexture.value = current.texture;
          material.uniforms.uCurrentSize.value.copy(current.size);
          material.uniforms.uNextSize.value.copy(current.size);
          material.uniforms.uCurrentAccent.value.copy(material.uniforms.uNextAccent.value);
          material.uniforms.uCurrentSurface.value.copy(material.uniforms.uNextSurface.value);
          previous?.texture.dispose();
        }
        transitioningRef.current = false;
      }
    }

    const pointer = material.uniforms.uPointer.value;
    const pointerMoving = Math.abs(pointer.x - targetPointerX) > 0.001
      || Math.abs(pointer.y - targetPointerY) > 0.001
      || Math.abs(material.uniforms.uPointerActivity.value - targetActivity) > 0.001;
    if (!props.reducedMotion && (transitioningRef.current || pointerMoving || now < activeUntilRef.current)) {
      invalidate();
    }
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

export function LibraryAtmosphereField(props: LibraryAtmosphereFieldProps) {
  const { locale } = useI18n();
  const [supported] = useState(supportsWebGl2);
  const reducedMotion = Boolean(useReducedMotion());
  const darkTheme = useDarkTheme();
  const documentVisible = useDocumentVisible();

  if (!supported) {
    return null;
  }

  return (
    <div className="library-atmosphere-field" data-library-webgl2-field aria-hidden="true">
      <LibraryAtmosphereFieldBoundary>
        <Canvas
          flat
          fallback={null}
          dpr={[1, 1.25]}
          frameloop="demand"
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
          <AtmospherePlane
            {...props}
            locale={locale}
            darkTheme={darkTheme}
            documentVisible={documentVisible}
            reducedMotion={reducedMotion}
          />
        </Canvas>
      </LibraryAtmosphereFieldBoundary>
    </div>
  );
}
