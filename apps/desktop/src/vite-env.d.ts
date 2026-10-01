/// <reference types="vite/client" />

import "react";

interface ImportMetaEnv {
  readonly VITE_LANGAME_DESKTOP_UPDATES_ENABLED?: string;
}

declare module "react" {
  interface SVGAttributes<T> {
    referrerPolicy?: "no-referrer";
  }
}
