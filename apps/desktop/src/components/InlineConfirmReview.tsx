import { useEffect, useId, useLayoutEffect, useRef, type ReactNode, type RefObject } from "react";
import { useI18n } from "../i18n";
import "./inline-confirm-action.css";

interface Props {
  label: string;
  message: string;
  wrapperClassName?: string;
  confirmLabel?: string;
  confirmClassName?: string;
  cancelLabel?: string;
  cancelIcon?: ReactNode;
  revealOnOpen?: boolean;
  preparing?: boolean;
  pending?: boolean;
  scrollable?: boolean;
  error?: ReactNode;
  containerRef?: RefObject<HTMLSpanElement | null>;
  onConfirm?: () => void;
  onCancel: () => void;
  onDismissOutside?: () => void;
  onRetry?: () => void;
}

/** The open confirmation, whether the user clicked a destructive action or startup found a conflict. */
export function InlineConfirmReview(props: Props) {
  const { t } = useI18n();
  const descriptionId = useId();
  const fallbackContainer = useRef<HTMLSpanElement>(null);
  const container = props.containerRef ?? fallbackContainer;
  const cancel = useRef<HTMLButtonElement>(null);
  const dismiss = props.onDismissOutside ?? props.onCancel;
  const cancelLabel = props.cancelLabel ?? t("common.cancel");

  useLayoutEffect(() => {
    cancel.current?.focus();
    if (props.revealOnOpen) container.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [container, props.revealOnOpen]);
  useEffect(() => {
    if (props.pending) return;
    function outside(event: Event) {
      if (event.target instanceof Node && !container.current?.contains(event.target)) dismiss();
    }
    document.addEventListener("pointerdown", outside);
    document.addEventListener("focusin", outside);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("focusin", outside);
    };
  }, [container, dismiss, props.pending]);

  return <span ref={container} className={`inline-confirm-action is-confirming ${props.wrapperClassName ?? ""}`.trim()}
    onKeyDown={(event) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      if (!props.pending) props.onCancel();
    }}>
    <span className="inline-confirm-review" role="group" aria-label={props.label}
      aria-describedby={descriptionId} aria-busy={Boolean(props.pending || props.preparing)}>
      <span className="inline-confirm-message" id={descriptionId} tabIndex={props.scrollable ? 0 : undefined}>
        {props.preparing ? t("common.loading") : props.message}
      </span>
      {props.error}
      <span className="inline-confirm-buttons">
        <button ref={cancel} type="button" className="secondary-button inline-confirm-cancel"
          aria-label={props.cancelIcon ? cancelLabel : undefined} title={props.cancelIcon ? cancelLabel : undefined}
          disabled={props.pending} onClick={props.onCancel}>
          {props.cancelIcon ?? cancelLabel}
        </button>
        {props.onRetry ? <button type="button" className="secondary-button" onClick={props.onRetry}>{t("common.retry")}</button> : null}
        <button type="button" className={`secondary-button inline-confirm-submit ${props.confirmClassName ?? ""}`.trim()}
          disabled={props.pending || props.preparing || !props.onConfirm} onClick={props.onConfirm}>
          {props.pending ? t("common.processing") : props.confirmLabel ?? t("common.confirm")}
        </button>
      </span>
    </span>
  </span>;
}
