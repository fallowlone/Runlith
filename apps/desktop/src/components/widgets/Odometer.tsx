import { useEffect, useState } from "react";

const DIGITS = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

/**
 * Rolling digits, moved by transform only. Hidden from assistive tech: the card
 * holds the plain number, so a screen reader never hears the digit columns.
 */
export function Odometer({ value }: { value: number }) {
  // Starts from zero and rolls to the value, so a new number visibly arrives.
  const [shown, setShown] = useState(0);
  useEffect(() => {
    const frame = requestAnimationFrame(() => setShown(value));
    return () => cancelAnimationFrame(frame);
  }, [value]);

  const width = String(value).length;
  const digits = String(shown).padStart(width, "0").slice(-width).split("");
  return (
    <span className="odo" aria-hidden="true">
      {digits.map((digit, index) => (
        // Keyed from the right: a new leading digit does not reset the others.
        <span key={digits.length - index} className="dg">
          <span className="col" style={{ transform: `translateY(${-Number(digit)}em)` }}>
            {DIGITS.map((item) => <span key={item}>{item}</span>)}
          </span>
        </span>
      ))}
    </span>
  );
}
