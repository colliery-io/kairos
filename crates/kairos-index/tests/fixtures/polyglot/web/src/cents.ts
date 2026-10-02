import { Money } from "./format";

// 2 functions with the same job, where one calls the other
// (COLLIERY-T-2531). A function and its callee are not repeated code.

export function centsOf(prices: Money[]): number[] {
  const out: number[] = [];
  for (const price of prices) {
    out.push(price.cents);
  }
  return out;
}

export function listCents(prices: Money[]): number[] {
  const cents = centsOf(prices);
  if (cents.length === 0) {
    return [];
  }
  return cents;
}
