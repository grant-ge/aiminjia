import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ChevronLeft, ChevronRight, CornerDownLeft, Info } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { Question } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import type { PendingAction } from "./pendingActionSelectors";

type PermissionDestination = "session" | "workspace" | "user";
type PermissionChoice = "allow-once" | "allow-remember";

export type PermissionDecision = {
  remember: boolean;
  destination: PermissionDestination;
  feedback?: string;
};

interface Props {
  action: PendingAction | PendingAction[];
  onAllowPermission: (
    toolCallId: string,
    decision: PermissionDecision,
  ) => Promise<void> | void;
  onDenyPermission: (
    toolCallId: string,
    decision: PermissionDecision,
  ) => Promise<void> | void;
  onCancelPermission: (toolCallId: string) => Promise<void> | void;
  onSubmitInteraction: (
    interactionId: string,
    value: {
      answers: Record<string, string>;
      annotations?: Record<string, unknown>;
    },
  ) => Promise<void> | void;
  onCancelInteraction: (interactionId: string) => Promise<void> | void;
  onClearStalePermission: (sessionId: string) => Promise<void> | void;
  onClearStaleInteraction: (sessionId: string) => Promise<void> | void;
}

const DESTINATION_OPTIONS: Array<{
  value: PermissionDestination;
  label: string;
  description: string;
}> = [
  {
    value: "session",
    label: "仅本次",
    description: "这次处理后不保留规则。",
  },
  {
    value: "workspace",
    label: "记住到工作区",
    description: "只对当前工作区后续操作复用这条规则。",
  },
  {
    value: "user",
    label: "记住到用户级",
    description: "对当前用户后续同类操作都复用。",
  },
];

function permissionDestinations(
  action: Extract<PendingAction, { kind: "permission" }>,
): PermissionDestination[] {
  const options = new Set<PermissionDestination>(
    action.ask.rememberOptions ?? ["session"],
  );
  options.add("session");
  return DESTINATION_OPTIONS.map((option) => option.value).filter((value) =>
    options.has(value),
  );
}

function initialDestination(
  action: Extract<PendingAction, { kind: "permission" }>,
  destinations: PermissionDestination[],
) {
  return action.ask.defaultDestination &&
    destinations.includes(action.ask.defaultDestination)
    ? action.ask.defaultDestination
    : (destinations[0] ?? "session");
}

function permissionDecision(
  destination: PermissionDestination,
): PermissionDecision {
  return {
    remember: destination !== "session",
    destination,
  };
}

function denyDecision(): PermissionDecision {
  return {
    remember: false,
    destination: "session",
  };
}

function preferredRememberDestination(
  destinations: PermissionDestination[],
): PermissionDestination | null {
  if (destinations.includes("user")) return "user";
  if (destinations.includes("workspace")) return "workspace";
  return null;
}

function extractPermissionTarget(message: string): string {
  const pathMatch = message.match(/(?:路径|path)\s*=\s*([^\n，。]+)/i);
  if (pathMatch?.[1]) return pathMatch[1].trim();
  const windowsPathMatch = message.match(/([A-Za-z]:[\\/][^\s，。?？]+)/);
  if (windowsPathMatch?.[1]) return windowsPathMatch[1].trim();
  const absolutePathMatch = message.match(/(\/[^\s，。?？]+)/);
  if (absolutePathMatch?.[1]) return absolutePathMatch[1].trim();
  return message.trim();
}

function permissionCommandPreview(target: string): string {
  if (!target) return "";
  if (/^[A-Za-z]:[\\/]/.test(target)) return `dir /a ${target}`;
  return `ls -la ${target}`;
}

function fitTextareaToContent(textarea: HTMLTextAreaElement | null) {
  if (!textarea) return;
  textarea.style.height = "auto";
  const lineHeight = Number.parseFloat(getComputedStyle(textarea).lineHeight);
  const minHeight = Number.isFinite(lineHeight) ? lineHeight : 24;
  textarea.style.height = `${Math.max(textarea.scrollHeight, minHeight)}px`;
}

function SurfaceShell({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="rounded-lg border border-border bg-card p-4 shadow-[var(--shadow-md)]">
      <div className="mb-4">
        <p className="text-sm font-semibold text-foreground">{title}</p>
      </div>
      {children}
    </section>
  );
}

