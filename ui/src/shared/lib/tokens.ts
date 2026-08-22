export function formatTokensForLocale(value: number, locale: string): string {
  const shortFormatter = new Intl.NumberFormat(locale, {
    maximumFractionDigits: 2,
    minimumFractionDigits: 0,
  });
  const fullFormatter = new Intl.NumberFormat(locale);
  const absoluteValue = Math.abs(value);
  if (absoluteValue >= 1_000_000_000) {
    return `${shortFormatter.format(value / 1_000_000_000)}B`;
  }
  if (absoluteValue >= 1_000_000) {
    return `${shortFormatter.format(value / 1_000_000)}M`;
  }
  if (absoluteValue >= 1_000) {
    return `${shortFormatter.format(value / 1_000)}K`;
  }
  return fullFormatter.format(value);
}

export function formatSignedTokensForLocale(value: number, locale: string): string {
  const sign = value > 0 ? "+" : "";
  return `${sign}${formatTokensForLocale(value, locale)}`;
}
