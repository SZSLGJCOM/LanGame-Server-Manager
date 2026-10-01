export interface AiDataRecipient {
  origin: string;
  loopback: boolean;
  encrypted: boolean;
}

/** Display only the configured HTTP origin: paths and credentials may contain secrets. */
export function describeAiDataRecipient(baseUrl: string): AiDataRecipient | null {
  try {
    const url = new URL(baseUrl);
    if (url.protocol !== "https:" && url.protocol !== "http:") return null;
    const hostname = url.hostname.toLowerCase();
    return {
      origin: url.origin,
      loopback: hostname === "localhost" || hostname === "localhost." || hostname === "[::1]"
        || /^127\.(?:\d{1,3}\.){2}\d{1,3}$/.test(hostname),
      encrypted: url.protocol === "https:"
    };
  } catch {
    return null;
  }
}
