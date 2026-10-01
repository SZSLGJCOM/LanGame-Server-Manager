import type { SVGProps } from "react";

interface IconProps extends SVGProps<SVGSVGElement> {
  name:
    | "zap"
    | "dashboard"
    | "server"
    | "package"
    | "terminal"
    | "user"
    | "users"
    | "database"
    | "monitor"
    | "shield"
    | "check"
    | "lock"
    | "settings"
    | "bell"
    | "search"
    | "refresh"
    | "chevron-left"
    | "chevron-right"
    | "plus"
    | "external-link"
    | "user-x"
    | "minus"
    | "square"
    | "restore"
    | "sun"
    | "moon"
    | "x"
    | "edit-3"
    | "alert-triangle"
    | "cpu"
    | "loader"
    | "alert-circle"
    | "message-square"
    | "send"
    | "pause"
    | "play"
    | "play-circle"
    | "stop-circle"
    | "clock"
    | "history"
    | "trash"
    | "file-text"
    | "list"
    | "inbox"
    | "check-circle"
    | "circle-dot"
    | "folder"
    | "copy"
    | "hard-drive"
    | "star"
    | "eye"
    | "download"
    | "tag";
}

export type ShellIconName = IconProps["name"];

const ICON_PATHS: Record<IconProps["name"], string[]> = {
  pause: ["M8 5v14", "M16 5v14"],
  play: ["m8 5 11 7-11 7V5Z"],
  zap: ["M13 2 4 14h7l-1 8 9-12h-7l1-8Z"],
  dashboard: ["M3 3h8v8H3z", "M13 3h8v5h-8z", "M13 10h8v11h-8z", "M3 13h8v8H3z"],
  server: ["M4 4h16v6H4z", "M4 14h16v6H4z", "M7 7h.01", "M7 17h.01"],
  package: ["M12 2 4.5 6v12L12 22l7.5-4V6L12 2Z", "M12 22V12", "M4.5 6 12 10l7.5-4"],
  terminal: ["M4 17 10 11 4 5", "M12 19h8"],
  user: ["M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2", "M12 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8"],
  users: ["M16 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2", "M10 7a4 4 0 1 0 0-8 4 4 0 0 0 0 8", "M20 8a3 3 0 1 1 0 6", "M23 21v-2a4 4 0 0 0-3-3.87"],
  database: ["M12 3C7 3 3 4.79 3 7s4 4 9 4 9-1.79 9-4-4-4-9-4Z", "M3 7v5c0 2.21 4 4 9 4s9-1.79 9-4V7", "M3 12v5c0 2.21 4 4 9 4s9-1.79 9-4v-5"],
  monitor: ["M3 5h18v12H3z", "M8 21h8", "M12 17v4"],
  shield: ["M12 2l7 4v6c0 5-3.5 9-7 10-3.5-1-7-5-7-10V6l7-4Z"],
  check: ["M20 6 9 17l-5-5"],
  lock: ["M6 10h12v10H6z", "M8 10V7a4 4 0 0 1 8 0v3"],
  settings: ["M12 2v3", "M12 19v3", "M4.93 4.93l2.12 2.12", "M16.95 16.95l2.12 2.12", "M2 12h3", "M19 12h3", "M4.93 19.07l2.12-2.12", "M16.95 7.05l2.12-2.12", "M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8Z"],
  bell: ["M15 17h5l-1.4-1.4A2 2 0 0 1 18 14.2V11a6 6 0 1 0-12 0v3.2a2 2 0 0 1-.6 1.4L4 17h5", "M9 17a3 3 0 0 0 6 0"],
  search: ["m21 21-4.35-4.35", "M10 18a8 8 0 1 1 0-16 8 8 0 0 1 0 16Z"],
  refresh: ["M21 12a9 9 0 0 1-15.36 6.36L3 15", "M3 21v-6h6", "M3 12a9 9 0 0 1 15.36-6.36L21 9", "M21 3v6h-6"],
  "chevron-left": ["M15 18l-6-6 6-6"],
  "chevron-right": ["M9 18l6-6-6-6"],
  plus: ["M12 5v14", "M5 12h14"],
  "external-link": ["M15 3h6v6", "M10 14 21 3", "M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"],
  "user-x": ["M16 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2", "M10 7a4 4 0 1 0 0-8 4 4 0 0 0 0 8", "M18 8l5 5", "M23 8l-5 5"],
  minus: ["M5 12h14"],
  square: ["M6 6h12v12H6z"],
  restore: ["M8 8h12v12H8z", "M4 16V4h12"],
  sun: ["M12 3v2", "M12 19v2", "M4.93 4.93l1.41 1.41", "M17.66 17.66l1.41 1.41", "M3 12h2", "M19 12h2", "M4.93 19.07l1.41-1.41", "M17.66 6.34l1.41-1.41", "M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8Z"],
  moon: ["M21 12.79A9 9 0 1 1 11.21 3c0 5 4 9 9 9Z"],
  x: ["M6 6l12 12", "M18 6 6 18"],
  "edit-3": ["M12 20h9", "M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5Z"],
  "alert-triangle": ["M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0Z", "M12 9v4", "M12 17h.01"],
  cpu: ["M9 2H6a2 2 0 0 0-2 2v3", "M18 2h3a2 2 0 0 1 2 2v3", "M6 22H3a2 2 0 0 1-2-2v-3", "M18 22h3a2 2 0 0 0 2-2v-3", "M7 7h10v10H7z", "M10 10h4v4h-4z"],
  loader: ["M12 2v4", "M12 18v4", "M4.93 4.93l2.83 2.83", "M16.24 16.24l2.83 2.83", "M2 12h4", "M18 12h4", "M4.93 19.07l2.83-2.83", "M16.24 7.76l2.83-2.83"],
  "alert-circle": ["M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Z", "M12 8v4", "M12 16h.01"],
  "message-square": ["M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"],
  send: ["M22 2 11 13", "M22 2 15 22 11 13 2 9l20-7Z"],
  "play-circle": ["M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Z", "M10 8l6 4-6 4V8Z"],
  "stop-circle": ["M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Z", "M9 9h6v6H9z"],
  clock: ["M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Z", "M12 6v6l4 2"],
  history: ["M3 12a9 9 0 1 0 3-6.7", "M3 4v6h6", "M12 7v5l3 2"],
  trash: ["M3 6h18", "M8 6V3h8v3", "M6 6l1 15h10l1-15", "M10 11v6", "M14 11v6"],
  "file-text": ["M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8l-6-6Z", "M14 2v6h6", "M16 13H8", "M16 17H8", "M10 9H8"],
  list: ["M8 6h13", "M8 12h13", "M8 18h13", "M3 6h.01", "M3 12h.01", "M3 18h.01"],
  inbox: ["M22 12h-6l-2 3H10l-2-3H2", "M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11Z"],
  "check-circle": ["M22 11.08V12a10 10 0 1 1-5.93-9.14", "M22 4 12 14.01l-3-3"],
  "circle-dot": ["M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Z", "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z"],
  folder: ["M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"],
  copy: ["M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1", "M12 8h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-8a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2z"],
  "hard-drive": ["M22 12H2", "M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11Z", "M6 16h.01", "M10 16h.01"],
  star: ["M12 3.2 14.7 9l6.3.7-4.7 4.2 1.4 6.1L12 17.2 6.3 20l1.4-6.1L3 9.7 9.3 9 12 3.2Z"],
  eye: ["M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z", "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z"],
  download: ["M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4", "M7 10l5 5 5-5", "M12 15V3"],
  tag: ["M20.59 13.41 13.42 20.58a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82Z", "M7 7h.01"],
};

export function ShellIcon({ name, className, ...props }: IconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
      {...props}
    >
      {ICON_PATHS[name].map((path, index) => (
        <path key={`${name}-${index}`} d={path} />
      ))}
    </svg>
  );
}