function PermissionPanel({
  action,
  onAllowPermission,
  onDenyPermission,
}: Pick<Props, "onAllowPermission" | "onDenyPermission"> & {
  action: Extract<PendingAction, { kind: "permission" }>;
}) {
  const { t } = useTranslation();
  const destinations = useMemo(() => permissionDestinations(action), [action]);
  const rememberDestination = useMemo(
    () => preferredRememberDestination(destinations),
    [destinations],
  );
  const defaultDestination = initialDestination(action, destinations);
  const [choice, setChoice] = useState<PermissionChoice>(() =>
    defaultDestination === "session" ? "allow-once" : "allow-remember",
  );
  const submittingRef = useRef(false);
  const [submitting, setSubmitting] = useState(false);
  const target = extractPermissionTarget(action.ask.message);
  const commandPreview = permissionCommandPreview(target);
  const canRemember = rememberDestination != null;
  const selectedChoice =
    choice === "allow-remember" && !canRemember ? "allow-once" : choice;

  async function runOnce(task: () => Promise<void> | void) {
    if (submittingRef.current) return;
    submittingRef.current = true;
    setSubmitting(true);
    try {
      await task();
    } finally {
      submittingRef.current = false;
      setSubmitting(false);
    }
  }

  function submitPermissionChoice() {
    const destination =
      selectedChoice === "allow-remember" && rememberDestination
        ? rememberDestination
        : "session";
    return runOnce(() =>
      onAllowPermission(action.ask.toolCallId, permissionDecision(destination)),
    );
  }

  return (
    <section className="rounded-lg border border-border bg-card p-3 shadow-[var(--shadow-md)]">
      <fieldset className="space-y-3">
        <legend className="break-all px-1 text-sm font-semibold leading-6 text-foreground">
          {t("pendingAction.permission.title", { target })}
        </legend>

        {commandPreview ? (
          <p className="whitespace-pre-wrap break-all px-1 py-2 font-mono text-sm leading-6 text-muted-foreground">
            {commandPreview}
          </p>
        ) : null}

        <div className="space-y-1">
          <PermissionOption
            value="allow-once"
            selected={selectedChoice === "allow-once"}
            name={`permission-choice-${action.ask.toolCallId}`}
            index={1}
            label={t("pendingAction.permission.allowOnce")}
            onSelect={setChoice}
          />
          {canRemember ? (
            <PermissionOption
              value="allow-remember"
              selected={selectedChoice === "allow-remember"}
              name={`permission-choice-${action.ask.toolCallId}`}
              index={2}
              label={t("pendingAction.permission.allowRemember")}
              onSelect={setChoice}
            />
          ) : null}

          <div className="flex flex-wrap items-end justify-between gap-3">
            <div className="ml-auto flex shrink-0 items-center justify-end gap-2 py-1">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs"
                disabled={submitting}
                onClick={() =>
                  void runOnce(() =>
                    onDenyPermission(action.ask.toolCallId, denyDecision()),
                  )
                }
              >
                {t("pendingAction.permission.skip")}
              </Button>
              <Button
                type="button"
                size="sm"
                className="h-7 rounded-full px-3 text-xs"
                disabled={submitting}
                onClick={() => void submitPermissionChoice()}
              >
                {t("pendingAction.permission.submit")}
                <span className="ml-0.5 inline-flex h-4 w-4 items-center justify-center rounded-full bg-primary-foreground/15">
                  <CornerDownLeft className="h-3 w-3" aria-hidden="true" />
                </span>
              </Button>
            </div>
          </div>
        </div>
      </fieldset>
    </section>
  );
}

