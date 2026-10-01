import { useEffect, useRef } from "react";
import { attachHtmlMediaFallbacks, replaceHtmlMediaContent } from "../html-media-fallback";
import { useI18n } from "../i18n";

export function LibraryStoryHtml({ sanitizedHtml }: { sanitizedHtml: string }) {
  const { locale } = useI18n();
  const container = useRef<HTMLDivElement | null>(null);
  const renderedHtml = useRef<string | null>(null);
  useEffect(() => {
    if (!container.current) return;
    if (renderedHtml.current !== sanitizedHtml) {
      replaceHtmlMediaContent(container.current, sanitizedHtml);
      renderedHtml.current = sanitizedHtml;
    }
    return attachHtmlMediaFallbacks(container.current, locale);
  }, [sanitizedHtml, locale]);
  return <div ref={container} className="library-story-html" />;
}
