/**
 * Data formatting utilities used across the app.
 *
 * All date/number formatting uses the zh-CN locale by default.
 * No external dependencies — relies entirely on native Intl APIs.
 */

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
 * Format an ISO date string to a localized Chinese display string.
 *
 * @param isoString - An ISO 8601 date string (e.g., "2025-12-15T14:30:00Z").
 * @returns A formatted date string in zh-CN locale.
 *
 * @example
 * formatDate("2025-12-15T14:30:00Z") // "2025年12月15日 14:30"
 * formatDate("2024-01-01T00:00:00Z") // "2024年1月1日 00:00"
 */
export function formatDate(isoString: string): string {
  const date = new Date(isoString);

  const dateFormatter = new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "long",
    day: "numeric",
  });

  const timeFormatter = new Intl.DateTimeFormat("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });

  return `${dateFormatter.format(date)} ${timeFormatter.format(date)}`;
}

/**
 * Format an ISO date string as a relative time description in Chinese.
 *
 * Returns human-friendly descriptions like "刚刚", "3 分钟前", "2 小时前",
 * "昨天", or falls back to an absolute date for older dates.
 *
 * @param isoString - An ISO 8601 date string.
 * @returns A relative time string in Chinese.
 *
 * @example
 * // Assuming current time is 2025-12-15T14:30:00Z:
 * formatRelativeTime("2025-12-15T14:29:30Z") // "刚刚"
 * formatRelativeTime("2025-12-15T14:27:00Z") // "3 分钟前"
 * formatRelativeTime("2025-12-15T12:30:00Z") // "2 小时前"
 * formatRelativeTime("2025-12-14T10:00:00Z") // "昨天"
 * formatRelativeTime("2025-12-13T10:00:00Z") // "2 天前"
 * formatRelativeTime("2025-11-15T10:00:00Z") // "1 个月前"
 * formatRelativeTime("2024-12-15T10:00:00Z") // "1 年前"
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
    return "刚刚";
  }

  if (diffMinutes < 60) {
    return `${diffMinutes} 分钟前`;
  }

  if (diffHours < 24) {
    return `${diffHours} 小时前`;
  }

  if (diffDays === 1) {
    return "昨天";
  }

  if (diffDays < 30) {
    return `${diffDays} 天前`;
  }

  if (diffMonths < 12) {
    return `${diffMonths} 个月前`;
  }

  return `${diffYears} 年前`;
}

/**
 * Format a number as a currency string.
 *
 * Defaults to CNY (Chinese Yuan, "¥") when no currency code is provided.
 *
 * @param amount - The numeric amount to format.
 * @param currency - An ISO 4217 currency code (default: "CNY").
 * @returns A formatted currency string in zh-CN locale.
 *
 * @example
 * formatCurrency(12345)           // "¥12,345.00"
 * formatCurrency(12345.678)       // "¥12,345.68"
 * formatCurrency(99.9, "USD")     // "US$99.90"
 * formatCurrency(0)               // "¥0.00"
 */
export function formatCurrency(amount: number, currency: string = "CNY"): string {
  return new Intl.NumberFormat("zh-CN", {
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
 *
 * @example
 * formatPercentage(84.6)      // "84.6%"
 * formatPercentage(100)       // "100.0%"
 * formatPercentage(0.5, 2)    // "0.50%"
 * formatPercentage(33.333, 0) // "33%"
 */
export function formatPercentage(value: number, decimals: number = 1): string {
  return `${value.toFixed(decimals)}%`;
}

/**
 * Truncate text to a maximum length and append an ellipsis if truncated.
 *
 * If the text is shorter than or equal to `maxLength`, it is returned as-is.
 * Otherwise, it is cut at `maxLength` characters and "..." is appended.
 *
 * @param text - The text string to truncate.
 * @param maxLength - The maximum number of characters before truncation.
 * @returns The original or truncated text.
 *
 * @example
 * truncateText("Hello", 10)                        // "Hello"
 * truncateText("Hello, World!", 5)                  // "Hello..."
 * truncateText("这是一段很长的中文文本", 6)            // "这是一段很长..."
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
 * Uses the zh-CN locale grouping (standard 3-digit comma separation).
 *
 * @param value - The number to format.
 * @returns A formatted number string with commas.
 *
 * @example
 * formatNumber(1032)       // "1,032"
 * formatNumber(1000000)    // "1,000,000"
 * formatNumber(42)         // "42"
 * formatNumber(1234567.89) // "1,234,567.89"
 */
export function formatNumber(value: number): string {
  return new Intl.NumberFormat("zh-CN").format(value);
}