function PermissionGroupPanel({
  action,
  onAllowPermission,
  onDenyPermission,
}: Pick<Props, "onAllowPermission" | "onDenyPermission"> & {
  action: Extract<PendingAction, { kind: "permission-group" }>;
}) {
  const { t } = useTranslation();
  const representative = action.asks[0];
  if (!representative) return null;
  const representativeAction = {
    kind: "permission" as const,
    ask: representative,
  };
  const destinations = useMemo(
    () => permissionDestinations(representativeAction),
    [representativeAction],
  );
  const rememberDestination = useMemo(
    () => preferredRememberDestination(destinations),
    [destinations],
  );
  const defaultDestination = initialDestination(
    representativeAction,
    destinations,
  );
  const [choice, setChoice] = useState<PermissionChoice>(() =>
    defaultDestination === "session" ? "allow-once" : "allow-remember",
  );
  const submittingRef = useRef(false);
  const [submitting, setSubmitting] = useState(false);
  const canRemember = rememberDestination != null;
  const selectedChoice =
    choice === "allow-remember" && !canRemember ? "allow-once" : choice;
  const requests = action.asks.map((ask) => ({
    toolCallId: ask.toolCallId,
    toolName: ask.toolName,
    target: extractPermissionTarget(ask.message),
  }));

  async function runOnce(task: () => Promise<void> | void) {
    if (submittingRef.current) return;
    submittingRef.current = true;
    setSubmitting(true);
    try {
      await task();
    } finally {
      submittingRef.current = false;
      setSubmitting(false);
    }
  }

  function submitPermissionChoice() {
    const destination =
      selectedChoice === "allow-remember" && rememberDestination
        ? rememberDestination
        : "session";
    const decision = permissionDecision(destination);
    return runOnce(async () => {
      for (const ask of action.asks) {
        await onAllowPermission(ask.toolCallId, decision);
      }
    });
  }

  function denyPermissionChoice() {
    const decision = denyDecision();
    return runOnce(async () => {
      for (const ask of action.asks) {
        await onDenyPermission(ask.toolCallId, decision);
      }
    });
  }

  return (
    <section className="rounded-lg border border-border bg-card p-3 shadow-[var(--shadow-md)]">
      <fieldset className="space-y-3">
        <legend className="break-all px-1 text-sm font-semibold leading-6 text-foreground">
          需要处理 {action.asks.length} 个权限请求
        </legend>

        <div
          aria-label="权限请求列表"
          className="flex flex-wrap items-center gap-x-4 gap-y-1 px-1"
          role="list"
        >
          {requests.map((request) => (
            <div
              key={`${request.toolCallId}:${request.target}`}
              className="inline-flex max-w-full items-baseline gap-1.5 text-sm leading-6"
              role="listitem"
            >
              <span className="font-medium text-foreground">
                {request.toolName}
              </span>
              <span className="break-all font-mono text-muted-foreground">
                {request.target}
              </span>
            </div>
          ))}
        </div>

        <div className="space-y-1">
          <PermissionOption
            value="allow-once"
            selected={selectedChoice === "allow-once"}
            name={`permission-choice-group-${representative.toolCallId}`}
            index={1}
            label={t("pendingAction.permission.allowOnce")}
            onSelect={setChoice}
          />
          {canRemember ? (
            <PermissionOption
              value="allow-remember"
              selected={selectedChoice === "allow-remember"}
              name={`permission-choice-group-${representative.toolCallId}`}
              index={2}
              label={t("pendingAction.permission.allowRemember")}
              onSelect={setChoice}
            />
          ) : null}

          <div className="flex flex-wrap items-end justify-between gap-3">
            <div className="ml-auto flex shrink-0 items-center justify-end gap-2 py-1">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs"
                disabled={submitting}
                onClick={() => void denyPermissionChoice()}
              >
                {t("pendingAction.permission.skip")}
              </Button>
              <Button
                type="button"
                size="sm"
                className="h-7 rounded-full px-3 text-xs"
                disabled={submitting}
                onClick={() => void submitPermissionChoice()}
              >
                {t("pendingAction.permission.submit")}
                <span className="ml-0.5 inline-flex h-4 w-4 items-center justify-center rounded-full bg-primary-foreground/15">
                  <CornerDownLeft className="h-3 w-3" aria-hidden="true" />
                </span>
              </Button>
            </div>
          </div>
        </div>
      </fieldset>
    </section>
  );
}

