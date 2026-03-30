/**
 * Data formatting utilities used across the app.
 *
 * Date/number formatting uses the current i18n locale by default.
 * No external dependencies — relies entirely on native Intl APIs + i18next.
 */
import i18n from '@/i18n'

/** Get the current locale string for Intl APIs. */
function currentLocale(): string {
  return i18n.language || 'zh-CN'
}

/**
 * Format bytes to a human-readable size string.
 *
 * @param bytes - The number of bytes to format.
 * @returns A formatted string with the appropriate unit (B, KB, MB, GB).
 *
 * @example
 * formatFileSize(0)          // "0 B"
 * formatFileSize(1024)       // "1.00 KB"
 * formatFileSize(1536)       // "1.50 KB"
 * formatFileSize(1048576)    // "1.00 MB"
 * formatFileSize(1073741824) // "1.00 GB"
 */
export function formatFileSize(bytes: number): string {
  if (bytes === 0) return "0 B";

  const units = ["B", "KB", "MB", "GB"];
  const base = 1024;
  const unitIndex = Math.min(
    Math.floor(Math.log(bytes) / Math.log(base)),
    units.length - 1,
  );
  const value = bytes / Math.pow(base, unitIndex);

  // Show decimals only for KB and above
  if (unitIndex === 0) {
    return `${value} B`;
  }

  return `${value.toFixed(2)} ${units[unitIndex]}`;
}

/**
 * Format an ISO date string to a localized display string.
 *
 * @param isoString - An ISO 8601 date string (e.g., "2025-12-15T14:30:00Z").
 * @returns A formatted date string in the current locale.
 */
export function formatDate(isoString: string): string {
  const date = new Date(isoString);
  const locale = currentLocale();

  const dateFormatter = new Intl.DateTimeFormat(locale, {
    year: "numeric",
    month: "long",
    day: "numeric",
  });

  const timeFormatter = new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });

  return `${dateFormatter.format(date)} ${timeFormatter.format(date)}`;
}

/**
 * Format an ISO date string as a relative time description.
 *
 * Uses i18n translation keys for the time labels.
 *
 * @param isoString - An ISO 8601 date string.
 * @returns A relative time string in the current language.
 */
export function formatRelativeTime(isoString: string): string {
  const date = new Date(isoString);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSeconds = Math.floor(diffMs / 1000);
  const diffMinutes = Math.floor(diffSeconds / 60);
  const diffHours = Math.floor(diffMinutes / 60);
  const diffDays = Math.floor(diffHours / 24);
  const diffMonths = Math.floor(diffDays / 30);
  const diffYears = Math.floor(diffDays / 365);

  if (diffSeconds < 60) {
    return i18n.t("format.justNow");
  }

  if (diffMinutes < 60) {
    return i18n.t("format.minutesAgo", { count: diffMinutes });
  }

  if (diffHours < 24) {
    return i18n.t("format.hoursAgo", { count: diffHours });
  }

  if (diffDays === 1) {
    return i18n.t("format.yesterday");
  }

  if (diffDays < 30) {
    return i18n.t("format.daysAgo", { count: diffDays });
  }

  if (diffMonths < 12) {
    return i18n.t("format.monthsAgo", { count: diffMonths });
  }

  return i18n.t("format.yearsAgo", { count: diffYears });
}

/**
 * Format a number as a currency string.
 *
 * Defaults to CNY (Chinese Yuan, "¥") when no currency code is provided.
 *
 * @param amount - The numeric amount to format.
 * @param currency - An ISO 4217 currency code (default: "CNY").
 * @returns A formatted currency string in the current locale.
 */
export function formatCurrency(amount: number, currency: string = "CNY"): string {
  return new Intl.NumberFormat(currentLocale(), {
    style: "currency",
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(amount);
}

/**
 * Format a number as a percentage string.
 *
 * The input value is treated as a raw number (not a fraction), so
 * `84.6` becomes `"84.6%"`, not `"8460%"`.
 *
 * @param value - The numeric value to format as a percentage.
 * @param decimals - Number of decimal places to display (default: 1).
 * @returns A formatted percentage string.
 */
export function formatPercentage(value: number, decimals: number = 1): string {
  return `${value.toFixed(decimals)}%`;
}

/**
 * Truncate text to a maximum length and append an ellipsis if truncated.
 *
 * @param text - The text string to truncate.
 * @param maxLength - The maximum number of characters before truncation.
 * @returns The original or truncated text.
 */
export function truncateText(text: string, maxLength: number): string {
  if (text.length <= maxLength) {
    return text;
  }

  return `${text.slice(0, maxLength)}...`;
}

/**
 * Format a large number with comma separators for readability.
 *
 * Uses the current locale grouping.
 *
 * @param value - The number to format.
 * @returns A formatted number string with commas.
 */
export function formatNumber(value: number): string {
  return new Intl.NumberFormat(currentLocale()).format(value);
}
