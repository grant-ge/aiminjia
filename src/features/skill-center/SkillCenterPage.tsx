import { MoreHorizontal, Search, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";

import { AppDropdown } from "@/components/common/AppDropdown";
import { requestConfirm } from "@/components/common/ConfirmDialogHost";
import { PageSectionShell } from "@/components/shell/PageSectionShell";
import { PageTopBar } from "@/components/shell/PageTopBar";
import { SkillCard } from "@/components/skills/SkillCard";
import { SkillCategoryBar } from "@/components/skills/SkillCategoryBar";
import { SkillOfficeSection } from "@/components/skills/SkillOfficeSection";
import { getSkillAvatarClass } from "@/components/skills/skillVisual";
import { Button } from "@/components/ui/button";
import {
  SKILL_CATEGORIES,
  type SkillCategoryId,
} from "@/data/skill-categories";
import { useChat } from "@/hooks/useChat";
import { refreshSkillRegistry, syncBuiltinSkills } from "@/lib/tauri";
import { localizeSkill } from "@/lib/skillLocalization";
import { useAuthStore } from "@/stores/authStore";
import { useNotificationStore } from "@/stores/notificationStore";
import { useSkillStore } from "@/stores/skillStore";
import { useUiStore } from "@/stores/uiStore";

import { SkillValidationResultDialog } from "./SkillValidationResultDialog";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  SkillValidationError,
  type SkillValidationKind,
} from "@/stores/skillStore";
import { uploadWithOverwriteConfirm } from "./uploadWithOverwriteConfirm";
import {
  ChevronDown,
  Cloud,
  FolderOpen,
  HardDrive,
  Package,
} from "lucide-react";

const SKILL_LOGOS_BY_ID: Record<string, string> = {
  "html-ppt": "/skill-avatars/pptx-generator.svg",
  browser: "/skill-avatars/web-access.svg",
  payslip: "/skill-avatars/smart-payslip.jpg",
  smartcb: "/skill-avatars/smart-compensation.jpg",
  rehcm: "/skill-avatars/renlijia-hr.jpg",
  "dingtalk-workspace": "/logos/dingtalk.png",
  "xiaojia-doctor": "/brand-avatar-gold.svg",
};

function getSkillAvatar(skillId: string) {
  const brandLogo = SKILL_LOGOS_BY_ID[skillId];
  if (brandLogo) {
    return (
      <img
        src={brandLogo}
        alt=""
        draggable={false}
        className="h-full w-full rounded-md object-cover"
      />
    );
  }

  return null;
}

