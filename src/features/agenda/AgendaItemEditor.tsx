import { useEffect, useState } from "react";
import { Folder } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  type AgendaItem,
  type CreateAgendaItemRequest,
  type Freq,
  type RecurrenceRule,
  type UpdateAgendaItemRequest,
  createAgendaItem,
  getDefaultFolder,
  pickLocalDirectory,
  updateAgendaItem,
} from "@/lib/tauri";
import {
  Sheet,
  SheetContent,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { DateTimePicker } from "@/components/ui/date-time-picker";
import { FormField } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { NumberInput } from "@/components/ui/number-input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useHomeStore } from "@/stores/homeStore";

type Frequency = "one_shot" | Freq;

interface AgendaItemEditorProps {
  open: boolean;
  initial?: AgendaItem | null;
  initialDraft?: Partial<CreateAgendaItemRequest> | null;
  organizerEmployeeId?: string;
  onClose: () => void;
  onSaved: () => void;
}

const DEFAULT_AGENDA_ORGANIZER_ID = "default";

export function AgendaItemEditor({
  open,
  initial,
  initialDraft,
  organizerEmployeeId,
  onClose,
  onSaved,
}: AgendaItemEditorProps) {
  const { t, i18n } = useTranslation();
  const [title, setTitle] = useState("");
  const [prompt, setPrompt] = useState("");
  const [startAtLocal, setStartAtLocal] = useState("");
  const [timezone, setTimezone] = useState("Asia/Shanghai");
  const [frequency, setFrequency] = useState<Frequency>("one_shot");
  const [intervalCount, setIntervalCount] = useState(1);
  const [endKind, setEndKind] = useState<"never" | "count" | "until">("never");
  const [endCount, setEndCount] = useState(10);
  const [endUntilLocal, setEndUntilLocal] = useState("");
  const [workspacePath, setWorkspacePath] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const homeWorkspace = useHomeStore((s) => s.selectedWorkspace);

  useEffect(() => {
    if (initial) {
      setTitle(initial.title);
      setPrompt(initial.prompt);
      setStartAtLocal(toLocalInput(initial.startAt));
      setTimezone(initial.timezone);
      setFrequency(initial.rule?.freq ?? "one_shot");
      setIntervalCount(initial.rule?.interval ?? 1);
      const ec = initial.rule?.endCondition;
      if (!ec || ec.kind === "never") {
        setEndKind("never");
      } else if (ec.kind === "count") {
        setEndKind("count");
        setEndCount(ec.n);
      } else {
        setEndKind("until");
        setEndUntilLocal(toLocalInput(ec.at));
      }
      setWorkspacePath(initial.workspacePath ?? null);
    } else {
      setTitle(initialDraft?.title ?? "");
      setPrompt(initialDraft?.prompt ?? "");
      setStartAtLocal(
        initialDraft?.startAt
          ? toLocalInput(initialDraft.startAt)
          : defaultStartAtLocal(),
      );
      setTimezone(initialDraft?.timezone ?? "Asia/Shanghai");
      const draftRule = initialDraft?.rule ?? null;
      setFrequency(draftRule?.freq ?? "one_shot");
      setIntervalCount(draftRule?.interval ?? 1);
      const ec = draftRule?.endCondition;
      if (!ec || ec.kind === "never") {
        setEndKind("never");
      } else if (ec.kind === "count") {
        setEndKind("count");
        setEndCount(ec.n);
      } else {
        setEndKind("until");
        setEndUntilLocal(toLocalInput(ec.at));
      }
      // 新建：默认用 home picker 选过的 workspace；没选过就异步取 default folder
      if (initialDraft?.workspacePath !== undefined) {
        setWorkspacePath(initialDraft.workspacePath ?? null);
      } else if (homeWorkspace?.rootPath) {
        setWorkspacePath(homeWorkspace.rootPath);
      } else {
        setWorkspacePath(null);
        getDefaultFolder()
          .then((ws) => {
            if (ws?.rootPath) {
              setWorkspacePath((prev) => prev ?? ws.rootPath);
            }
          })
          .catch(() => {
            // 非致命：保留 null，后端会 fallback 到全局 workspace
          });
      }
    }
    setError(null);
  }, [initial, initialDraft, open, homeWorkspace, organizerEmployeeId]);

  const buildRule = (): RecurrenceRule | null => {
    if (frequency === "one_shot") return null;
    let endCondition: RecurrenceRule["endCondition"];
    if (endKind === "never") {
      endCondition = { kind: "never" };
    } else if (endKind === "count") {
      endCondition = { kind: "count", n: endCount };
    } else {
      endCondition = {
        kind: "until",
        at: new Date(endUntilLocal).toISOString(),
      };
    }
    return { freq: frequency, interval: intervalCount, endCondition };
  };

  const canSave = !!title && !!prompt && !!startAtLocal && !saving;

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    try {
      const startAt = new Date(startAtLocal).toISOString();
      if (initial) {
        const req: UpdateAgendaItemRequest = {
          title,
          prompt,
          startAt,
          timezone,
          rule: buildRule(),
          workspacePath,
        };
        await updateAgendaItem(initial.id, req);
      } else {
        const organizerId =
          (organizerEmployeeId ?? "").trim() || DEFAULT_AGENDA_ORGANIZER_ID;
        const req: CreateAgendaItemRequest = {
          title,
          prompt,
          startAt,
          timezone,
          organizerEmployeeId: organizerId,
          rule: buildRule(),
          workspacePath,
        };
        await createAgendaItem(req);
      }
      onSaved();
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const handlePickWorkspace = async () => {
    try {
      const path = await pickLocalDirectory({
        defaultPath: workspacePath ?? undefined,
        title: t("schedules.editor.fields.pickDialogTitle"),
      });
      if (path) setWorkspacePath(path);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <Sheet open={open} onOpenChange={(v) => !v && onClose()}>
      <SheetContent
        data-aijia-agenda-editor
        className="w-[480px] flex flex-col gap-0 overflow-hidden p-0"
      >
        <SheetHeader
          data-aijia-agenda-header
          className="h-[3.5rem] shrink-0 justify-center border-b border-border px-6 py-0"
        >
          <SheetTitle className="text-md">
            {initial
              ? t("schedules.editor.titleEdit")
              : t("schedules.editor.titleNew")}
          </SheetTitle>
        </SheetHeader>

        <div
          data-aijia-agenda-form-body
          className="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 py-5"
        >
          <FormField
            htmlFor="agenda-editor-title"
            label={t("schedules.editor.fields.title")}
          >
            <Input
              id="agenda-editor-title"
              placeholder={t("schedules.editor.fields.title")}
              data-aijia-agenda-field="title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
          </FormField>
          <FormField
            htmlFor="agenda-editor-prompt"
            label={t("schedules.editor.fields.promptPlaceholder")}
          >
            <Textarea
              id="agenda-editor-prompt"
              placeholder={t("schedules.editor.fields.promptPlaceholder")}
              data-aijia-agenda-field="prompt"
              rows={4}
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
            />
          </FormField>

          <FormField label={t("schedules.editor.fields.frequency")}>
            <Select
              value={frequency}
              onValueChange={(value: string) =>
                setFrequency(value as Frequency)
              }
            >
              <SelectTrigger
                aria-label={t("schedules.editor.fields.frequency")}
                data-aijia-agenda-field="frequency"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  value="one_shot"
                  data-aijia-agenda-option="one_shot"
                >
                  {t("schedules.editor.freqOptions.oneShot")}
                </SelectItem>
                <SelectItem value="daily" data-aijia-agenda-option="daily">
                  {t("schedules.editor.freqOptions.daily")}
                </SelectItem>
                <SelectItem value="weekly" data-aijia-agenda-option="weekly">
                  {t("schedules.editor.freqOptions.weekly")}
                </SelectItem>
                <SelectItem value="monthly" data-aijia-agenda-option="monthly">
                  {t("schedules.editor.freqOptions.monthly")}
                </SelectItem>
                <SelectItem value="yearly" data-aijia-agenda-option="yearly">
                  {t("schedules.editor.freqOptions.yearly")}
                </SelectItem>
              </SelectContent>
            </Select>
          </FormField>

          {frequency !== "one_shot" ? (
            <div className="space-y-4">
              <FormField
                label={t("schedules.editor.fields.intervalEvery", {
                  unit: t(`schedules.frequency.noun.${frequency}`),
                })}
              >
                <NumberInput
                  min={1}
                  value={intervalCount}
                  onValueChange={setIntervalCount}
                  aria-label={t("schedules.editor.fields.intervalAria")}
                />
              </FormField>

              <FormField label={t("schedules.editor.fields.endCondition")}>
                <Select
                  value={endKind}
                  onValueChange={(value: string) =>
                    setEndKind(value as "never" | "count" | "until")
                  }
                >
                  <SelectTrigger
                    aria-label={t("schedules.editor.fields.endCondition")}
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="never">
                      {t("schedules.editor.endOptions.never")}
                    </SelectItem>
                    <SelectItem value="count">
                      {t("schedules.editor.endOptions.count")}
                    </SelectItem>
                    <SelectItem value="until">
                      {t("schedules.editor.endOptions.until")}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </FormField>
              {endKind === "count" ? (
                <NumberInput
                  min={1}
                  value={endCount}
                  onValueChange={setEndCount}
                  aria-label={t("schedules.editor.fields.endCountAria")}
                />
              ) : null}
              {endKind === "until" ? (
                <DateTimePicker
                  value={endUntilLocal}
                  onChange={setEndUntilLocal}
                  label={t("schedules.editor.fields.endUntilAria")}
                  placeholder={t("schedules.editor.fields.endUntilAria")}
                  level="minute"
                  locale={i18n.language}
                />
              ) : null}
            </div>
          ) : null}

          <FormField
            htmlFor="agenda-editor-start"
            label={t("schedules.editor.fields.startTime")}
          >
            <DateTimePicker
              id="agenda-editor-start"
              value={startAtLocal}
              onChange={setStartAtLocal}
              label={t("schedules.editor.fields.startTime")}
              placeholder={t("schedules.editor.fields.startTime")}
              level="minute"
              locale={i18n.language}
            />
          </FormField>

          <FormField
            label={t("schedules.editor.fields.workspace")}
            description={t("schedules.editor.fields.workspaceHint")}
          >
            <div className="flex items-center gap-2">
              <div
                className="flex h-9 flex-1 items-center truncate rounded-md border border-input bg-card px-3 py-2 text-sm"
                title={workspacePath ?? undefined}
              >
                {workspacePath ?? t("schedules.editor.fields.workspaceDefault")}
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handlePickWorkspace}
                aria-label={t("schedules.editor.fields.pickWorkspaceAria")}
              >
                <Folder className="h-4 w-4" />
                {t("schedules.editor.fields.pick")}
              </Button>
            </div>
          </FormField>

          {error ? (
            <div className="text-xs text-destructive">{error}</div>
          ) : null}
        </div>

        <SheetFooter
          data-aijia-agenda-footer
          className="h-[4.0625rem] shrink-0 flex-row items-center justify-end gap-2 border-t border-border px-6 py-0 sm:justify-end sm:space-x-0"
        >
          <Button
            variant="outline"
            onClick={onClose}
            disabled={saving}
            data-aijia-agenda-action="cancel"
          >
            {t("schedules.editor.actions.cancel")}
          </Button>
          <Button
            onClick={handleSave}
            disabled={!canSave}
            data-aijia-agenda-action="save"
          >
            {t("schedules.editor.actions.save")}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}

function toLocalInput(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function defaultStartAtLocal(): string {
  const d = new Date();
  if (d.getSeconds() > 0 || d.getMilliseconds() > 0) {
    d.setMinutes(d.getMinutes() + 1);
  }
  d.setSeconds(0, 0);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}
