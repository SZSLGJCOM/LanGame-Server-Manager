import { ActivityNotice } from "./ActivityNotice";
import { useEffect, useId, useLayoutEffect, useRef, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
import { useI18n } from "../i18n";
import { describeError } from "../app-state";
import "./inline-confirm-action.css";

interface InlineConfirmActionProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onClick" | "children"> {
  children: ReactNode;
  confirmation: string;
  confirmLabel?: string;
  cancelLabel?: string;
  onConfirm: () => void | Promise<void>;
  prepareConfirmation?: () => Promise<string>;
  scopeKey: string;
  wrapperClassName?: string;
}

export function InlineConfirmAction({ children, confirmation, confirmLabel, cancelLabel, onConfirm, prepareConfirmation,
  scopeKey, wrapperClassName = "", disabled, ...buttonProps }: InlineConfirmActionProps) {
  const { t } = useI18n();
  const descriptionId = useId();
  const [review, setReview] = useState<{ scope: string; source: string; message: string; ready: boolean } | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const containerRef = useRef<HTMLSpanElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const submittingRef = useRef(false);
  const restoreFocusRef = useRef(false);
  const mountedRef = useRef(false);
  const preparationGeneration = useRef(0);
  const reviewRef = useRef(review);
  reviewRef.current = review;
  const open = review?.scope === scopeKey && review.source === confirmation && (!disabled || pending);

  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; preparationGeneration.current++; };
  }, []);

  useLayoutEffect(() => {
    if (open) cancelRef.current?.focus();
    else if (restoreFocusRef.current) triggerRef.current?.focus();
    restoreFocusRef.current = false;
  }, [open]);

  useEffect(() => {
    preparationGeneration.current++;
    setPreparing(false);
    setReview((current) => submittingRef.current && current?.scope === scopeKey && current.source === confirmation
      ? current : null);
    setError(null);
  }, [scopeKey, confirmation, disabled]);

  useEffect(() => {
    if (!open) return;
    function dismissOutside(event: Event) {
      if (event.target instanceof Node && !containerRef.current?.contains(event.target) && !submittingRef.current) {
        setReview(null);
        preparationGeneration.current++;
      }
    }
    document.addEventListener("pointerdown", dismissOutside);
    document.addEventListener("focusin", dismissOutside);
    return () => {
      document.removeEventListener("pointerdown", dismissOutside);
      document.removeEventListener("focusin", dismissOutside);
    };
  }, [open]);

  function cancel() {
    if (submittingRef.current) return;
    restoreFocusRef.current = true;
    preparationGeneration.current++;
    setPreparing(false);
    setReview(null);
    setError(null);
  }

  async function beginReview() {
    const generation = ++preparationGeneration.current;
    setError(null);
    setReview({ scope: scopeKey, source: confirmation, message: confirmation, ready: !prepareConfirmation });
    if (!prepareConfirmation) return;
    setPreparing(true);
    try {
      const message = await prepareConfirmation();
      if (mountedRef.current && generation === preparationGeneration.current) {
        setReview({ scope: scopeKey, source: confirmation, message, ready: true });
      }
    } catch (cause) {
      if (mountedRef.current && generation === preparationGeneration.current) setError(describeError(cause));
    } finally {
      if (mountedRef.current && generation === preparationGeneration.current) setPreparing(false);
    }
  }

  async function confirm() {
    if (!open || disabled || !review?.ready || preparing || submittingRef.current) return;
    submittingRef.current = true;
    setPending(true);
    setError(null);
    try {
      await onConfirm();
      if (mountedRef.current && reviewRef.current === review) {
        restoreFocusRef.current = Boolean(containerRef.current?.contains(document.activeElement));
        setReview(null);
      }
    } catch (cause) {
      if (mountedRef.current && reviewRef.current === review) setError(describeError(cause));
    } finally {
      submittingRef.current = false;
      if (mountedRef.current) setPending(false);
    }
  }

  return <span ref={containerRef} className={`inline-confirm-action ${open ? "is-confirming" : ""} ${wrapperClassName}`.trim()}
    onKeyDown={(event) => {
      if (open && event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        cancel();
      }
    }}>
    {!open ? <button {...buttonProps} ref={triggerRef} type={buttonProps.type ?? "button"} disabled={disabled || pending} aria-busy={pending || undefined}
      onClick={(event) => { event.preventDefault(); void beginReview(); }}>
      {children}
    </button> : <span className="inline-confirm-review" role="group" aria-label={buttonProps["aria-label"] ?? confirmation} aria-describedby={descriptionId} aria-busy={pending || preparing}>
      <span id={descriptionId} className="inline-confirm-message" tabIndex={prepareConfirmation && review.ready ? 0 : undefined}>
        {preparing ? t("common.loading") : review.message}
      </span>
      {error ? <ActivityNotice tone="error" onDismiss={() => setError(null)}>{error}</ActivityNotice> : null}
      <span className="inline-confirm-buttons">
        <button ref={cancelRef} type="button" className="secondary-button" disabled={pending} onClick={cancel}>
          {cancelLabel ?? t("common.cancel")}
        </button>
        {!preparing && !review.ready && <button type="button" className="secondary-button" onClick={() => void beginReview()}>{t("common.retry")}</button>}
        <button type="button" className="secondary-button inline-confirm-submit" disabled={pending || preparing || !review.ready} onClick={() => void confirm()}>
          {pending ? t("common.processing") : confirmLabel ?? t("common.confirm")}
        </button>
      </span>
    </span>}
  </span>;
}