export function SkillCenterPage() {
  const { t, i18n } = useTranslation();
  const [category, setCategory] = useState<SkillCategoryId>("recommended");
  const [query, setQuery] = useState("");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [validationFailure, setValidationFailure] = useState<{
    kind: SkillValidationKind;
    detail?: string;
  } | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [checkingId, setCheckingId] = useState<string | null>(null);
  const skills = useSkillStore((s) => s.skills);
  const isLoading = useSkillStore((s) => s.isLoading);
  const reload = useSkillStore((s) => s.reload);
  const upload = useSkillStore((s) => s.upload);
  const uninstall = useSkillStore((s) => s.uninstall);
  const listByCategory = useSkillStore((s) => s.listByCategory);
  const setRoute = useUiStore((s) => s.setRoute);
  const pushNotification = useNotificationStore((s) => s.push);
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn);
  useChat();

  const runInstall = useCallback(
    async (picked: string) => {
      try {
        const outcome = await uploadWithOverwriteConfirm((force) =>
          upload(picked, force),
        );
        if (outcome === "installed") {
          pushNotification({
            level: "success",
            title: t("skillCenter.uploadSuccess"),
            message: t("skillCenter.uploadSuccessDesc"),
            actions: [],
            dismissible: true,
            autoHide: 4,
            context: "toast",
          });
        }
      } catch (err) {
        if (err instanceof SkillValidationError) {
          setValidationFailure({ kind: err.kind, detail: err.detail });
          return;
        }
        pushNotification({
          level: "error",
          title: t("skillCenter.uploadFailed"),
          message: err instanceof Error ? err.message : String(err),
          actions: [],
          dismissible: true,
          autoHide: 6,
          context: "toast",
        });
      }
    },
    [pushNotification, t, upload],
  );

  const handleImportDirectory = useCallback(async () => {
    if (import.meta.env.DEV) {
      const queue = (
        window as unknown as {
          __aijia?: { _pickSkillImportMockQueue?: string[] };
        }
      ).__aijia?._pickSkillImportMockQueue;
      if (queue && queue.length > 0) {
        const mocked = queue.shift();
        if (mocked) {
          await runInstall(mocked);
          return;
        }
      }
    }
    const picked = await openDialog({
      directory: true,
      multiple: false,
      title: t("skillCenter.selectDir"),
    });
    if (!picked || Array.isArray(picked)) return;
    await runInstall(picked);
  }, [runInstall, t]);

  const handleImportArchive = useCallback(async () => {
    if (import.meta.env.DEV) {
      const queue = (
        window as unknown as {
          __aijia?: { _pickSkillImportMockQueue?: string[] };
        }
      ).__aijia?._pickSkillImportMockQueue;
      if (queue && queue.length > 0) {
        const mocked = queue.shift();
        if (mocked) {
          await runInstall(mocked);
          return;
        }
      }
    }
    const picked = await openDialog({
      directory: false,
      multiple: false,
      title: t("skillCenter.selectArchive"),
      filters: [{ name: t("skillCenter.archiveFilter"), extensions: ["zip"] }],
    });
    if (!picked || Array.isArray(picked)) return;
    await runInstall(picked);
  }, [runInstall, t]);

  const handleDeleteSkill = async (skillId: string, displayName: string) => {
    const confirmed = await requestConfirm({
      title: t("skillCenter.deleteSkill"),
      description: t("skillCenter.deleteConfirm", { name: displayName }),
      confirmLabel: t("skillCenter.deleteLabel"),
      cancelLabel: t("skillCenter.cancelLabel"),
      variant: "destructive",
    });
    if (!confirmed) return;
    try {
      await uninstall(skillId);
      pushNotification({
        level: "success",
        title: t("skillCenter.skillDeleted"),
        message: t("skillCenter.skillDeletedDesc", { name: displayName }),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: "toast",
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      pushNotification({
        level: "error",
        title: t("skillCenter.deleteFailed"),
        message,
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: "toast",
      });
    }
  };

  const handleSyncBuiltin = async () => {
    if (syncing) return;
    setSyncing(true);
    try {
      const result = await syncBuiltinSkills();
      await reload();
      pushNotification({
        level: "success",
        title:
          result.installed.length > 0
            ? t("skillCenter.syncDone", { count: result.installed.length })
            : t("skillCenter.syncUpToDate"),
        message: "",
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: "toast",
      });
    } catch (err) {
      pushNotification({
        level: "error",
        title: t("skillCenter.syncFailed"),
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: "toast",
      });
    } finally {
      setSyncing(false);
    }
  };

  /** 同步本地技能：让后端重扫 user/global skills 目录，把新装但内存 registry
   *  还不知道的技能同步上来。用于"AI 用 lotus_skill.py 装完技能，但 registry 没刷新"
   *  这种 disk-app 不同步的兜底场景。后端 refresh_skill_registry 内部会发
   *  TAURI_EVENTS.SKILL_REGISTRY_REFRESHED 事件，AuthGate 监听后会自动 reload，
   *  这里再显式 reload 一次防止网络抖动遗漏。 */
  const handleSyncLocal = async () => {
    if (syncing) return;
    setSyncing(true);
    try {
      await refreshSkillRegistry();
      await reload();
      pushNotification({
        level: "success",
        title: t("skillCenter.syncLocalDone"),
        message: "",
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: "toast",
      });
    } catch (err) {
      pushNotification({
        level: "error",
        title: t("skillCenter.syncFailed"),
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: "toast",
      });
    } finally {
      setSyncing(false);
    }
  };

  /**
   * Per-card check-update: reuses the global sync IPC and inspects the
   * `installed` array to surface a card-targeted toast. Avoids a separate
   * per-skill backend command — the OPS list API has no per-skill query
   * so a dedicated command would still fetch the whole list.
   */
  const handleCheckSkillUpdate = async (
    skillId: string,
    displayName: string,
  ) => {
    if (checkingId || syncing) return;
    setCheckingId(skillId);
    try {
      const result = await syncBuiltinSkills();
      await reload();
      const updated = result.installed.includes(skillId);
      pushNotification({
        level: "success",
        title: updated ? t("skillCenter.hasUpdate") : t("skillCenter.upToDate"),
        message: displayName,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: "toast",
      });
    } catch (err) {
      pushNotification({
        level: "error",
        title: t("skillCenter.checkUpdateFailed"),
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: "toast",
      });
    } finally {
      setCheckingId(null);
    }
  };

  const handleExportSkill = async (skillId: string, displayName: string) => {
    try {
      const dest = await invoke<string>("export_installed_skill", { skillId });
      pushNotification({
        level: "success",
        title: t("skillCenter.exported"),
        message: t("skillCenter.exportedDesc", { name: displayName, dest }),
        actions: [],
        dismissible: true,
        autoHide: 10,
        context: "toast",
      });
    } catch (err) {
      pushNotification({
        level: "error",
        title: t("skillCenter.exportFailed"),
        message: String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: "toast",
      });
    }
  };

  const handleLoadError = (error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    setLoadError(message);
    console.error("Failed to load skills:", error);
  };

  const loadSkills = useCallback(async () => {
    setLoadError(null);
    try {
      await reload();
    } catch (error) {
      handleLoadError(error);
    }
  }, [reload]);

  useEffect(() => {
    void reload().catch(handleLoadError);
  }, [reload]);

  const categoryItems = useMemo(
    () => [
      { key: "recommended", label: t("skillCenter.allCategory") },
      ...SKILL_CATEGORIES.map((c) => ({ key: c.id, label: c.name })),
    ],
    [t],
  );

  const normalizedQuery = query.trim().toLowerCase();
  const matchesQuery = (skill: (typeof skills)[number]) => {
    if (!normalizedQuery) return true;
    return [
      skill.displayName,
      skill.displayNameEn,
      skill.description,
      skill.shortDescription,
      skill.shortDescriptionEn,
      skill.triggerText,
      skill.category,
    ].some((value) => value?.toLowerCase().includes(normalizedQuery));
  };

  const officeSkills =
    category === "recommended"
      ? skills.filter(matchesQuery)
      : category === "mine"
        ? skills.filter((s) => s.source === "user").filter(matchesQuery)
        : category === "tenant"
          ? skills.filter((s) => s.source === "tenant").filter(matchesQuery)
          : listByCategory(category).filter(matchesQuery);

  function getSkillMeta(source: string, cat: string) {
    const normalizedCategory = cat || "general";
    const label =
      SKILL_CATEGORIES.find((c) => c.id === normalizedCategory)?.name ??
      t("skillCenter.defaultCategory");
    // Backend emits: 'user' (local upload/own scope), 'tenant' (pushed by
    // tenant admin via lotus tenant-portal), 'global' (platform/OPS public),
    // 'builtin' (legacy fixture in tests). Surface each so users can tell
    // why a skill exists and who can update it.
    let sourceLabel: string;
    switch (source) {
      case "user":
      case "builtin":
        sourceLabel = t("skillCenter.sourceUser");
        break;
      case "tenant":
        sourceLabel = t("skillCenter.sourceTenant");
        break;
      case "global":
        sourceLabel = t("skillCenter.sourcePlatform");
        break;
      default:
        sourceLabel = t("skillCenter.custom");
    }
    return `${sourceLabel} · ${label}`;
  }

  return (
    <>
      <PageSectionShell
        topBar={
          <PageTopBar
            variant="title"
            title={
              <div className="flex min-w-0 items-center gap-2.5">
                <span className="truncate text-[15px] font-semibold leading-[22px] text-foreground">
                  {t("skillCenter.title")}
                </span>
                <span className="rounded-md bg-secondary px-2 py-0.5 text-xs font-medium text-muted-foreground">
                  {t("skillCenter.installedCount", { count: skills.length })}
                </span>
              </div>
            }
            trailing={
              <>
                <div className="flex h-9 w-[240px] items-center gap-2 rounded-md border border-input bg-card px-3">
                  <Search className="h-4 w-4 shrink-0 text-muted-foreground" />
                  <input
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                    className="min-w-0 flex-1 bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground"
                    placeholder={t("skillCenter.searchPlaceholder")}
                  />
                </div>
                {isLoggedIn && (
                  <AppDropdown
                    ariaLabel={t("skillCenter.syncSkills")}
                    trigger={
                      <Button
                        size="md"
                        variant="outline"
                        disabled={syncing}
                        data-testid="skills-sync-builtin"
                      >
                        {syncing
                          ? t("skillCenter.syncing")
                          : t("skillCenter.syncSkills")}
                        <ChevronDown className="h-3.5 w-3.5" />
                      </Button>
                    }
                    items={[
                      {
                        id: "sync-builtin",
                        label: t("skillCenter.syncBuiltin"),
                        icon: <Cloud className="h-4 w-4" />,
                        onSelect: () => void handleSyncBuiltin(),
                        dataAttrs: {
                          "data-aijia-skill-sync-action": "builtin",
                        },
                      },
                      {
                        id: "sync-local",
                        label: t("skillCenter.syncLocal"),
                        icon: <HardDrive className="h-4 w-4" />,
                        onSelect: () => void handleSyncLocal(),
                        dataAttrs: { "data-aijia-skill-sync-action": "local" },
                      },
                    ]}
                  />
                )}
                <AppDropdown
                  ariaLabel={t("skillCenter.importSkill")}
                  trigger={
                    <Button size="md" data-aijia-skill-import-trigger>
                      {t("skillCenter.importSkill")}
                      <ChevronDown className="h-3.5 w-3.5" />
                    </Button>
                  }
                  items={[
                    {
                      id: "import-dir",
                      label: t("skillCenter.importDirectory"),
                      icon: <FolderOpen className="h-4 w-4" />,
                      onSelect: () => void handleImportDirectory(),
                      dataAttrs: {
                        "data-aijia-skill-import-action": "directory",
                      },
                    },
                    {
                      id: "import-archive",
                      label: t("skillCenter.importArchive"),
                      icon: <Package className="h-4 w-4" />,
                      onSelect: () => void handleImportArchive(),
                      dataAttrs: {
                        "data-aijia-skill-import-action": "archive",
                      },
                    },
                  ]}
                />
              </>
            }
          />
        }
      >
        <SkillOfficeSection
          categoryBar={
            <SkillCategoryBar
              items={categoryItems}
              activeKey={category}
              onSelect={(key) => setCategory(key as SkillCategoryId)}
            />
          }
        >
          {isLoading && skills.length === 0 ? (
            <SkillCenterState title={t("skillCenter.loading")} />
          ) : loadError && skills.length === 0 ? (
            <SkillCenterState
              title={t("skillCenter.loadFailed")}
              desc={loadError}
              actionLabel={t("skillCenter.retry")}
              onAction={() => void loadSkills()}
            />
          ) : officeSkills.length === 0 ? (
            category === "mine" ? (
              <SkillCenterState
                title={t("skillCenter.noLocalSkills")}
                desc={t("skillCenter.noLocalSkillsDesc")}
              />
            ) : normalizedQuery ? (
              <SkillCenterState
                title={t("skillCenter.noMatch")}
                desc={t("skillCenter.noMatchDesc", { query: normalizedQuery })}
              />
            ) : (
              <SkillCenterState
                title={t("skillCenter.noSkillsInCategory")}
                desc={t("skillCenter.noSkillsInCategoryDesc")}
              />
            )
          ) : (
            officeSkills.map((skill) => {
              const localized = localizeSkill(skill, i18n.language);
              const isUserSkill = skill.source === "user";
              const menuItems: Array<{
                id: string;
                label: string;
                icon?: React.ReactNode;
                className?: string;
                disabled?: boolean;
                onSelect: () => void;
              }> = [];
              if (isUserSkill) {
                menuItems.push({
                  id: "export",
                  label: t("skillCenter.exportLabel"),
                  onSelect: () =>
                    void handleExportSkill(skill.id, localized.name),
                });
                menuItems.push({
                  id: "delete",
                  label: t("skillCenter.deleteSkill"),
                  icon: <Trash2 />,
                  className: "text-destructive [&_svg]:text-destructive",
                  onSelect: () =>
                    void handleDeleteSkill(skill.id, localized.name),
                });
              } else if (isLoggedIn) {
                // Non-user skills (builtin / global) can be re-synced from OPS.
                menuItems.push({
                  id: "check-update",
                  label:
                    checkingId === skill.id
                      ? t("skillCenter.checking")
                      : t("skillCenter.checkUpdate"),
                  disabled: checkingId === skill.id || syncing,
                  onSelect: () =>
                    void handleCheckSkillUpdate(skill.id, localized.name),
                });
              }
              return (
                <SkillCard
                  key={skill.id}
                  title={localized.name}
                  meta={getSkillMeta(skill.source, skill.category)}
                  desc={localized.description}
                  iconNode={getSkillAvatar(
                    skill.id,
                  )}
                  iconBg={getSkillAvatarClass(skill.category)}
                  version={skill.version}
                  skillId={skill.id}
                  skillSource={skill.source}
                  onClick={() =>
                    setRoute({ kind: "skill-detail", skillId: skill.id })
                  }
                  actionsSlot={
                    menuItems.length === 0 ? (
                      <div aria-hidden="true" className="h-8 w-8" />
                    ) : (
                      <AppDropdown
                        ariaLabel={`${localized.name} ${t("skillCenter.moreActions")}`}
                        trigger={
                          <Button unstyled
                            type="button"
                            className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                          >
                            <MoreHorizontal className="h-4 w-4" />
                          </Button>
                        }
                        items={menuItems}
                      />
                    )
                  }
                />
              );
            })
          )}
        </SkillOfficeSection>
      </PageSectionShell>
      <SkillValidationResultDialog
        open={validationFailure !== null}
        onOpenChange={(next) => {
          if (!next) setValidationFailure(null);
        }}
        failure={validationFailure}
        onRetry={() => {
          setValidationFailure(null);
          void handleImportDirectory();
        }}
      />
    </>
  );
}

function SkillCenterState({
  title,
  desc,
  actionLabel,
  onAction,
}: {
  title: string;
  desc?: string;
  actionLabel?: string;
  onAction?: () => void;
}) {
  return (
    <div className="col-span-full rounded-md border border-dashed border-border bg-card p-6 text-sm shadow-[var(--shadow-card)]">
      <div className="font-semibold text-foreground">{title}</div>
      {desc ? <p className="mt-1 text-muted-foreground">{desc}</p> : null}
      {actionLabel && onAction ? (
        <Button className="mt-3" variant="outline" size="sm" onClick={onAction}>
          {actionLabel}
        </Button>
      ) : null}
    </div>
  );
}
