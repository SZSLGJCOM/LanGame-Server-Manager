import { useEffect, useRef } from "react";
import type { VideoHTMLAttributes } from "react";
import { attachStreamVideo } from "../stream-video-player";
import { useI18n } from "../i18n";

interface StreamVideoProps extends Omit<VideoHTMLAttributes<HTMLVideoElement>, "src"> {
  streamUrl: string;
}

export function StreamVideo({ streamUrl, autoPlay, muted, onError, ...props }: StreamVideoProps) {
  const { locale } = useI18n();
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const failurePending = useRef(false);
  const playback = useRef<{ streamUrl: string; time: number; playing: boolean } | null>(null);
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    failurePending.current = false;
    const detach = attachStreamVideo(video, { streamUrl, autoPlay, muted, locale,
      initialPlayback: playback.current?.streamUrl === streamUrl ? playback.current : undefined, onUnavailable() {
      failurePending.current = true;
      video.dispatchEvent(new Event("error"));
    } });
    return () => {
      playback.current = { streamUrl, time: Number.isFinite(video.currentTime) ? video.currentTime : 0, playing: !video.paused };
      detach();
    };
  }, [autoPlay, muted, streamUrl, locale]);

  return <video key={streamUrl} ref={videoRef} autoPlay={autoPlay} muted={muted} {...props}
    onError={(event) => {
      if (failurePending.current) { failurePending.current = false; onError?.(event); }
    }} />;
}
