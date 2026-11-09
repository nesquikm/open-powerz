/**
 * FR QFTCX1, AC-QFTCX1.3: whether this browser can reach a meter. The web edition needs
 * WebUSB or WebHID (Chrome, Edge); without both (Safari, Firefox) the page says so and
 * still offers the demo meter.
 */

/** The bits of `navigator` the check reads. */
export interface SupportProbe {
  usb?: unknown;
  hid?: unknown;
}

export type Support = "supported" | "not-supported";

/** `"supported"` when the browser has WebUSB or WebHID. */
export function detectSupport(probe: SupportProbe): Support {
  return probe.usb != null || probe.hid != null ? "supported" : "not-supported";
}
