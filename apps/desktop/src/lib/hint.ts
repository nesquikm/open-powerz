/** One run of an error hint: prose (body font) or a quoted command or path (monospace). */
export interface HintPart {
  text: string;
  code: boolean;
}

/** Splits a hint at its backtick quotes: quoted runs become code parts (without the
 * backticks), the rest prose; an unmatched backtick stays plain text, empty parts drop. */
export function splitHint(hint: string): HintPart[] {
  const parts: HintPart[] = [];
  const push = (text: string, code: boolean) => {
    if (text) parts.push({ text, code });
  };
  const re = /`([^`]*)`/g;
  let last = 0;
  for (const m of hint.matchAll(re)) {
    push(hint.slice(last, m.index), false);
    push(m[1] ?? "", true);
    last = m.index + m[0].length;
  }
  push(hint.slice(last), false);
  return parts;
}