function PermissionOption({
  value,
  selected,
  name,
  index,
  label,
  suffix,
  muted,
  onSelect,
}: {
  value: PermissionChoice;
  selected: boolean;
  name: string;
  index: number;
  label: string;
  suffix?: string;
  muted?: boolean;
  onSelect: (value: PermissionChoice) => void;
}) {
  return (
    <label
      className={cn(
        "group flex min-h-9 cursor-pointer items-start gap-3 rounded-md px-3 py-2 text-sm text-foreground transition-colors",
        selected ? "bg-muted/60" : "hover:bg-muted/40",
        muted && !selected && "text-muted-foreground",
      )}
    >
      <span className="w-5 shrink-0 pt-0.5 text-right text-sm tabular-nums text-muted-foreground">
        {index}.
      </span>
      <input
        type="radio"
        className="sr-only"
        name={name}
        aria-label={suffix ? `${label} ${suffix}` : label}
        checked={selected}
        onChange={() => onSelect(value)}
      />
      <span
        className={cn(
          "min-w-0 flex-1 whitespace-normal break-words leading-6",
          selected && "font-medium",
        )}
      >
        {label}
        {suffix ? (
          <span className="ml-1 font-mono text-muted-foreground break-all">
            {suffix}
          </span>
        ) : null}
      </span>
    </label>
  );
}

function questionKey(question: Question) {
  return question.question;
}

function selectedValues(answer: string | undefined) {
  return (answer ?? "").split(", ").filter(Boolean);
}

function answerSummary(answers: Record<string, string>) {
  return Object.entries(answers)
    .map(([question, answer]) => `${question} = ${answer}`)
    .join("；");
}

function optionLabels(question: Question) {
  return new Set(question.options.map((option) => option.label));
}

function valuesWithoutCustom(question: Question, answer: string | undefined) {
  const labels = optionLabels(question);
  return selectedValues(answer).filter((value) => labels.has(value));
}

function isCustomOptionLabel(label: string, customLabel: string) {
  const normalized = label.trim().toLowerCase();
  const normalizedCustom = customLabel.trim().toLowerCase();
  return (
    normalized === normalizedCustom ||
    normalized === "other" ||
    normalized === "其它"
  );
}

