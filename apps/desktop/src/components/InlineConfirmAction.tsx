import { ActivityNotice } from "./ActivityNotice";
import { InlineConfirmReview } from "./InlineConfirmReview";
import { useEffect, useLayoutEffect, useRef, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
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
  const [review, setReview] = useState<{ scope: string; source: string; message: string; ready: boolean } | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const containerRef = useRef<HTMLSpanElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
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
    if (!open && restoreFocusRef.current) triggerRef.current?.focus();
    restoreFocusRef.current = false;
  }, [open]);

  useEffect(() => {
    preparationGeneration.current++;
    setPreparing(false);
    setReview((current) => submittingRef.current && current?.scope === scopeKey && current.source === confirmation
      ? current : null);
    setError(null);
  }, [scopeKey, confirmation, disabled]);

  function cancel(restoreFocus: boolean) {
    if (submittingRef.current) return;
    restoreFocusRef.current = restoreFocus;
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

  if (open) return <InlineConfirmReview wrapperClassName={wrapperClassName} containerRef={containerRef}
    label={buttonProps["aria-label"] ?? confirmation} message={review.message} confirmLabel={confirmLabel} cancelLabel={cancelLabel}
    pending={pending} preparing={preparing} scrollable={Boolean(prepareConfirmation && review.ready)}
    onCancel={() => cancel(true)} onDismissOutside={() => cancel(false)}
    onConfirm={review.ready ? () => void confirm() : undefined}
    onRetry={!preparing && !review.ready ? () => void beginReview() : undefined}
    error={error ? <ActivityNotice tone="error" onDismiss={() => setError(null)}>{error}</ActivityNotice> : null} />;

  return <span className={`inline-confirm-action ${wrapperClassName}`.trim()}>
    <button {...buttonProps} ref={triggerRef} type={buttonProps.type ?? "button"} disabled={disabled || pending} aria-busy={pending || undefined}
      onClick={(event) => { event.preventDefault(); void beginReview(); }}>
      {children}
    </button>
  </span>;
}
