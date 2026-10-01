import { Money } from "./format";

// 2 ways to add up the cents of some prices (COLLIERY-T-1857). The code
// differs, but the job is the same.

export function sumCents(prices: Money[]): number {
  let total = 0;
  for (const price of prices) {
    total += price.cents;
  }
  return total;
}

export function addUpCents(prices: Money[]): number {
  const amounts = prices.map(
    (price) => price.cents,
  );
  return amounts.reduce((left, right) => left + right, 0);
}
