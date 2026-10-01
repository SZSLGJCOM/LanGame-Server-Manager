interface LanMarkProps {
  className?: string;
  label?: string;
}

export function LanMark({ className, label }: LanMarkProps) {
  return (
    <svg
      className={className}
      width={47}
      height={31}
      viewBox="0 78 676 448"
      fill="none"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <path
        d="M598 393C620 335 593 273 540 239C519 225 494 218 469 222C452 153 394 105 321 105C246 105 187 152 161 225C144 219 124 216 106 219C48 229 7 278 7 343C7 417 63 472 148 472H594"
        stroke="currentColor"
        strokeWidth="29"
        strokeLinecap="butt"
        strokeLinejoin="round"
      />
      <circle
        cx="629"
        cy="477"
        r="31"
        stroke="currentColor"
        strokeWidth="17"
      />
      <rect x="227" y="327" width="41" height="42" fill="currentColor" />
      <rect x="367" y="327" width="41" height="42" fill="currentColor" />
    </svg>
  );
}
