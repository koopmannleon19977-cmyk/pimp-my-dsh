import { add, multiply } from "./src/calc.js";

const failures = [];
if (add(2, 3) !== 5) failures.push(`add(2,3) = ${add(2, 3)}, expected 5`);
if (multiply(4, 5) !== 20) failures.push(`multiply(4,5) = ${multiply(4, 5)}, expected 20`);

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}
console.log("OK");
