export type Currency = "EUR" | "USD";

export interface Money {
  cents: number;
  currency: Currency;
}

export enum Rounding {
  Up,
  Down,
}

export function formatPrice(money: Money): string {
  return `${(money.cents / 100).toFixed(2)} ${money.currency}`;
}
