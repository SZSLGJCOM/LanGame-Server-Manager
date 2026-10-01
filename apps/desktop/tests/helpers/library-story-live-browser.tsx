import React, { useEffect } from "react";
import { createRoot } from "react-dom/client";
import { I18nProvider, useI18n } from "../../src/i18n";
import type { LocaleCode } from "../../src/i18n-config";
import type { ModuleStoreEntry } from "../../src/store-media";
import { LibraryStoryPanel } from "../../src/views/library/LibraryStoryPanel";
import "../../src/app.css";

const fixture = document.getElementById("fixture")!;
const root = createRoot(fixture);
const nonce = new URLSearchParams(location.search).get("nonce");
const errors: string[] = [];
addEventListener("error", (event) => errors.push(event.message));
addEventListener("unhandledrejection", (event) => errors.push(String(event.reason)));
document.documentElement.dataset.theme = "dark";

function Harness({ appId, language }: { appId: number; language: LocaleCode }) {
  const { locale, setLocale } = useI18n();
  useEffect(() => { setLocale(language); }, [language, setLocale]);
  const entry: ModuleStoreEntry = {
    storeSource: "steam", storeAppId: appId, storeName: `Steam ${appId}`,
    shortDescription: "", aboutParagraphs: [], genres: [], categories: [], developers: [], publishers: [],
    releaseDate: "", storeUrl: "", officialLinks: [], screenshots: [], trailers: [], coverUrl: null,
  };
  return <main style={{ width: 960, maxWidth: "calc(100% - 64px)", margin: "32px auto" }}>
    <h1>Steam {appId} · {language}</h1>
    {locale === language ? <LibraryStoryPanel storeEntry={entry} storyParagraphs={[]} /> : null}
  </main>;
}

async function waitFor(predicate: () => boolean, description: string, deadline: number) {
  while (!predicate()) {
    if (performance.now() > deadline) throw new Error(`${description}: ${JSON.stringify({
      text: fixture.querySelector("[role=status]")?.textContent,
      images: [...fixture.querySelectorAll("img")].map((image) => ({ src: image.src, width: image.naturalWidth, hidden: image.hidden })),
      videos: [...fixture.querySelectorAll("video")].map((video) => ({ src: video.currentSrc, ready: video.readyState, time: video.currentTime, muted: video.muted, paused: video.paused, error: video.error?.code })),
    })}`);
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

async function run() {
  const config = await (await fetch("/__steam_probe_config", { signal: AbortSignal.timeout(5000) })).json() as {
    token: string; stories: { appId: number; locale: LocaleCode }[]; sourceMode: string;
  };
  sessionStorage.setItem("langameLanToken", config.token);
  const stories = [];
  for (const { appId, locale } of config.stories) {
    const started = performance.now();
    const deadline = started + 20000;
    root.render(<I18nProvider><Harness appId={appId} language={locale} /></I18nProvider>);
    await waitFor(() => Boolean(fixture.querySelector(".library-story-html")?.textContent?.trim()),
      `Official story text ${appId}/${locale}`, deadline);
    await waitFor(() => {
      const images = [...fixture.querySelectorAll<HTMLImageElement>(".library-story-html img")];
      const videos = [...fixture.querySelectorAll<HTMLVideoElement>(".library-story-html video")];
      return images.every((image) => !image.hidden && image.naturalWidth > 0) &&
        videos.every((video) => video.readyState >= 3 && video.currentTime > 0.1 && !video.paused && video.muted);
    }, `Playable real Steam media ${appId}/${locale}`, deadline);
    const playing = [...fixture.querySelectorAll<HTMLVideoElement>(".library-story-html video")];
    const previousTimes = playing.map((video) => video.currentTime);
    await waitFor(() => playing.every((video, index) => !video.paused && video.currentTime !== previousTimes[index]),
      `Advancing animations ${appId}/${locale}`, deadline);
    stories.push({ appId, locale, textChars: fixture.querySelector(".library-story-html")!.textContent!.trim().length,
      elapsedMs: Math.round(performance.now() - started),
      images: [...fixture.querySelectorAll<HTMLImageElement>("img")].map((image) => ({ width: image.naturalWidth, height: image.naturalHeight, url: image.currentSrc })),
      videos: [...fixture.querySelectorAll<HTMLVideoElement>("video")].map((video) => ({ width: video.videoWidth, height: video.videoHeight, time: video.currentTime, muted: video.muted, url: video.currentSrc })),
    });
    if (stories.length < config.stories.length) {
      root.render(null);
      await waitFor(() => !fixture.querySelector(".library-story-html"), "Previous story disposed", deadline);
    }
  }
  if (errors.length) throw new Error(errors.join("; "));
  return { status: "passed", source_mode: config.sourceMode, stories, browser_errors: errors };
}

void run().catch((error: unknown) => ({ status: "failed", error: String(error), browser_errors: errors }))
  .then((report) => fetch(`/__reliability_result/${nonce}`, { method: "POST", body: JSON.stringify(report) }));