function UserQuestionPanel({
  action,
  onSubmitInteraction,
  onCancelInteraction,
}: Pick<Props, "onSubmitInteraction" | "onCancelInteraction"> & {
  action: Extract<PendingAction, { kind: "user-question" }>;
}) {
  const { t } = useTranslation();
  const questions = action.interaction.payload.questions;
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [customSelected, setCustomSelected] = useState<Record<string, boolean>>(
    {},
  );
  const [customAnswers, setCustomAnswers] = useState<Record<string, string>>(
    {},
  );
  const [activeIndex, setActiveIndex] = useState(0);
  const [validationKey, setValidationKey] = useState<string | null>(null);
  const customInputRef = useRef<HTMLTextAreaElement>(null);
  const submittingRef = useRef(false);
  const [submitting, setSubmitting] = useState(false);
  const activeQuestion =
    questions[Math.min(activeIndex, Math.max(questions.length - 1, 0))];
  const activeKey = activeQuestion ? questionKey(activeQuestion) : "";
  const activeCustomSelected = Boolean(customSelected[activeKey]);
  const activeCustomAnswer = customAnswers[activeKey] ?? "";
  const customLabel = t("pendingAction.interaction.custom");
  const visibleOptions = activeQuestion
    ? activeQuestion.options.filter(
        (option) => !isCustomOptionLabel(option.label, customLabel),
      )
    : [];

  function answerForQuestion(question: Question) {
    const key = questionKey(question);
    if (question.options.length === 0) return (answers[key] ?? "").trim();
    if (!customSelected[key]) return (answers[key] ?? "").trim();
    const customAnswer = (customAnswers[key] ?? "").trim();
    if (!question.multiSelect) return customAnswer;
    const values = valuesWithoutCustom(question, answers[key]);
    if (customAnswer) values.push(customAnswer);
    return values.join(", ").trim();
  }

  function resolvedAnswers() {
    return Object.fromEntries(
      questions.map((question) => [
        questionKey(question),
        answerForQuestion(question),
      ]),
    );
  }

  function firstUnansweredIndex() {
    return questions.findIndex((question) => !answerForQuestion(question));
  }

  async function runOnce(task: () => Promise<void> | void) {
    if (submittingRef.current) return;
    submittingRef.current = true;
    setSubmitting(true);
    try {
      await task();
    } finally {
      submittingRef.current = false;
      setSubmitting(false);
    }
  }

  function cancelInteraction() {
    return runOnce(() => onCancelInteraction(action.interaction.interactionId));
  }

  function submitInteractionChoice() {
    const unansweredIndex = firstUnansweredIndex();
    if (unansweredIndex >= 0) {
      const unansweredQuestion = questions[unansweredIndex];
      setActiveIndex(unansweredIndex);
      setValidationKey(questionKey(unansweredQuestion));
      return;
    }
    const finalAnswers = resolvedAnswers();
    const summary = answerSummary(finalAnswers);
    return runOnce(() =>
      onSubmitInteraction(action.interaction.interactionId, {
        answers: finalAnswers,
        annotations: {
          userChoiceSummary: t("pendingAction.interaction.submittedFeedback", {
            summary,
          }),
        },
      }),
    );
  }

  useEffect(() => {
    if (!validationKey) return;
    const validationQuestion = questions.find(
      (question) => questionKey(question) === validationKey,
    );
    if (validationQuestion && answerForQuestion(validationQuestion)) {
      setValidationKey(null);
    }
  }, [answers, customAnswers, customSelected, questions, validationKey]);

  useEffect(() => {
    if (!activeCustomSelected) return;
    fitTextareaToContent(customInputRef.current);
  }, [activeCustomAnswer, activeCustomSelected, activeKey]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      void cancelInteraction();
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  });

  function move(delta: number) {
    setActiveIndex((current) =>
      Math.min(Math.max(current + delta, 0), Math.max(questions.length - 1, 0)),
    );
  }

  function advanceAfterSingleChoice() {
    if (activeIndex >= questions.length - 1) return;
    setActiveIndex(activeIndex + 1);
  }

  function selectCustomAnswer(question: Question) {
    const key = questionKey(question);
    setCustomSelected((current) => ({ ...current, [key]: true }));
    setAnswers((current) => {
      if (!question.multiSelect) {
        return { ...current, [key]: customAnswers[key] ?? "" };
      }
      const values = valuesWithoutCustom(question, current[key]);
      const customAnswer = (customAnswers[key] ?? "").trim();
      if (customAnswer) values.push(customAnswer);
      return { ...current, [key]: values.join(", ") };
    });
    requestAnimationFrame(() => customInputRef.current?.focus());
  }

  function updateCustomAnswer(question: Question, value: string) {
    const key = questionKey(question);
    setCustomSelected((current) => ({ ...current, [key]: true }));
    setCustomAnswers((current) => ({ ...current, [key]: value }));
    setAnswers((current) => {
      if (!question.multiSelect) return { ...current, [key]: value };
      const values = valuesWithoutCustom(question, current[key]);
      if (value.trim()) values.push(value.trim());
      return { ...current, [key]: values.join(", ") };
    });
  }

  if (!activeQuestion) {
    return (
      <SurfaceShell title={t("pendingAction.interaction.title")}>
        <div className="flex justify-end">
          <Button
            type="button"
            variant="ghost"
            disabled={submitting}
            onClick={() => void cancelInteraction()}
          >
            {t("pendingAction.interaction.stop")}
          </Button>
        </div>
      </SurfaceShell>
    );
  }

  const questionActions = (
    <div className="flex flex-wrap items-center justify-end gap-2 py-1">
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="h-7 px-2 text-xs"
        aria-label={t("pendingAction.interaction.skip")}
        disabled={submitting}
        onClick={() => void cancelInteraction()}
      >
        {t("pendingAction.interaction.skip")}
        <kbd className="rounded-md bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">
          ESC
        </kbd>
      </Button>
      <Button
        type="button"
        size="sm"
        className="h-7 rounded-full px-4 text-xs"
        disabled={submitting}
        onClick={() => void submitInteractionChoice()}
      >
        {t("pendingAction.interaction.continue")}
        <span className="ml-0.5 inline-flex h-4 w-4 items-center justify-center rounded-full bg-primary-foreground/15">
          <CornerDownLeft className="h-3 w-3" aria-hidden="true" />
        </span>
      </Button>
    </div>
  );

  const customOption = (
    <label
      className={cn(
        "group flex min-h-9 min-w-0 flex-1 cursor-pointer items-start gap-3 rounded-md px-3 py-2 text-sm text-foreground transition-colors",
        activeCustomSelected ? "bg-muted/60" : "hover:bg-muted/40",
      )}
    >
      <span className="w-5 shrink-0 pt-0.5 text-right text-sm tabular-nums text-muted-foreground">
        {visibleOptions.length + 1}.
      </span>
      <input
        type={activeQuestion.multiSelect ? "checkbox" : "radio"}
        className="sr-only"
        name={`interaction-${action.interaction.interactionId}-${activeIndex}-${activeKey}`}
        aria-label={t("pendingAction.interaction.custom")}
        checked={activeCustomSelected}
        onChange={() => selectCustomAnswer(activeQuestion)}
      />
      {activeCustomSelected ? (
        <textarea
          ref={customInputRef}
          rows={1}
          aria-label={t("pendingAction.interaction.custom")}
          className="min-h-5 w-full resize-none overflow-hidden bg-transparent p-0 text-sm leading-5 text-foreground [font:inherit] align-top outline-none placeholder:text-muted-foreground"
          placeholder={t("pendingAction.interaction.customPlaceholder")}
          value={activeCustomAnswer}
          onFocus={() => selectCustomAnswer(activeQuestion)}
          onChange={(event) => {
            updateCustomAnswer(activeQuestion, event.target.value);
            fitTextareaToContent(event.currentTarget);
          }}
        />
      ) : (
        <span className="min-w-0 flex-1 leading-6 text-muted-foreground">
          {t("pendingAction.interaction.custom")}
        </span>
      )}
    </label>
  );

  return (
    <section className="rounded-lg border border-border bg-card p-3 shadow-[var(--shadow-md)]">
      <fieldset className="space-y-3">
        <div className="flex items-center justify-between gap-3 px-1">
          <legend className="text-sm font-semibold leading-6 text-foreground">
            {activeQuestion.question}
          </legend>
          {questions.length > 1 ? (
            <div className="flex shrink-0 items-center gap-2 text-xs text-muted-foreground">
              <button
                type="button"
                aria-label={t("pendingAction.interaction.previous")}
                className="inline-flex h-6 w-6 items-center justify-center rounded-md hover:bg-accent disabled:opacity-35"
                disabled={activeIndex === 0}
                onClick={() => move(-1)}
              >
                <ChevronLeft className="h-4 w-4" aria-hidden="true" />
              </button>
              <span>
                {t("pendingAction.interaction.progress", {
                  current: activeIndex + 1,
                  total: questions.length,
                })}
              </span>
              <button
                type="button"
                aria-label={t("pendingAction.interaction.next")}
                className="inline-flex h-6 w-6 items-center justify-center rounded-md hover:bg-accent disabled:opacity-35"
                disabled={activeIndex >= questions.length - 1}
                onClick={() => move(1)}
              >
                <ChevronRight className="h-4 w-4" aria-hidden="true" />
              </button>
            </div>
          ) : null}
        </div>

        {validationKey === activeKey ? (
          <p className="px-1 text-xs text-destructive">
            {t("pendingAction.interaction.validationRequired")}
          </p>
        ) : null}

        {activeQuestion.options.length > 0 ? (
          <div className="space-y-1">
            {visibleOptions.map((option, optionIndex) => {
              const selected = selectedValues(answers[activeKey]).includes(
                option.label,
              );
              return (
                <label
                  key={option.label}
                  className={cn(
                    "group flex min-h-9 cursor-pointer items-center gap-3 rounded-md px-3 py-2 text-sm text-foreground transition-colors",
                    selected ? "bg-muted/60" : "hover:bg-muted/40",
                  )}
                >
                  <span className="w-5 shrink-0 text-right text-sm tabular-nums text-muted-foreground">
                    {optionIndex + 1}.
                  </span>
                  <input
                    type={activeQuestion.multiSelect ? "checkbox" : "radio"}
                    className="sr-only"
                    name={`interaction-${action.interaction.interactionId}-${activeIndex}-${activeKey}`}
                    aria-label={option.label}
                    checked={selected}
                    onChange={(event) => {
                      if (!activeQuestion.multiSelect) {
                        setCustomSelected((current) => ({
                          ...current,
                          [activeKey]: false,
                        }));
                        setAnswers((current) => ({
                          ...current,
                          [activeKey]: option.label,
                        }));
                        advanceAfterSingleChoice();
                        return;
                      }
                      setAnswers((current) => {
                        const currentSelected = selectedValues(
                          current[activeKey],
                        );
                        const next = event.target.checked
                          ? [...currentSelected, option.label]
                          : currentSelected.filter(
                              (item) => item !== option.label,
                            );
                        return { ...current, [activeKey]: next.join(", ") };
                      });
                    }}
                  />
                  <span
                    className={cn(
                      "flex min-w-0 flex-1 items-center gap-1.5 leading-6",
                      selected && "font-medium",
                    )}
                  >
                    <span className="min-w-0 truncate">{option.label}</span>
                    <OptionInfo
                      content={option.preview ?? option.description}
                    />
                  </span>
                </label>
              );
            })}
            <div className="flex flex-wrap items-end gap-2">
              {customOption}
              <div className="ml-auto shrink-0 self-end">{questionActions}</div>
            </div>
          </div>
        ) : (
          <>
            <textarea
              aria-label={activeQuestion.question}
              className="min-h-24 w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={answers[activeKey] ?? ""}
              onChange={(event) =>
                setAnswers((current) => ({
                  ...current,
                  [activeKey]: event.target.value,
                }))
              }
            />
            {questionActions}
          </>
        )}
      </fieldset>
    </section>
  );
}

