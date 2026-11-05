#!/usr/bin/env bash
# Playwright's Linux system packages for the UI jobs, without a mirror on every run
# (FR 9QTPNR). Run from apps/desktop on a GitHub-hosted Ubuntu runner.
#
#   parse     prints the packages a dry run's output on stdin reports missing
#   list      prints `missing=<true|false>` and `key=<cache key>` for $GITHUB_OUTPUT: the
#             packages Playwright's own dry run finds missing (transitive ones included),
#             simulated against this runner image's package lists
#   download  fetches those packages' .deb files into the cache folder (the mirror is
#             asked only here, so once per runner image and Playwright version)
#   unlock    clears what a killed apt leaves behind, before a retry
#   install   installs the cached .deb files offline, then checks nothing is missing
set -euo pipefail

DEBS="${HOME}/.cache/playwright-debs"
LIST="${RUNNER_TEMP:-/tmp}/playwright-missing-packages.txt"

# The packages a dry run's output (on stdin) reports missing, one per line, sorted; empty
# when everything is installed. Fails on output it does not recognise.
parse_dry_run() {
  local out
  out="$(cat)"
  if grep -q "All system dependencies are installed" <<<"${out}"; then
    return 0
  fi
  if ! grep -q "^Missing system dependencies" <<<"${out}"; then
    echo "unexpected dry-run output:" >&2
    echo "${out}" >&2
    return 1
  fi
  sed -n '/^Missing system dependencies/,$p' <<<"${out}" | sed -n 's/^  \([^ ][^ ]*\)$/\1/p' | sort -u
}

# The packages Playwright's dry run finds missing on this machine. The dry run exits 1
# when something is missing, so its status is not the answer; its output is.
missing_packages() {
  { bunx playwright install-deps --dry-run chromium webkit 2>&1 || true; } | parse_dry_run
}

case "${1:-}" in
  parse)
    parse_dry_run
    ;;
  list)
    missing_packages >"${LIST}"
    if [ ! -s "${LIST}" ]; then
      echo "missing=false"
      exit 0
    fi
    echo "missing=true"
    image="${ImageOS:-unknown}-${ImageVersion:-unknown}"
    hash="$(sha256sum "${LIST}" | cut -c1-16)"
    echo "key=playwright-debs-${image}-$(bunx playwright --version | awk '{print $2}')-${hash}"
    ;;
  download)
    mkdir -p "${DEBS}"
    # The image's own package lists first; a mirror that has moved on gets one update.
    # shellcheck disable=SC2046
    if ! sudo apt-get install -y --download-only --no-install-recommends $(cat "${LIST}"); then
      sudo apt-get update
      # shellcheck disable=SC2046
      sudo apt-get install -y --download-only --no-install-recommends $(cat "${LIST}")
    fi
    cp /var/cache/apt/archives/*.deb "${DEBS}/"
    ls "${DEBS}" | wc -l | xargs echo "cached .deb files:"
    ;;
  unlock)
    sudo killall -q apt-get apt dpkg || true
    sudo rm -f /var/lib/dpkg/lock-frontend /var/lib/dpkg/lock \
      /var/cache/apt/archives/lock /var/lib/apt/lists/lock
    sudo dpkg --configure -a
    ;;
  install)
    sudo dpkg -i "${DEBS}"/*.deb
    left="$(missing_packages)"
    if [ -n "${left}" ]; then
      echo "still missing after the offline install:" >&2
      echo "${left}" >&2
      exit 1
    fi
    echo "all Playwright system dependencies are installed"
    ;;
  *)
    echo "usage: $0 parse|list|download|unlock|install" >&2
    exit 2
    ;;
esac
