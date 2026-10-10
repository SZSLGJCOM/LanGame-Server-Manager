export function readInlinePreviewStatus(frame: Pick<HTMLIFrameElement, "contentDocument">): string | undefined {
  return frame.contentDocument?.documentElement?.dataset.preview;
}