function OptionInfo({ content }: { content?: string }) {
  if (!content) return null;
  return (
    <TooltipProvider delayDuration={200}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            role="button"
            tabIndex={0}
            aria-label={content}
            className="inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:text-foreground"
            onClick={(event) => event.stopPropagation()}
            onKeyDown={(event) => event.stopPropagation()}
          >
            <Info className="h-3.5 w-3.5" aria-hidden="true" />
          </span>
        </TooltipTrigger>
        <TooltipContent side="top" className="max-w-72 whitespace-normal">
          {content}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

function actionKey(action: PendingAction) {
  switch (action.kind) {
    case "permission":
      return `permission:${action.ask.toolCallId}`;
    case "permission-group":
      return `permission-group:${action.asks.map((ask) => ask.toolCallId).join(",")}`;
    case "user-question":
      return `interaction:${action.interaction.interactionId}`;
    case "stale-permission":
      return `stale-permission:${action.toolCallId}`;
    case "stale-interaction":
      return `stale-interaction:${action.interactionId}`;
  }
}

function actionTabLabel(
  action: PendingAction,
  actions: PendingAction[],
  index: number,
  t: ReturnType<typeof useTranslation>["t"],
) {
  if (action.kind === "permission") {
    return t("pendingAction.tabs.permission", { name: action.ask.toolName });
  }
  if (action.kind === "permission-group") {
    const toolNames = new Set(action.asks.map((ask) => ask.toolName));
    return t("pendingAction.tabs.permission", {
      name:
        toolNames.size === 1
          ? (action.asks[0]?.toolName ?? "Permission")
          : `${action.asks.length} 个请求`,
    });
  }
  if (action.kind === "stale-permission") {
    return t("pendingAction.tabs.permission", { name: action.toolName });
  }
  const questionIndex =
    actions
      .slice(0, index + 1)
      .filter(
        (item) =>
          item.kind === "user-question" || item.kind === "stale-interaction",
      ).length || 1;
  return t("pendingAction.tabs.question", { index: questionIndex });
}

function StalePermissionPanel({
  action,
  onClearStalePermission,
}: Pick<Props, "onClearStalePermission"> & {
  action: Extract<PendingAction, { kind: "stale-permission" }>;
}) {
  const { t } = useTranslation();
  const submittingRef = useRef(false);
  const [submitting, setSubmitting] = useState(false);

  async function runOnce(task: () => Promise<void> | void) {
    if (submittingRef.current) return;
    submittingRef.current = true;
    setSubmitting(true);
    try {
      await task();
    } finally {
      submittingRef.current = false;
      setSubmitting(false);
    }
  }

  return (
    <SurfaceShell title={t("pendingAction.stalePermission.title")}>
      <div className="space-y-4">
        <p className="text-sm leading-6 text-foreground">
          {t("pendingAction.stalePermission.messageBefore")}
          <span className="font-medium">{action.toolName}</span>
          {t("pendingAction.stalePermission.messageAfter")}
        </p>
        <div className="flex justify-end">
          <Button
            type="button"
            variant="ghost"
            disabled={submitting}
            onClick={() =>
              void runOnce(() => onClearStalePermission(action.sessionId))
            }
          >
            {t("pendingAction.stalePermission.stop")}
          </Button>
        </div>
      </div>
    </SurfaceShell>
  );
}

function StaleInteractionPanel({
  action,
  onClearStaleInteraction,
}: Pick<Props, "onClearStaleInteraction"> & {
  action: Extract<PendingAction, { kind: "stale-interaction" }>;
}) {
  const { t } = useTranslation();
  const submittingRef = useRef(false);
  const [submitting, setSubmitting] = useState(false);

  async function runOnce(task: () => Promise<void> | void) {
    if (submittingRef.current) return;
    submittingRef.current = true;
    setSubmitting(true);
    try {
      await task();
    } finally {
      submittingRef.current = false;
      setSubmitting(false);
    }
  }

  return (
    <SurfaceShell title={t("pendingAction.staleInteraction.title")}>
      <div className="space-y-4">
        <p className="text-sm leading-6 text-foreground">
          {t("pendingAction.staleInteraction.messageBefore")}
          <span className="font-medium">{action.interactionKind}</span>
          {t("pendingAction.staleInteraction.messageAfter")}
        </p>
        <div className="flex justify-end">
          <Button
            type="button"
            variant="ghost"
            disabled={submitting}
            onClick={() =>
              void runOnce(() => onClearStaleInteraction(action.sessionId))
            }
          >
            {t("pendingAction.staleInteraction.stop")}
          </Button>
        </div>
      </div>
    </SurfaceShell>
  );
}

function ActiveActionPanel(props: Props & { action: PendingAction }) {
  if (props.action.kind === "permission") {
    return (
      <PermissionPanel
        key={props.action.ask.toolCallId}
        action={props.action}
        onAllowPermission={props.onAllowPermission}
        onDenyPermission={props.onDenyPermission}
      />
    );
  }

  if (props.action.kind === "permission-group") {
    return (
      <PermissionGroupPanel
        key={props.action.asks.map((ask) => ask.toolCallId).join(":")}
        action={props.action}
        onAllowPermission={props.onAllowPermission}
        onDenyPermission={props.onDenyPermission}
      />
    );
  }

  if (props.action.kind === "stale-permission") {
    return (
      <StalePermissionPanel
        key={props.action.toolCallId}
        action={props.action}
        onClearStalePermission={props.onClearStalePermission}
      />
    );
  }

  if (props.action.kind === "stale-interaction") {
    return (
      <StaleInteractionPanel
        key={props.action.interactionId}
        action={props.action}
        onClearStaleInteraction={props.onClearStaleInteraction}
      />
    );
  }

  return (
    <UserQuestionPanel
      key={props.action.interaction.interactionId}
      action={props.action}
      onSubmitInteraction={props.onSubmitInteraction}
      onCancelInteraction={props.onCancelInteraction}
    />
  );
}

export function PendingActionSurface(props: Props) {
  const { t } = useTranslation();
  const actions = Array.isArray(props.action) ? props.action : [props.action];
  const [activeKey, setActiveKey] = useState(() => actionKey(actions[0]));
  const activeAction =
    actions.find((action) => actionKey(action) === activeKey) ?? actions[0];
  const activeActionKey = actionKey(activeAction);

  useEffect(() => {
    if (actions.some((action) => actionKey(action) === activeKey)) return;
    setActiveKey(actionKey(actions[0]));
  }, [actions, activeKey]);

  if (actions.length <= 1) {
    return <ActiveActionPanel {...props} action={activeAction} />;
  }

  return (
    <div className="space-y-2">
      <div
        role="tablist"
        aria-label={t("pendingAction.tabs.label")}
        className="flex flex-wrap gap-1"
      >
        {actions.map((action, index) => {
          const key = actionKey(action);
          const selected = key === activeActionKey;
          return (
            <button
              key={key}
              type="button"
              role="tab"
              aria-selected={selected}
              className={cn(
                "rounded-md px-2.5 py-1 text-xs transition-colors",
                selected
                  ? "bg-muted text-foreground"
                  : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
              )}
              onClick={() => setActiveKey(key)}
            >
              {actionTabLabel(action, actions, index, t)}
            </button>
          );
        })}
      </div>
      <ActiveActionPanel {...props} action={activeAction} />
    </div>
  );
}
